//! Source-material and project-instruction ports.

use std::path::{Path, PathBuf};

use crate::errors::SfumatoResult;

/// UTF-8 source material supplied to a resource workflow.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceDocument {
    /// Canonical local source path.
    pub path: PathBuf,
    /// Budgeted source content.
    pub content: String,
}

/// Optional project-owned model guidance loaded from `SFUMATO.md`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectInstructions {
    /// Project-relative instruction file path.
    pub path: PathBuf,
    /// Raw instruction Markdown without prompt wrappers.
    pub content: String,
}

/// Port for deterministic, budgeted local source discovery and reading.
pub trait SourceReader: Send + Sync {
    /// Discovers supported source files and returns their budgeted contents.
    fn collect(&self, inputs: &[PathBuf]) -> SfumatoResult<Vec<SourceDocument>>;

    /// Loads optional project-level instructions.
    fn project_instructions(
        &self,
        project_root: &Path,
    ) -> SfumatoResult<Option<ProjectInstructions>>;
}
