use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use tokio::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum MarpError {
    #[error("Marp CLI is not installed. Install @marp-team/marp-cli to export PDFs.")]
    Missing,
}

pub async fn render_pdf(
    markdown_path: &Path,
    theme_css_path: &Path,
    pdf_path: &Path,
    browser_path: Option<&Path>,
) -> Result<()> {
    let args = command_args(markdown_path, theme_css_path, pdf_path, browser_path)?;
    let output = Command::new("marp").args(args).output().await;

    let output = match output {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(MarpError::Missing.into());
        }
        Err(error) => return Err(error).context("Could not start Marp CLI"),
    };

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "Marp CLI exited with status {}{}{}",
            output.status,
            format_stream("stdout", stdout.trim()),
            format_stream("stderr", stderr.trim())
        );
    }

    Ok(())
}

fn command_args(
    markdown_path: &Path,
    theme_css_path: &Path,
    pdf_path: &Path,
    configured_browser_path: Option<&Path>,
) -> Result<Vec<OsString>> {
    let mut args = vec![
        "--allow-local-files".into(),
        "--theme".into(),
        theme_css_path.as_os_str().to_owned(),
        markdown_path.as_os_str().to_owned(),
        "-o".into(),
        pdf_path.as_os_str().to_owned(),
    ];
    if let Some(browser_path) = resolved_browser_path(configured_browser_path)? {
        args.splice(
            0..0,
            [
                "--browser".into(),
                "chrome".into(),
                "--browser-path".into(),
                browser_path.as_os_str().to_owned(),
            ],
        );
    }
    Ok(args)
}

fn format_stream(label: &str, value: &str) -> String {
    if value.is_empty() {
        String::new()
    } else {
        format!("\n\n{label}:\n{value}")
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

fn resolved_browser_path(configured_browser_path: Option<&Path>) -> Result<Option<PathBuf>> {
    if let Some(path) = configured_browser_path {
        if !path.is_file() {
            bail!(
                "Configured Marp browser path does not exist or is not a file: {}",
                path.display()
            );
        }
        return Ok(Some(path.to_path_buf()));
    }

    Ok(detected_browser_path())
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../../tests/unit/renderers_marp.rs"]
mod tests;
