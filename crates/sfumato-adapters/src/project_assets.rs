//! Portable filesystem catalog for reusable project assets.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sfumato_core::{
    errors::{ErrorClass, ErrorCode, SfumatoError, SfumatoResult},
    project_assets::{
        PROJECT_ASSET_SCHEMA_VERSION, ProjectAsset, ProjectAssetCatalog,
        validate_project_asset_name,
    },
};
use sha2::{Digest, Sha256};

use crate::config_files::{read_toml, write_toml};

/// Filesystem project-asset catalog rooted below `<project>/.sfumato/assets`.
#[derive(Clone, Copy, Debug, Default)]
pub struct FilesystemProjectAssetCatalog;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AssetManifest {
    schema_version: u32,
    #[serde(default)]
    assets: BTreeMap<String, AssetRecord>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AssetRecord {
    description: String,
    media_type: String,
    filename: String,
    file: PathBuf,
    content_hash: String,
}

impl ProjectAssetCatalog for FilesystemProjectAssetCatalog {
    fn list(&self, project_root: &Path) -> SfumatoResult<Vec<ProjectAsset>> {
        asset_result((|| {
            let (root, manifest) = load_manifest(project_root)?;
            manifest
                .assets
                .into_iter()
                .map(|(name, record)| record_to_asset(&root, name, record))
                .collect()
        })())
    }

    fn load(&self, project_root: &Path, name: &str) -> SfumatoResult<ProjectAsset> {
        asset_result((|| {
            validate_project_asset_name(name)?;
            let (root, manifest) = load_manifest(project_root)?;
            let record = manifest.assets.get(name).cloned().with_context(|| {
                format!("Project asset '{name}' was not found in {}", root.display())
            })?;
            record_to_asset(&root, name.to_string(), record)
        })())
    }

    fn add(
        &self,
        project_root: &Path,
        source: &Path,
        name: Option<&str>,
        description: Option<&str>,
    ) -> SfumatoResult<ProjectAsset> {
        asset_result((|| {
            let source = source
                .canonicalize()
                .with_context(|| format!("Could not resolve project asset {}", source.display()))?;
            if !source.is_file() || fs::symlink_metadata(&source)?.file_type().is_symlink() {
                bail!("Project asset source must be a regular non-symlink file");
            }
            let filename = source
                .file_name()
                .and_then(|value| value.to_str())
                .context("Project asset filename must be valid UTF-8")?
                .to_string();
            let inferred = filename
                .rsplit_once('.')
                .map(|(stem, _)| stem)
                .unwrap_or(&filename);
            let asset_name = name.map(str::to_owned).unwrap_or_else(|| slug(inferred));
            validate_project_asset_name(&asset_name)?;
            let media_type = supported_media_type(&source)?;
            if media_type == "image/svg+xml" {
                validate_svg(&source)?;
            }
            let bytes = fs::read(&source)?;
            let content_hash = format!("{:x}", Sha256::digest(&bytes));
            let (root, mut manifest) = load_manifest(project_root)?;
            if manifest.assets.contains_key(&asset_name) {
                bail!("Project asset '{asset_name}' already exists");
            }
            fs::create_dir_all(root.join("files"))?;
            let managed_name = format!("{}-{}", asset_name, filename);
            let relative = PathBuf::from("files").join(&managed_name);
            fs::write(root.join(&relative), bytes)?;
            manifest.assets.insert(
                asset_name.clone(),
                AssetRecord {
                    description: description
                        .unwrap_or("Reusable project visual asset")
                        .to_string(),
                    media_type,
                    filename: managed_name,
                    file: relative.clone(),
                    content_hash,
                },
            );
            if let Err(error) = save_manifest(&root, &manifest) {
                let _ = fs::remove_file(root.join(&relative));
                return Err(error);
            }
            let record = manifest.assets.remove(&asset_name).expect("inserted");
            record_to_asset(&root, asset_name, record)
        })())
    }

