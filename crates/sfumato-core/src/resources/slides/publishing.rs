//! Publication policy for processed slide artifacts.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::filesystem::WorkspaceFileSystem;

/// Result of publishing, or removing a stale copy of, a generated PDF.
pub(super) struct PdfPublication {
    pub(super) path: Option<PathBuf>,
    pub(super) warning: Option<String>,
}

/// Publishes a committed PDF without changing the committed revision outcome.
pub(super) fn publish_pdf(
    workspace: &dyn WorkspaceFileSystem,
    publish_root: Option<&Path>,
    committed_pdf: Option<&Path>,
    staged_pdf_path: &Path,
) -> Result<PdfPublication> {
    let Some(destination) = publish_root else {
        return Ok(PdfPublication {
            path: None,
            warning: None,
        });
    };

    if let Some(pdf) = committed_pdf {
        return Ok(match workspace.publish_atomic(pdf, destination) {
            Ok(path) => PdfPublication {
                path: Some(path),
                warning: None,
            },
            Err(error) => PdfPublication {
                path: None,
                warning: Some(format!(
                    "Committed the workspace revision, but could not publish its PDF: {error:#}"
                )),
            },
        });
    }

    let stale_pdf = destination.join(
        staged_pdf_path
            .file_name()
            .context("Generated PDF path must have a filename")?,
    );
    let warning = workspace.remove_file(&stale_pdf).err().map(|error| {
        format!("No PDF was generated and the stale published PDF could not be removed: {error:#}")
    });
    Ok(PdfPublication {
        path: None,
        warning,
    })
}

#[cfg(test)]
#[path = "../../../tests/unit/resources_slides_publishing.rs"]
mod tests;
