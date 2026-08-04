use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Serialize;
use sfumato_core::{
    errors::{ErrorClass, OperationStage, SfumatoError, SfumatoResult},
    operation::OperationContext,
    renderers::{DiagramRenderer, MermaidThemeConfig},
};
use tokio::process::Command;

use crate::{renderers::marp::resolved_browser_path, runtime::run_command};

/// Mermaid CLI adapter producing transparent themed SVG files.
#[derive(Clone, Copy, Debug, Default)]
pub struct MermaidCliRenderer;

#[async_trait]
impl DiagramRenderer for MermaidCliRenderer {
    async fn render_svg(
        &self,
        input_path: &Path,
        output_path: &Path,
        theme: &MermaidThemeConfig,
        browser_path: Option<&Path>,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<String> {
        let result: Result<String> = async {
            let puppeteer_config = write_puppeteer_config(output_path, browser_path)?;
            let mermaid_config = write_mermaid_config(output_path, theme)?;
            let mut command = Command::new("mmdc");
            command.args(mermaid_cli_args(
                input_path,
                output_path,
                puppeteer_config.as_deref(),
                Some(&mermaid_config),
            ));
            let output = run_command(&mut command, operation, stage).await;
            remove_config(puppeteer_config.as_deref());
            remove_config(Some(&mermaid_config));

            let output = match output {
                Ok(output) => output,
                Err(error)
                    if error
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
                {
                    bail!(
                        "Mermaid CLI is not installed. Install @mermaid-js/mermaid-cli to render Mermaid diagrams."
                    );
                }
                Err(error) => return Err(error).context("Could not run Mermaid CLI"),
            };
            if !output.status.success() {
                bail!(
                    "Mermaid CLI exited with status {}{}{}",
                    output.status,
                    format_stream("stdout", String::from_utf8_lossy(&output.stdout).trim()),
                    format_stream("stderr", String::from_utf8_lossy(&output.stderr).trim())
                );
            }

            let svg = std::fs::read_to_string(output_path).with_context(|| {
                format!("Could not read rendered diagram {}", output_path.display())
            })?;
            validate_svg(&svg)?;
            Ok(svg)
        }
        .await;
        result.map_err(|error| render_error(error, stage))
    }
}

fn render_error(error: anyhow::Error, stage: OperationStage) -> SfumatoError {
    if let Some(error) = error.downcast_ref::<SfumatoError>() {
        let mut error = error.clone();
        if error.stage.is_none() {
            error.stage = Some(stage);
        }
        return error;
    }
    let message = format!("{error:#}");
    let class = if message.contains("is not installed") {
        ErrorClass::Unavailable
    } else {
        ErrorClass::Permanent
    };
    SfumatoError::render(class, message).at_stage(stage)
}

fn validate_svg(svg: &str) -> Result<()> {
    if !svg.trim_start().starts_with("<svg") {
        bail!("Diagram renderer did not return an SVG document");
    }
    Ok(())
}

fn mermaid_cli_args(
    input_path: &Path,
    output_path: &Path,
    puppeteer_config: Option<&Path>,
    mermaid_config: Option<&Path>,
) -> Vec<OsString> {
    let mut args = vec![
        "-i".into(),
        input_path.as_os_str().to_owned(),
        "-o".into(),
        output_path.as_os_str().to_owned(),
        "--backgroundColor".into(),
        "transparent".into(),
    ];
    if let Some(config) = puppeteer_config {
        args.extend(["-p".into(), config.as_os_str().to_owned()]);
    }
    if let Some(config) = mermaid_config {
        args.extend(["-c".into(), config.as_os_str().to_owned()]);
    }
    args
}

/// Opt-out for environments where the Chromium sandbox cannot start.
///
/// The usual case is a container running as root, where Chrome refuses to sandbox
/// and exits. That is a property of the environment, so it is named by the
/// environment rather than assumed for everyone.
const DISABLE_SANDBOX_ENV: &str = "SFUMATO_DISABLE_BROWSER_SANDBOX";

#[derive(Serialize)]
struct PuppeteerConfig<'a> {
    #[serde(rename = "executablePath")]
    executable_path: &'a Path,
    args: Vec<&'static str>,
}

fn write_puppeteer_config(
    output_path: &Path,
    configured: Option<&Path>,
) -> Result<Option<PathBuf>> {
    // Shares `resolved_browser_path` with the slide and page renderers rather than
    // scanning `/Applications` alone: `marp.browser_path` exists for a browser that
    // is not in the default location, and a configured path that does not exist is
    // an error worth reporting instead of silently falling back to a scan.
    let Some(browser_path) = resolved_browser_path(configured)? else {
        return Ok(None);
    };
    let path = output_path.with_extension("puppeteer.json");
    // The sandbox stays on. What `mmdc` loads is Mermaid source written by a model
    // and never reviewed by the user, so this is the layer that keeps a renderer
    // bug in the page from reaching the rest of the machine. It was disabled
    // unconditionally and without explanation; rendering was measured to work with
    // it enabled, so there is nothing to trade away by default. The other two
    // browser launch sites already leave it alone.
    let rendered = serde_json::to_string(&PuppeteerConfig {
        executable_path: &browser_path,
        args: sandbox_args(std::env::var(DISABLE_SANDBOX_ENV).ok().as_deref()),
    })
    .context("Could not render Puppeteer config")?;
    std::fs::write(&path, rendered)
        .with_context(|| format!("Could not write {}", path.display()))?;
    Ok(Some(path))
}

/// Returns the browser arguments, which are empty unless the sandbox is opted out.
///
/// Takes the setting rather than reading the environment so the decision can be
/// tested without mutating process-global state.
fn sandbox_args(opt_out: Option<&str>) -> Vec<&'static str> {
    let disabled = opt_out.is_some_and(|value| matches!(value.trim(), "1" | "true" | "TRUE"));
    if disabled {
        vec!["--no-sandbox"]
    } else {
        Vec::new()
    }
}

fn write_mermaid_config(output_path: &Path, config: &MermaidThemeConfig) -> Result<PathBuf> {
    let path = output_path.with_extension("mermaid.json");
    let rendered = serde_json::to_string(config).context("Could not render Mermaid config")?;
    std::fs::write(&path, rendered)
        .with_context(|| format!("Could not write {}", path.display()))?;
    Ok(path)
}

fn remove_config(path: Option<&Path>) {
    if let Some(path) = path {
        let _ = std::fs::remove_file(path);
    }
}

fn format_stream(label: &str, value: &str) -> String {
    if value.is_empty() {
        String::new()
    } else {
        format!("\n\n{label}:\n{value}")
    }
}

#[cfg(test)]
#[path = "../../tests/unit/renderers_diagrams.rs"]
mod tests;
