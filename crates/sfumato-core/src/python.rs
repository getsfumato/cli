//! Managed Python environments and one-shot execution of generated code.
//!
//! Two workflows need to run Python that a model wrote: the Manim video engine
//! and the chart tool. Both want the same guarantees — a pinned interpreter, a
//! pinned dependency set, a syntax gate before anything executes, and a working
//! directory that does not survive the call. Expressing that once here keeps the
//! generated code from ever reaching the artifact store: only the files a caller
//! named as outputs are harvested, and the directory they were produced in is
//! gone by the time the call returns.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use async_trait::async_trait;
use serde::Serialize;

use crate::{
    errors::{ErrorClass, SfumatoError, SfumatoResult},
    operation::OperationContext,
    renderers::RendererStatus,
};

/// One managed environment's pinned interpreter and dependency set.
#[derive(Clone, Debug, Serialize)]
pub struct PythonEnvironmentSpec {
    /// Stable environment ID used by callers, e.g. `charting`.
    pub id: String,
    /// Interpreter version requested from the environment manager.
    pub python: String,
    /// Fully pinned requirement strings installed into the base environment.
    pub packages: Vec<String>,
}

/// One request to execute generated Python in a managed environment.
pub struct PythonRunRequest {
    /// Environment ID declared by the bundled manifest.
    pub environment: String,
    /// Additional requirements layered on top of the pinned base.
    ///
    /// Validated by the caller against the project's allowlist before it gets
    /// here: this layer decides how a package is installed, not whether the
    /// project permits it.
    pub extra_packages: Vec<String>,
    /// Generated files written into the run directory, keyed by relative path.
    pub files: BTreeMap<String, String>,
    /// Relative path of the module executed after the syntax gate passes.
    pub entrypoint: String,
    /// Arguments appended after the entrypoint.
    pub arguments: Vec<String>,
    /// Relative paths harvested out of the run directory before it is dropped.
    pub outputs: Vec<String>,
    /// Directory the harvested outputs are copied into.
    pub output_dir: PathBuf,
}

/// What one completed run produced.
#[derive(Clone, Debug)]
pub struct PythonRunResult {
    /// Captured standard output, useful when a script reports what it drew.
    pub stdout: String,
    /// Captured standard error, retained even on success for diagnostics.
    pub stderr: String,
    /// Harvested output paths, in the order they were requested.
    pub outputs: Vec<PathBuf>,
}

/// Port for provisioning managed Python environments and running code in them.
#[async_trait]
pub trait PythonRuntime: Send + Sync {
    /// Provisions an environment and returns its interpreter path.
    ///
    /// Idempotent: an environment that already carries the requested pins is
    /// returned without touching the network.
    async fn ensure(
        &self,
        environment: &str,
        extra_packages: &[String],
        operation: &OperationContext,
    ) -> SfumatoResult<PathBuf>;

    /// Reports where an environment's interpreter lives without provisioning it.
    ///
    /// Health checks and renderers that shell out to an installed console script
    /// need the location before anything is installed, and asking for it must not
    /// be the thing that triggers a download.
    fn interpreter_path(&self, environment: &str) -> SfumatoResult<PathBuf>;

    /// Writes, syntax-checks, and executes generated code, then harvests outputs.
    async fn run(
        &self,
        request: PythonRunRequest,
        operation: &OperationContext,
    ) -> SfumatoResult<PythonRunResult>;

    /// Reports installation and health state for the managed environments.
    async fn doctor(
        &self,
        environment: Option<&str>,
        operation: &OperationContext,
    ) -> SfumatoResult<Vec<RendererStatus>>;

    /// Removes one managed environment and every layer derived from it.
    fn remove(&self, environment: &str) -> SfumatoResult<()>;
}

/// Operations a generated script may not reach for.
///
/// This is a screen, not a sandbox: it rejects the obvious escapes early so a
/// model gets a clear refusal instead of a confusing runtime failure, and it
/// keeps a careless script from touching the machine on the way to the real
/// isolation the run directory provides. Anything genuinely hostile needs the
/// project-level `allow_python` gate to have been opened first.
const FORBIDDEN_OPERATIONS: [&str; 10] = [
    "subprocess",
    "socket",
    "requests",
    "urllib",
    "open(",
    "exec(",
    "eval(",
    "__import__",
    "environ",
    "breakpoint(",
];

/// Modules generated code has a reason to import.
///
/// An allowlist rather than a denylist, because a denylist of module names loses:
/// `import os` was refused while `from os import path` — the same import, spelled
/// the other way — passed, and `pathlib` reached the filesystem without ever
/// writing `open(`. Naming what is permitted makes a paraphrase fail by default.
///
/// The plotting and animation packages come from the bundled environment
/// manifest; the rest are pure-computation parts of the standard library that
/// chart and scene code genuinely uses.
const PERMITTED_MODULES: &[&str] = &[
    // Provisioned environments.
    "matplotlib",
    "mpl_toolkits",
    "numpy",
    "sympy",
    "manim",
    // Computation and formatting only: no filesystem, process, or network reach.
    "cmath",
    "colorsys",
    "collections",
    "dataclasses",
    "datetime",
    "decimal",
    "enum",
    "fractions",
    "functools",
    "itertools",
    "json",
    "math",
    "operator",
    "random",
    "re",
    "statistics",
    "string",
    "textwrap",
    "typing",
    "unicodedata",
];