    fn remove(&self, project_root: &Path, name: &str) -> SfumatoResult<ProjectAsset> {
        asset_result((|| {
            validate_project_asset_name(name)?;
            let (root, mut manifest) = load_manifest(project_root)?;
            let record = manifest
                .assets
                .remove(name)
                .with_context(|| format!("Project asset '{name}' was not found"))?;
            let asset = record_to_asset(&root, name.to_string(), record)?;
            save_manifest(&root, &manifest)?;
            if asset.path.is_file() {
                fs::remove_file(&asset.path)?;
            }
            Ok(asset)
        })())
    }
}

fn asset_root(project_root: &Path) -> PathBuf {
    project_root.join(".sfumato/assets")
}

fn load_manifest(project_root: &Path) -> Result<(PathBuf, AssetManifest)> {
    let root = asset_root(project_root);
    let path = root.join("manifest.toml");
    if !path.is_file() {
        return Ok((
            root,
            AssetManifest {
                schema_version: PROJECT_ASSET_SCHEMA_VERSION,
                assets: BTreeMap::new(),
            },
        ));
    }
    let manifest: AssetManifest = read_toml(&path)?;
    if manifest.schema_version != PROJECT_ASSET_SCHEMA_VERSION {
        bail!(
            "Unsupported project asset schema {}",
            manifest.schema_version
        );
    }
    Ok((root, manifest))
}

fn save_manifest(root: &Path, manifest: &AssetManifest) -> Result<()> {
    fs::create_dir_all(root)?;
    write_toml(&root.join("manifest.toml"), manifest)
}

fn record_to_asset(root: &Path, name: String, record: AssetRecord) -> Result<ProjectAsset> {
    validate_project_asset_name(&name)?;
    if record.file.is_absolute()
        || record
            .file
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        bail!("Project asset '{name}' has an unsafe managed path");
    }
    let path = root.join(&record.file);
    if !path.is_file() || fs::symlink_metadata(&path)?.file_type().is_symlink() {
        bail!("Managed file for project asset '{name}' is missing or unsafe");
    }
    let digest = format!("{:x}", Sha256::digest(fs::read(&path)?));
    if digest != record.content_hash {
        bail!("Managed file for project asset '{name}' failed its integrity check");
    }
    Ok(ProjectAsset {
        name,
        description: record.description,
        media_type: record.media_type,
        filename: record.filename,
        path,
        content_hash: record.content_hash,
    })
}

fn supported_media_type(path: &Path) -> Result<String> {
    let value = match path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        _ => bail!("Project assets currently support PNG, JPEG, WebP, GIF, and SVG files"),
    };
    Ok(value.to_string())
}

fn validate_svg(path: &Path) -> Result<()> {
    let source = fs::read_to_string(path).context("SVG project asset must be UTF-8")?;
    let lowercase = source.to_ascii_lowercase();
    for forbidden in [
        "<script",
        "javascript:",
        "onload=",
        "onerror=",
        "http://",
        "https://",
    ] {
        if lowercase.contains(forbidden) {
            bail!("SVG project asset contains forbidden active or remote content: {forbidden}");
        }
    }
    Ok(())
}

fn slug(value: &str) -> String {
    let mut output = value
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    while output.contains("--") {
        output = output.replace("--", "-");
    }
    output.trim_matches('-').to_string()
}

fn asset_result<T>(result: Result<T>) -> SfumatoResult<T> {
    result.map_err(|error| {
        if let Some(error) = error.downcast_ref::<SfumatoError>() {
            return error.clone();
        }
        let message = format!("{error:#}");
        let code = if message.contains("not found") {
            ErrorCode::NotFound
        } else {
            ErrorCode::Validation
        };
        SfumatoError::new(code, ErrorClass::Permanent, message)
    })
}

#[cfg(test)]
#[path = "../tests/unit/project_assets.rs"]
mod tests;
