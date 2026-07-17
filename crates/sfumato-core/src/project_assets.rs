//! Reusable project-owned assets such as logos and icon files.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{errors::SfumatoResult, sfumato_bail as bail};

/// Current project-asset manifest schema.
pub const PROJECT_ASSET_SCHEMA_VERSION: u32 = 1;

/// One validated reusable asset available to generation workflows.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProjectAsset {
    /// Stable project-local identifier.
    pub name: String,
    /// Human-readable model context.
    pub description: String,
    /// Detected media type.
    pub media_type: String,
    /// Original filename retained for presentation.
    pub filename: String,
    /// Absolute adapter-resolved source file.
    pub path: PathBuf,
    /// SHA-256 content digest.
    pub content_hash: String,
}

/// Model-facing reference to a reusable asset staged for one generation.
#[derive(Clone, Debug, Serialize)]
pub struct ProjectAssetReference {
    /// Stable project-local identifier.
    pub name: String,
    /// Human-readable intended use.
    pub description: String,
    /// Media type.
    pub media_type: String,
    /// Exact renderer-relative path the model may embed.
    pub reference: String,
    /// SHA-256 digest of the staged reusable file.
    pub content_hash: String,
}

/// Port for managing portable reusable assets under a project root.
pub trait ProjectAssetCatalog: Send + Sync {
    /// Lists all reusable assets in stable name order.
    fn list(&self, project_root: &Path) -> SfumatoResult<Vec<ProjectAsset>>;
    /// Loads one reusable asset by name.
    fn load(&self, project_root: &Path, name: &str) -> SfumatoResult<ProjectAsset>;
    /// Copies and registers a local file as a project asset.
    fn add(
        &self,
        project_root: &Path,
        source: &Path,
        name: Option<&str>,
        description: Option<&str>,
    ) -> SfumatoResult<ProjectAsset>;
    /// Removes one registration and its managed copy.
    fn remove(&self, project_root: &Path, name: &str) -> SfumatoResult<ProjectAsset>;
}

/// Validates a project asset identifier.
pub fn validate_project_asset_name(name: &str) -> SfumatoResult<()> {
    if name.is_empty()
        || name.starts_with('-')
        || name.ends_with('-')
        || !name.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
    {
        bail!("Invalid project asset name '{name}'. Use lowercase letters, numbers, and hyphens.");
    }
    Ok(())
}
