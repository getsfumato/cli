//! Portable filesystem catalog for reusable project artifacts.

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
        ALL_THEMES, AddProjectAssetRequest, PROJECT_ASSET_SCHEMA_VERSION, ProjectAsset,
        ProjectAssetCatalog, ProjectAssetMetadata, ProjectAssetVariant, UpdateProjectAssetRequest,
        validate_asset_theme, validate_project_asset_name,
    },
};
use sha2::{Digest, Sha256};

use crate::config_files::{read_toml, write_toml};

/// Filesystem project-artifact catalog rooted below `<project>/.sfumato/assets`.
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
    #[serde(flatten)]
    metadata: ProjectAssetMetadata,
    #[serde(default)]
    variants: BTreeMap<String, VariantRecord>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct VariantRecord {
    media_type: String,
    filename: String,
    file: PathBuf,
    content_hash: String,
}

#[derive(Deserialize)]
struct ManifestVersion {
    schema_version: u32,
}

#[derive(Deserialize)]
struct LegacyManifest {
    assets: BTreeMap<String, LegacyAssetRecord>,
}

#[derive(Deserialize)]
struct LegacyAssetRecord {
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
                format!(
                    "Project artifact '{name}' was not found in {}",
                    root.display()
                )
            })?;
            record_to_asset(&root, name.to_string(), record)
        })())
    }

    fn add(
        &self,
        project_root: &Path,
        request: AddProjectAssetRequest<'_>,
    ) -> SfumatoResult<ProjectAsset> {
        asset_result((|| {
            validate_asset_theme(request.theme)?;
            let source = request.source.canonicalize().with_context(|| {
                format!(
                    "Could not resolve project artifact {}",
                    request.source.display()
                )
            })?;
            if !source.is_file() || fs::symlink_metadata(&source)?.file_type().is_symlink() {
                bail!("Project artifact source must be a regular non-symlink file");
            }
            let original_filename = source
                .file_name()
                .and_then(|value| value.to_str())
                .context("Project artifact filename must be valid UTF-8")?;
            let inferred = original_filename
                .rsplit_once('.')
                .map(|(stem, _)| stem)
                .unwrap_or(original_filename);
            let name = request
                .name
                .map(str::to_owned)
                .unwrap_or_else(|| slug(inferred));
            validate_project_asset_name(&name)?;
            let media_type = supported_media_type(&source)?;
            let bytes = fs::read(&source)?;
            if media_type == "image/svg+xml" {
                validate_svg_bytes(&bytes)?;
            }
            store_variant(
                project_root,
                &name,
                request.theme,
                &media_type,
                &bytes,
                Some(request.metadata),
            )
        })())
    }

    fn add_generated_variant(
        &self,
        project_root: &Path,
        name: &str,
        theme: &str,
        media_type: &str,
        bytes: &[u8],
    ) -> SfumatoResult<ProjectAsset> {
        asset_result((|| {
            validate_project_asset_name(name)?;
            validate_asset_theme(theme)?;
            validate_media_type(media_type)?;
            if media_type == "image/svg+xml" {
                validate_svg_bytes(bytes)?;
            }
            store_variant(project_root, name, theme, media_type, bytes, None)
        })())
    }

    fn update(
        &self,
        project_root: &Path,
        name: &str,
        changes: UpdateProjectAssetRequest,
    ) -> SfumatoResult<ProjectAsset> {
        asset_result((|| {
            validate_project_asset_name(name)?;
            let (root, mut manifest) = load_manifest(project_root)?;
            let record = manifest
                .assets
                .get_mut(name)
                .with_context(|| format!("Project artifact '{name}' was not found"))?;
            if let Some(value) = changes.description {
                record.metadata.description = value;
            }
            if let Some(value) = changes.alt_text {
                record.metadata.alt_text = value;
            }
            if let Some(value) = changes.tags {
                record.metadata.tags = normalized_tags(value)?;
            }
            if let Some(value) = changes.generation_prompt {
                record.metadata.generation_prompt =
                    value.filter(|prompt| !prompt.trim().is_empty());
            }
            if let Some((from, to)) = changes.variant_theme {
                validate_asset_theme(&from)?;
                validate_asset_theme(&to)?;
                if record.variants.contains_key(&to) {
                    bail!("Project artifact '{name}' already has a '{to}' variant");
                }
                let variant = record.variants.remove(&from).with_context(|| {
                    format!("Project artifact '{name}' has no '{from}' variant")
                })?;
                record.variants.insert(to, variant);
            }
            validate_metadata(&record.metadata)?;
            save_manifest(&root, &manifest)?;
            let record = manifest
                .assets
                .remove(name)
                .expect("updated artifact exists");
            record_to_asset(&root, name.to_string(), record)
        })())
    }

    fn remove(&self, project_root: &Path, name: &str) -> SfumatoResult<ProjectAsset> {
        asset_result((|| {
            validate_project_asset_name(name)?;
            let (root, mut manifest) = load_manifest(project_root)?;
            let record = manifest
                .assets
                .remove(name)
                .with_context(|| format!("Project artifact '{name}' was not found"))?;
            let asset = record_to_asset(&root, name.to_string(), record)?;
            save_manifest(&root, &manifest)?;
            for variant in asset.variants.values() {
                if variant.path.is_file() {
                    fs::remove_file(&variant.path)?;
                }
            }
            Ok(asset)
        })())
    }
}

