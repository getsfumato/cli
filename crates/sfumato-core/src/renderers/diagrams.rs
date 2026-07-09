use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use tokio::process::Command;

#[derive(Clone, Debug, Default)]
pub struct MermaidDiagramRenderer;

impl MermaidDiagramRenderer {
    pub async fn render_svg(&self, input_path: &Path, output_path: &Path) -> Result<String> {
        let puppeteer_config = write_puppeteer_config(output_path)?;
        let output = Command::new("mmdc")
            .args(mermaid_cli_args(
                input_path,
                output_path,
                puppeteer_config.as_deref(),
            ))
            .output()
            .await;
        remove_puppeteer_config(puppeteer_config.as_deref());

        let output = match output {
            Ok(output) => output,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                bail!(
                    "Mermaid CLI is not installed. Install @mermaid-js/mermaid-cli to render Mermaid diagrams."
                );
            }
            Err(error) => return Err(error).context("Could not start Mermaid CLI"),
        };

        if !output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "Mermaid CLI exited with status {}{}{}",
                output.status,
                format_stream("stdout", stdout.trim()),
                format_stream("stderr", stderr.trim())
            );
        }

        let svg = std::fs::read_to_string(output_path).with_context(|| {
            format!("Could not read rendered diagram {}", output_path.display())
        })?;
        validate_svg(&svg)?;
        Ok(svg)
    }
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
) -> Vec<OsString> {
    let mut args = vec![
        "-i".into(),
        input_path.as_os_str().to_owned(),
        "-o".into(),
        output_path.as_os_str().to_owned(),
    ];

    if let Some(config) = puppeteer_config {
        args.extend(["-p".into(), config.as_os_str().to_owned()]);
    }

    args
}

#[derive(Serialize)]
struct PuppeteerConfig<'a> {
    #[serde(rename = "executablePath")]
    executable_path: &'a Path,
    args: [&'static str; 1],
}

fn write_puppeteer_config(output_path: &Path) -> Result<Option<PathBuf>> {
    let Some(browser_path) = detected_browser_path() else {
        return Ok(None);
    };

    let path = output_path.with_extension("puppeteer.json");
    let config = PuppeteerConfig {
        executable_path: &browser_path,
        args: ["--no-sandbox"],
    };
    let rendered = serde_json::to_string(&config).context("Could not render Puppeteer config")?;
    std::fs::write(&path, rendered)
        .with_context(|| format!("Could not write {}", path.display()))?;
    Ok(Some(path))
}

fn remove_puppeteer_config(path: Option<&Path>) {
    if let Some(path) = path {
        let _ = std::fs::remove_file(path);
    }
}

fn detected_browser_path() -> Option<PathBuf> {
    [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|path| path.is_file())
}

fn format_stream(label: &str, value: &str) -> String {
    if value.is_empty() {
        String::new()
    } else {
        format!("\n\n{label}:\n{value}")
    }
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../../tests/unit/renderers_diagrams.rs"]
mod tests;