/// Rejects generated Python that reaches outside its run directory.
///
/// `extra_modules` are packages the project has explicitly authorised through
/// `security.python_packages`; without them a project that layers its own
/// dependency could not import it.
pub fn screen_python_source(source: &str, extra_modules: &[String]) -> SfumatoResult<()> {
    let lowercase = source.to_ascii_lowercase();
    for forbidden in FORBIDDEN_OPERATIONS {
        if lowercase.contains(forbidden) {
            return Err(SfumatoError::render(
                ErrorClass::InvalidOutput,
                format!("Generated Python contains forbidden operation '{forbidden}'"),
            ));
        }
    }
    for module in imported_modules(source) {
        if !is_permitted_module(&module, extra_modules) {
            return Err(SfumatoError::render(
                ErrorClass::InvalidOutput,
                format!(
                    "Generated Python imports '{module}', which is not permitted. \
                     Allowed modules: {}.",
                    PERMITTED_MODULES.join(", ")
                ),
            ));
        }
    }
    Ok(())
}

/// Reports whether a module, or the package it belongs to, is permitted.
fn is_permitted_module(module: &str, extra_modules: &[String]) -> bool {
    // `matplotlib.pyplot` is permitted by `matplotlib`; `os.path` is not
    // permitted by anything, which is the case the denylist missed.
    let root = module.split('.').next().unwrap_or(module);
    PERMITTED_MODULES.contains(&root)
        || extra_modules.iter().any(|allowed| {
            let allowed = allowed
                .split(['=', '>', '<', '!', '~', '['])
                .next()
                .unwrap_or(allowed)
                .trim()
                .replace('-', "_")
                .to_ascii_lowercase();
            allowed == root
        })
}

/// Collects every module named by an `import` or `from ... import` statement.
///
/// Line-oriented and deliberately literal: this is a screen, so a construct it
/// cannot read — a continuation, an import built at runtime — is not silently
/// accepted, because `__import__` and `exec(` are refused outright above.
fn imported_modules(source: &str) -> Vec<String> {
    let mut modules = Vec::new();
    for line in source.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("from ") {
            if let Some(module) = rest.split_whitespace().next() {
                modules.push(module.trim_start_matches('.').to_ascii_lowercase());
            }
        } else if let Some(rest) = line.strip_prefix("import ") {
            // `import a.b, c as d` names two modules.
            for part in rest.split(',') {
                if let Some(module) = part.split_whitespace().next() {
                    modules.push(module.to_ascii_lowercase());
                }
            }
        }
    }
    modules.retain(|module| !module.is_empty());
    modules
}

/// Rejects a requirement string that is not a plain pinned package name.
///
/// Requirements are passed to an installer as arguments, so anything that could
/// read as a flag, a URL, or a local path is refused rather than escaped: a
/// package name is the only thing a caller has a reason to ask for.
pub fn validate_requirement(requirement: &str) -> SfumatoResult<()> {
    let requirement = requirement.trim();
    let reject = |reason: &str| {
        Err(SfumatoError::config(format!(
            "Python requirement '{requirement}' is not allowed: {reason}"
        )))
    };
    if requirement.is_empty() {
        return reject("it is empty");
    }
    let (name, version) = match requirement.split_once("==") {
        Some((name, version)) => (name, Some(version)),
        None => (requirement, None),
    };
    if name.is_empty()
        || !name.starts_with(|value: char| value.is_ascii_alphanumeric())
        || !name
            .chars()
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, '.' | '_' | '-'))
    {
        return reject("the package name must be alphanumeric with '.', '_', or '-'");
    }
    match version {
        None => Ok(()),
        Some(version)
            if !version.is_empty()
                && version.starts_with(|value: char| value.is_ascii_digit())
                && version.chars().all(|value| {
                    value.is_ascii_alphanumeric() || matches!(value, '.' | '+' | '-')
                }) =>
        {
            Ok(())
        }
        Some(_) => reject("the pinned version must start with a digit and be alphanumeric"),
    }
}

/// Rejects a relative path that would write outside the run directory.
pub fn validate_run_path(path: &str) -> SfumatoResult<()> {
    let candidate = Path::new(path);
    if path.trim().is_empty() {
        return Err(SfumatoError::config("Python run paths cannot be empty"));
    }
    if candidate.is_absolute()
        || candidate
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(SfumatoError::config(format!(
            "Python run path '{path}' must stay inside the run directory"
        )));
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/unit/python.rs"]
mod tests;
