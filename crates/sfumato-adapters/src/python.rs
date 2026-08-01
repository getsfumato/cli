//! Managed `uv` Python environments and disposable execution of generated code.
//!
//! Environments live under `~/.sfumato/python/<layer>/.venv` and are keyed by the
//! exact requirement set they were built from, so asking for one twice is free
//! and asking for a different pin set never silently reuses the wrong
//! interpreter. Generated code never lands in a managed directory: it is written
//! into a temporary run directory, syntax-checked, executed there, and the
//! directory is dropped once the declared outputs have been copied out.

use std::{collections::BTreeMap, fs, path::PathBuf};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use sfumato_core::{
    errors::{ErrorClass, OperationStage, SfumatoError, SfumatoResult},
    operation::OperationContext,
    python::{
        PythonEnvironmentSpec, PythonRunRequest, PythonRunResult, PythonRuntime,
        validate_requirement, validate_run_path,
    },
    renderers::RendererStatus,
};
use sha2::{Digest, Sha256};
use tokio::process::Command;

use crate::runtime::run_command;

const ENVIRONMENT_MANIFEST: &str = include_str!("../assets/python-environments/manifest.toml");

/// How much of a failing script's stderr travels back to the caller.
///
/// A Python traceback ends with the line that actually failed, so the tail is
/// the useful part; the cap exists because a matplotlib deprecation storm can
/// otherwise bury it under hundreds of warnings.
const MAX_ERROR_LINES: usize = 24;

#[derive(Deserialize)]
struct EnvironmentManifest {
    schema_version: u32,
    environments: BTreeMap<String, EnvironmentEntry>,
}

#[derive(Clone, Debug, Deserialize)]
struct EnvironmentEntry {
    python: String,
    packages: Vec<String>,
}

/// Provisions pinned Python environments with `uv` and runs generated code.
#[derive(Clone, Debug)]
pub struct UvPythonRuntime {
    root: PathBuf,
}

impl UvPythonRuntime {
    /// Creates a runtime rooted at an explicit directory.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Creates a runtime under `~/.sfumato/python`.
    pub fn default_path() -> Result<Self> {
        Ok(Self::new(
            dirs::home_dir()
                .context("Home directory is unavailable")?
                .join(".sfumato/python"),
        ))
    }

    fn parse_manifest() -> Result<EnvironmentManifest> {
        let manifest: EnvironmentManifest = toml::from_str(ENVIRONMENT_MANIFEST)
            .context("Bundled Python environment manifest is invalid")?;
        if manifest.schema_version != 1 {
            bail!(
                "Python environment manifest declares unsupported schema version {}",
                manifest.schema_version
            );
        }
        Ok(manifest)
    }

