//! Manifest-driven bundled JavaScript plugins for standalone pages.

use include_dir::{Dir, include_dir};
use serde::Deserialize;
use sfumato_core::{
    errors::{ErrorClass, SfumatoError, SfumatoResult},
    page_plugins::{PagePluginCatalog, PagePluginPackage, PagePluginSummary},
};
use sha2::{Digest, Sha256};

static PLUGINS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/assets/page-plugins");

/// Catalog backed by runtime assets embedded in the Sfumato binary.
#[derive(Clone, Copy, Debug, Default)]
pub struct BundledPagePluginCatalog;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginManifest {
    schema_version: u32,
    plugins: Vec<PluginEntry>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginEntry {
    id: String,
    name: String,
    version: String,
    api_global: String,
    runtime: String,
    guidance: String,
    license: String,
    runtime_hash: String,
}

impl PagePluginCatalog for BundledPagePluginCatalog {
    fn list(&self) -> SfumatoResult<Vec<PagePluginSummary>> {
        let mut values = manifest()?
            .plugins
            .into_iter()
            .map(summary)
            .collect::<Vec<_>>();
        values.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(values)
    }

    fn load(&self, id: &str) -> SfumatoResult<PagePluginPackage> {
        validate_id(id)?;
        let entry = manifest()?.plugins.into_iter().find(|entry| entry.id == id)
            .ok_or_else(|| SfumatoError::new(
                sfumato_core::errors::ErrorCode::NotFound,
                ErrorClass::Permanent,
                format!("Unknown page plugin '{id}'. Run `sfumato plugin list` to see available plugins."),
            ))?;
        let runtime_javascript = embedded_text(&entry.runtime)?;
        let actual_hash = format!("{:x}", Sha256::digest(runtime_javascript.as_bytes()));
        if actual_hash != entry.runtime_hash {
            return Err(SfumatoError::config(format!(
                "Bundled page plugin '{}' failed its runtime integrity check",
                entry.id
            )));
        }
        let guidance = embedded_text(&entry.guidance)?;
        let license_text = embedded_text(&entry.license)?;
        if guidance.trim().is_empty() || license_text.trim().is_empty() {
            return Err(SfumatoError::config(format!(
                "Bundled page plugin '{}' has incomplete guidance or license metadata",
                entry.id
            )));
        }
        Ok(PagePluginPackage {
            summary: summary(entry),
            guidance,
            runtime_javascript,
        })
    }
}

fn manifest() -> SfumatoResult<PluginManifest> {
    let raw = embedded_text("manifest.toml")?;
    let manifest: PluginManifest = toml::from_str(&raw).map_err(|error| {
        SfumatoError::config(format!("Invalid bundled page plugin manifest: {error}"))
    })?;
    if manifest.schema_version != 1 {
        return Err(SfumatoError::config(format!(
            "Unsupported bundled page plugin schema version {}",
            manifest.schema_version
        )));
    }
    for entry in &manifest.plugins {
        validate_id(&entry.id)?;
        if entry.version.trim().is_empty()
            || entry.api_global.trim().is_empty()
            || entry.runtime_hash.len() != 64
        {
            return Err(SfumatoError::config(format!(
                "Bundled page plugin '{}' has invalid metadata",
                entry.id
            )));
        }
    }
    Ok(manifest)
}

fn embedded_text(path: &str) -> SfumatoResult<String> {
    let file = PLUGINS.get_file(path).ok_or_else(|| {
        SfumatoError::config(format!("Bundled page plugin asset '{path}' is missing"))
    })?;
    file.contents_utf8().map(str::to_owned).ok_or_else(|| {
        SfumatoError::config(format!("Bundled page plugin asset '{path}' is not UTF-8"))
    })
}

fn summary(entry: PluginEntry) -> PagePluginSummary {
    PagePluginSummary {
        id: entry.id,
        name: entry.name,
        version: entry.version,
        api_global: entry.api_global,
        runtime_hash: entry.runtime_hash,
        license: entry.license,
    }
}

fn validate_id(id: &str) -> SfumatoResult<()> {
    if id.is_empty()
        || !id.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
        || id.starts_with('-')
        || id.ends_with('-')
    {
        return Err(SfumatoError::validation(format!(
            "Invalid page plugin ID '{id}'"
        )));
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/unit/page_plugins.rs"]
mod tests;
