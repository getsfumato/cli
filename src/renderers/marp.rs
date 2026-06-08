use std::{path::Path, process::Stdio};

use anyhow::{Context, Result, bail};
use tokio::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum MarpError {
    #[error("Marp CLI is not installed. Install @marp-team/marp-cli to export PDFs.")]
    Missing,
}

pub async fn render_pdf(markdown_path: &Path, pdf_path: &Path) -> Result<()> {
    let status = Command::new("marp")
        .arg(markdown_path)
        .arg("-o")
        .arg(pdf_path)
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