    /// Resolves one environment's pins, with any extras appended.
    ///
    /// The extras are sorted and deduplicated so that two callers naming the same
    /// packages in different orders land on the same cached layer rather than
    /// paying to build two identical environments.
    fn resolve(environment: &str, extra_packages: &[String]) -> Result<ResolvedEnvironment> {
        let manifest = Self::parse_manifest()?;
        let entry = manifest.environments.get(environment).with_context(|| {
            format!(
                "Unknown Python environment '{environment}'. Available: {}",
                manifest
                    .environments
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
        for requirement in entry.packages.iter().chain(extra_packages) {
            validate_requirement(requirement).map_err(|error| anyhow::anyhow!("{error}"))?;
        }
        let mut extras = extra_packages.to_vec();
        extras.sort();
        extras.dedup();
        let layer = if extras.is_empty() {
            environment.to_string()
        } else {
            let digest = Sha256::digest(extras.join(" ").as_bytes());
            format!("{environment}-{:x}", digest)
                .chars()
                .take(environment.len() + 13)
                .collect()
        };
        let mut packages = entry.packages.clone();
        packages.extend(extras);
        Ok(ResolvedEnvironment {
            spec: PythonEnvironmentSpec {
                id: environment.to_string(),
                python: entry.python.clone(),
                packages: entry.packages.clone(),
            },
            layer,
            packages,
        })
    }

    fn layer_root(&self, layer: &str) -> PathBuf {
        self.root.join(layer)
    }

    fn interpreter(&self, layer: &str) -> PathBuf {
        self.layer_root(layer).join(".venv/bin/python")
    }

    /// Records which requirement set a layer was built from.
    ///
    /// Without this a pin bump in the manifest would keep resolving to the stale
    /// environment already on disk, and the pins would describe an intention
    /// rather than what is actually installed.
    fn stamp(&self, layer: &str) -> PathBuf {
        self.layer_root(layer).join("requirements.txt")
    }

    fn is_current(&self, resolved: &ResolvedEnvironment) -> bool {
        if !self.interpreter(&resolved.layer).is_file() {
            return false;
        }
        fs::read_to_string(self.stamp(&resolved.layer))
            .map(|recorded| recorded == resolved.requirements())
            .unwrap_or(false)
    }

    async fn provision(
        &self,
        resolved: &ResolvedEnvironment,
        operation: &OperationContext,
    ) -> Result<PathBuf> {
        let interpreter = self.interpreter(&resolved.layer);
        if self.is_current(resolved) {
            return Ok(interpreter);
        }
        let root = self.layer_root(&resolved.layer);
        fs::create_dir_all(&root)
            .with_context(|| format!("Could not create {}", root.display()))?;
        let environment = root.join(".venv");
        let mut venv = Command::new("uv");
        venv.args(["venv", "--python", &resolved.spec.python])
            .arg(&environment);
        checked(
            &mut venv,
            operation,
            OperationStage::Resolve,
            &format!("Python environment '{}' creation", resolved.layer),
        )
        .await?;
        let mut install = Command::new("uv");
        install
            .args(["pip", "install", "--python"])
            .arg(&interpreter);
        for requirement in &resolved.packages {
            install.arg(requirement);
        }
        checked(
            &mut install,
            operation,
            OperationStage::Resolve,
            &format!("Python environment '{}' installation", resolved.layer),
        )
        .await?;
        // Stamped only after the install succeeds, so a half-built environment is
        // rebuilt on the next call instead of being trusted.
        fs::write(self.stamp(&resolved.layer), resolved.requirements())
            .with_context(|| format!("Could not record {} requirements", resolved.layer))?;
        Ok(interpreter)
    }
}

struct ResolvedEnvironment {
    spec: PythonEnvironmentSpec,
    layer: String,
    packages: Vec<String>,
}

impl ResolvedEnvironment {
    fn requirements(&self) -> String {
        let mut lines = self.packages.clone();
        lines.insert(0, format!("# python {}", self.spec.python));
        lines.join("\n")
    }
}

#[async_trait]
impl PythonRuntime for UvPythonRuntime {
    async fn ensure(
        &self,
        environment: &str,
        extra_packages: &[String],
        operation: &OperationContext,
    ) -> SfumatoResult<PathBuf> {
        managed_result(
            async {
                let resolved = Self::resolve(environment, extra_packages)?;
                self.provision(&resolved, operation).await
            }
            .await,
            OperationStage::Resolve,
        )
    }

    fn interpreter_path(&self, environment: &str) -> SfumatoResult<PathBuf> {
        let resolved = Self::resolve(environment, &[])
            .map_err(|error| SfumatoError::validation(format!("{error:#}")))?;
        Ok(self.interpreter(&resolved.layer))
    }

    async fn run(
        &self,
        request: PythonRunRequest,
        operation: &OperationContext,
    ) -> SfumatoResult<PythonRunResult> {
        managed_result(
            async {
                for path in request.files.keys().chain(request.outputs.iter()) {
                    validate_run_path(path).map_err(|error| anyhow::anyhow!("{error}"))?;
                }
                if !request.files.contains_key(&request.entrypoint) {
                    bail!(
                        "Python entrypoint '{}' is not among the generated files",
                        request.entrypoint
                    );
                }
                let interpreter = self
                    .provision(
                        &Self::resolve(&request.environment, &request.extra_packages)?,
                        operation,
                    )
                    .await?;

                // The run directory is a `TempDir`: whatever the script wrote that
                // the caller did not ask for, including the script itself, is gone
                // when this function returns.
                let run = tempfile::Builder::new()
                    .prefix("sfumato-python")
                    .tempdir()
                    .context("Could not create a Python run directory")?;
                for (relative, contents) in &request.files {
                    let path = run.path().join(relative);
                    if let Some(parent) = path.parent() {
                        fs::create_dir_all(parent)
                            .with_context(|| format!("Could not create {}", parent.display()))?;
                    }
                    fs::write(&path, contents)
                        .with_context(|| format!("Could not write {}", path.display()))?;
                }

                // Compiling first separates "the model wrote invalid Python" from
                // "the script ran and failed", which are repaired differently.
                let mut compile = Command::new(&interpreter);
                compile.arg("-m").arg("py_compile");
                for relative in request.files.keys() {
                    compile.arg(relative);
                }
                compile.current_dir(run.path());
                checked(
                    &mut compile,
                    operation,
                    OperationStage::Render,
                    "Generated Python syntax check",
                )
                .await?;

                let mut execute = Command::new(&interpreter);
                execute.arg(&request.entrypoint);
                for argument in &request.arguments {
                    execute.arg(argument);
                }
                execute.current_dir(run.path());
                let output = checked_output(
                    &mut execute,
                    operation,
                    OperationStage::Render,
                    "Generated Python execution",
                )
                .await?;

                fs::create_dir_all(&request.output_dir).with_context(|| {
                    format!("Could not create {}", request.output_dir.display())
                })?;
                let mut outputs = Vec::new();
                for relative in &request.outputs {
                    let produced = run.path().join(relative);
                    if !produced.is_file() {
                        bail!("Generated Python did not produce '{relative}'");
                    }
                    let name = PathBuf::from(relative);
                    let name = name
                        .file_name()
                        .context("Python output path has no file name")?;
                    let destination = request.output_dir.join(name);
                    fs::copy(&produced, &destination).with_context(|| {
                        format!("Could not harvest {} from the run directory", relative)
                    })?;
                    outputs.push(destination);
                }
                Ok(PythonRunResult {
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                    outputs,
                })
            }
            .await,
            OperationStage::Render,
        )
    }

    async fn doctor(
        &self,
        environment: Option<&str>,
        _operation: &OperationContext,
    ) -> SfumatoResult<Vec<RendererStatus>> {
        managed_result(
            (|| {
                let manifest = Self::parse_manifest()?;
                let mut statuses = Vec::new();
                for (id, entry) in &manifest.environments {
                    if environment.is_some_and(|requested| requested != id) {
                        continue;
                    }
                    let resolved = Self::resolve(id, &[])?;
                    let installed = self.interpreter(id).is_file();
                    let current = self.is_current(&resolved);
                    let mut details = vec![format!("Python {}", entry.python)];
                    details.extend(entry.packages.iter().cloned());
                    if installed && !current {
                        details.push("Installed pins differ from the manifest".to_string());
                    }
                    if !installed {
                        details.push(format!(
                            "Not installed. It is provisioned on first use, or run `sfumato renderer install {id}`."
                        ));
                    }
                    statuses.push(RendererStatus {
                        id: id.clone(),
                        version: entry.packages.join(", "),
                        installed,
                        healthy: current,
                        details,
                    });
                }
                if let Some(requested) = environment.filter(|_| statuses.is_empty()) {
                    bail!("Unknown Python environment '{requested}'");
                }
                Ok(statuses)
            })(),
            OperationStage::Resolve,
        )
    }

    fn remove(&self, environment: &str) -> SfumatoResult<()> {
        let manifest =
            Self::parse_manifest().map_err(|error| SfumatoError::internal(error.to_string()))?;
        if !manifest.environments.contains_key(environment) {
            return Err(SfumatoError::validation(format!(
                "Unknown Python environment '{environment}'"
            )));
        }
        // Layers derived from this base carry it as a prefix, and leaving them
        // behind would keep the removed pins installed under another name.
        let prefix = format!("{environment}-");
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(SfumatoError::render(
                    ErrorClass::Permanent,
                    error.to_string(),
                ));
            }
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name != environment && !name.starts_with(&prefix) {
                continue;
            }
            fs::remove_dir_all(entry.path())
                .map_err(|error| SfumatoError::render(ErrorClass::Permanent, error.to_string()))?;
        }
        Ok(())
    }
}

async fn checked(
    command: &mut Command,
    operation: &OperationContext,
    stage: OperationStage,
    label: &str,
) -> Result<()> {
    checked_output(command, operation, stage, label)
        .await
        .map(|_| ())
}

async fn checked_output(
    command: &mut Command,
    operation: &OperationContext,
    stage: OperationStage,
    label: &str,
) -> Result<std::process::Output> {
    let output = run_command(command, operation, stage)
        .await
        .with_context(|| format!("Could not run {label}"))?;
    if !output.status.success() {
        bail!(
            "{label} failed with {}:\n{}",
            output.status,
            failure_tail(
                &String::from_utf8_lossy(&output.stdout),
                &String::from_utf8_lossy(&output.stderr),
            )
        );
    }
    Ok(output)
}

/// Keeps the end of a failing run, where Python puts the reason.
fn failure_tail(stdout: &str, stderr: &str) -> String {
    let combined = if stderr.trim().is_empty() {
        stdout
    } else {
        stderr
    };
    let lines = combined.lines().collect::<Vec<_>>();
    let start = lines.len().saturating_sub(MAX_ERROR_LINES);
    let mut message = String::new();
    if start > 0 {
        message.push_str(&format!("… {start} earlier line(s) omitted\n"));
    }
    message.push_str(&lines[start..].join("\n"));
    message
}

fn managed_result<T>(result: Result<T>, stage: OperationStage) -> SfumatoResult<T> {
    result.map_err(|error| match error.downcast::<SfumatoError>() {
        Ok(mut error) => {
            if error.stage.is_none() {
                error.stage = Some(stage);
            }
            error
        }
        Err(error) => {
            SfumatoError::render(ErrorClass::Permanent, format!("{error:#}")).at_stage(stage)
        }
    })
}

#[cfg(test)]
#[path = "../tests/unit/python.rs"]
mod tests;
