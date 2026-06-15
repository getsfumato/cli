use std::{ffi::OsString, path::Path, process::Stdio};

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
) -> Result<()> {
    let status = Command::new("marp")
        .args(command_args(markdown_path, theme_css_path, pdf_path))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .status()
        .await;

    let status = match status {
        Ok(status) => status,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(MarpError::Missing.into());
        }
        Err(error) => return Err(error).context("Could not start Marp CLI"),
    };

    if !status.success() {
        bail!("Marp CLI exited with status {status}");
    }

    Ok(())
}

fn command_args(markdown_path: &Path, theme_css_path: &Path, pdf_path: &Path) -> Vec<OsString> {
    vec![
        "--theme".into(),
        theme_css_path.as_os_str().to_owned(),
        markdown_path.as_os_str().to_owned(),
        "-o".into(),
        pdf_path.as_os_str().to_owned(),
    ]
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../../tests/unit/renderers_marp.rs"]
mod tests;