fn store_variant(
    project_root: &Path,
    name: &str,
    theme: &str,
    media_type: &str,
    bytes: &[u8],
    metadata: Option<ProjectAssetMetadata>,
) -> Result<ProjectAsset> {
    validate_project_asset_name(name)?;
    validate_asset_theme(theme)?;
    validate_media_type(media_type)?;
    let (root, mut manifest) = load_manifest(project_root)?;
    let record = manifest
        .assets
        .entry(name.to_string())
        .or_insert_with(|| AssetRecord {
            metadata: metadata.clone().unwrap_or_else(|| ProjectAssetMetadata {
                description: "Reusable project visual artifact".into(),
                alt_text: String::new(),
                tags: Vec::new(),
                generation_prompt: None,
            }),
            variants: BTreeMap::new(),
        });
    if let Some(metadata) = metadata {
        record.metadata = metadata;
    }
    validate_metadata(&record.metadata)?;
    let content_hash = format!("{:x}", Sha256::digest(bytes));
    let extension = extension_for_media_type(media_type)?;
    let theme_slug = if theme == ALL_THEMES { "all" } else { theme };
    let filename = format!("{name}-{theme_slug}-{}.{}", &content_hash[..24], extension);
    let relative = PathBuf::from("files").join(&filename);
    fs::create_dir_all(root.join("files"))?;
    fs::write(root.join(&relative), bytes)?;
    let previous = record.variants.insert(
        theme.to_string(),
        VariantRecord {
            media_type: media_type.to_string(),
            filename,
            file: relative.clone(),
            content_hash,
        },
    );
    if let Err(error) = save_manifest(&root, &manifest) {
        let _ = fs::remove_file(root.join(&relative));
        return Err(error);
    }
    if let Some(previous) = previous {
        let previous_path = root.join(previous.file);
        if previous_path != root.join(&relative) && previous_path.is_file() {
            fs::remove_file(previous_path)?;
        }
    }
    let record = manifest
        .assets
        .remove(name)
        .expect("stored artifact exists");
    record_to_asset(&root, name.to_string(), record)
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
    let source = fs::read_to_string(&path)?;
    let version: ManifestVersion = toml::from_str(&source)?;
    match version.schema_version {
        PROJECT_ASSET_SCHEMA_VERSION => Ok((root, read_toml(&path)?)),
        1 => {
            let legacy: LegacyManifest = toml::from_str(&source)?;
            let manifest = AssetManifest {
                schema_version: PROJECT_ASSET_SCHEMA_VERSION,
                assets: legacy
                    .assets
                    .into_iter()
                    .map(|(name, record)| {
                        let variant = VariantRecord {
                            media_type: record.media_type,
                            filename: record.filename,
                            file: record.file,
                            content_hash: record.content_hash,
                        };
                        (
                            name,
                            AssetRecord {
                                metadata: ProjectAssetMetadata {
                                    alt_text: record.description.clone(),
                                    description: record.description,
                                    tags: Vec::new(),
                                    generation_prompt: None,
                                },
                                variants: BTreeMap::from([(ALL_THEMES.to_string(), variant)]),
                            },
                        )
                    })
                    .collect(),
            };
            save_manifest(&root, &manifest)?;
            Ok((root, manifest))
        }
        other => bail!("Unsupported project artifact schema {other}"),
    }
}

fn save_manifest(root: &Path, manifest: &AssetManifest) -> Result<()> {
    fs::create_dir_all(root)?;
    write_toml(&root.join("manifest.toml"), manifest)
}

fn record_to_asset(root: &Path, name: String, record: AssetRecord) -> Result<ProjectAsset> {
    validate_project_asset_name(&name)?;
    validate_metadata(&record.metadata)?;
    let variants = record
        .variants
        .into_iter()
        .map(|(theme, record)| {
            validate_asset_theme(&theme)?;
            if record.file.is_absolute()
                || record
                    .file
                    .components()
                    .any(|part| !matches!(part, std::path::Component::Normal(_)))
            {
                bail!("Project artifact '{name}' has an unsafe managed path");
            }
            let path = root.join(&record.file);
            if !path.is_file() || fs::symlink_metadata(&path)?.file_type().is_symlink() {
                bail!("Managed file for project artifact '{name}' is missing or unsafe");
            }
            let digest = format!("{:x}", Sha256::digest(fs::read(&path)?));
            if digest != record.content_hash {
                bail!("Managed file for project artifact '{name}' failed its integrity check");
            }
            Ok((
                theme.clone(),
                ProjectAssetVariant {
                    theme,
                    media_type: record.media_type,
                    filename: record.filename,
                    path,
                    content_hash: record.content_hash,
                },
            ))
        })
        .collect::<Result<_>>()?;
    Ok(ProjectAsset {
        name,
        metadata: record.metadata,
        variants,
    })
}

fn supported_media_type(path: &Path) -> Result<String> {
    let media_type = match path
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
        _ => bail!("Project artifacts currently support PNG, JPEG, WebP, GIF, and SVG files"),
    };
    Ok(media_type.to_string())
}

fn validate_media_type(media_type: &str) -> Result<()> {
    extension_for_media_type(media_type).map(|_| ())
}

fn extension_for_media_type(media_type: &str) -> Result<&'static str> {
    match media_type {
        "image/png" => Ok("png"),
        "image/jpeg" | "image/jpg" => Ok("jpg"),
        "image/webp" => Ok("webp"),
        "image/gif" => Ok("gif"),
        "image/svg+xml" => Ok("svg"),
        other => bail!("Unsupported project artifact media type '{other}'"),
    }
}

fn validate_svg_bytes(bytes: &[u8]) -> Result<()> {
    let source = std::str::from_utf8(bytes).context("SVG project artifact must be UTF-8")?;
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
            bail!("SVG project artifact contains forbidden active or remote content: {forbidden}");
        }
    }
    Ok(())
}

fn validate_metadata(metadata: &ProjectAssetMetadata) -> Result<()> {
    if metadata.description.trim().is_empty() {
        bail!("Project artifact description cannot be empty");
    }
    normalized_tags(metadata.tags.clone()).map(|_| ())
}

fn normalized_tags(tags: Vec<String>) -> Result<Vec<String>> {
    let mut tags = tags
        .into_iter()
        .map(|tag| tag.trim().to_ascii_lowercase())
        .filter(|tag| !tag.is_empty())
        .collect::<Vec<_>>();
    if tags.iter().any(|tag| {
        !tag.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
    }) {
        bail!("Project artifact tags use lowercase letters, numbers, and hyphens");
    }
    tags.sort();
    tags.dedup();
    Ok(tags)
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
