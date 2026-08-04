use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use serde::Serialize;

use crate::{
    errors::{ErrorClass, OperationStage, SfumatoError, SfumatoResult as Result},
    filesystem::WorkspaceFileSystem,
    operation::OperationContext,
    project_assets::{ProjectAssetCatalog, ProjectAssetReference, ProjectAssetVariant},
    prompts::{PromptCatalog, PromptId, PromptProvenance, PromptRenderRequest, PromptVariables},
    providers::{ImageGenerationProvider, ImageGenerationRequest},
    themes::ThemePackage,
};

pub(crate) struct PreparedProjectAsset {
    source: PathBuf,
    destination: PathBuf,
    pub reference: ProjectAssetReference,
}

pub(crate) struct PreparedProjectAssets {
    pub assets: Vec<PreparedProjectAsset>,
    pub prompts: Vec<PromptProvenance>,
    pub warnings: Vec<String>,
}

pub(crate) struct PrepareProjectAssetsRequest<'a> {
    pub catalog: &'a dyn ProjectAssetCatalog,
    pub project_root: &'a Path,
    pub theme: &'a ThemePackage,
    pub image_provider: Option<&'a Arc<dyn ImageGenerationProvider>>,
    pub prompt_catalog: &'a dyn PromptCatalog,
    pub project_instructions: &'a str,
    pub output_dir: &'a Path,
    pub reference_prefix: &'a str,
    pub dry_run: bool,
    pub operation: &'a OperationContext,
}

#[derive(Serialize)]
struct ImagePromptContext<'a> {
    requested_prompt: &'a str,
    theme_name: &'a str,
    theme_colors: String,
    theme_fonts: String,
    project_instructions: &'a str,
}

#[derive(Serialize)]
struct RegenerationPromptContext<'a> {
    artifact_name: &'a str,
    artifact_description: &'a str,
    artifact_alt_text: &'a str,
    artifact_tags: &'a [String],
    generation_recipe: &'a str,
}

pub(crate) async fn prepare_project_assets(
    request: PrepareProjectAssetsRequest<'_>,
) -> Result<PreparedProjectAssets> {
    let mut assets = Vec::new();
    let mut prompts = Vec::new();
    let mut warnings = Vec::new();
    let listing = request.catalog.list(request.project_root)?;
    // A damaged record cannot be prepared, but the generation already reports what
    // it could not use, so it is warned about rather than silently absent.
    for entry in &listing.unreadable {
        warnings.push(format!(
            "Project artifact '{}' was skipped: {}",
            entry.name, entry.problem
        ));
    }
    for mut asset in listing.entries {
        let variant = if let Some(variant) = asset.resolve(&request.theme.manifest.name) {
            Some(variant.clone())
        } else if request.dry_run {
            warnings.push(format!(
                "Project artifact '{}' has no '{}' or wildcard variant; dry-run did not regenerate it.",
                asset.name, request.theme.manifest.name
            ));
            None
        } else if let (Some(provider), Some(recipe)) = (
            request.image_provider,
            asset.metadata.generation_prompt.as_deref(),
        ) {
            request.operation.checkpoint(OperationStage::Draft)?;
            let regeneration = request.prompt_catalog.render(PromptRenderRequest {
                id: PromptId::ProjectAssetRegenerationUser,
                variables: PromptVariables::from_serializable(&RegenerationPromptContext {
                    artifact_name: &asset.name,
                    artifact_description: &asset.metadata.description,
                    artifact_alt_text: &asset.metadata.alt_text,
                    artifact_tags: &asset.metadata.tags,
                    generation_recipe: recipe,
                })?,
            })?;
            prompts.push(regeneration.provenance);
            let context = ImagePromptContext {
                requested_prompt: &regeneration.text,
                theme_name: &request.theme.manifest.name,
                theme_colors: format_tokens(&request.theme.manifest.tokens.colors),
                theme_fonts: format_tokens(&request.theme.manifest.tokens.fonts),
                project_instructions: request.project_instructions,
            };
            let rendered = request.prompt_catalog.render(PromptRenderRequest {
                id: PromptId::ImageGenerationUser,
                variables: PromptVariables::from_serializable(&context)?,
            })?;
            prompts.push(rendered.provenance);
            let generated = provider
                .generate_image(
                    ImageGenerationRequest {
                        prompt: rendered.text,
                    },
                    request.operation,
                    OperationStage::Draft,
                )
                .await?;
            asset = request.catalog.add_generated_variant(
                request.project_root,
                &asset.name,
                &request.theme.manifest.name,
                &generated.media_type,
                &generated.bytes,
            )?;
            asset.resolve(&request.theme.manifest.name).cloned()
        } else {
            let reason = if request.image_provider.is_none() {
                "no image model is configured"
            } else {
                "its metadata has no generation prompt"
            };
            warnings.push(format!(
                "Project artifact '{}' was omitted because it has no '{}' or wildcard variant and {reason}.",
                asset.name, request.theme.manifest.name
            ));
            None
        };
        if let Some(variant) = variant {
            assets.push(prepared_asset(
                &asset.name,
                &asset.metadata,
                &variant,
                request.output_dir,
                request.reference_prefix,
            ));
        }
    }
    assets.sort_by(|left, right| left.reference.name.cmp(&right.reference.name));
    Ok(PreparedProjectAssets {
        assets,
        prompts,
        warnings,
    })
}

fn prepared_asset(
    name: &str,
    metadata: &crate::project_assets::ProjectAssetMetadata,
    variant: &ProjectAssetVariant,
    output_dir: &Path,
    reference_prefix: &str,
) -> PreparedProjectAsset {
    let reference = format!(
        "{}/{}",
        reference_prefix.trim_end_matches('/'),
        variant.filename
    );
    PreparedProjectAsset {
        source: variant.path.clone(),
        destination: output_dir.join(&variant.filename),
        reference: ProjectAssetReference {
            name: name.to_string(),
            description: metadata.description.clone(),
            alt_text: metadata.alt_text.clone(),
            tags: metadata.tags.clone(),
            theme: variant.theme.clone(),
            media_type: variant.media_type.clone(),
            reference,
            content_hash: variant.content_hash.clone(),
        },
    }
}

impl PreparedProjectAssets {
    pub fn references(&self) -> Vec<ProjectAssetReference> {
        self.assets
            .iter()
            .map(|asset| asset.reference.clone())
            .collect()
    }

    pub fn allowed_paths(&self) -> Vec<PathBuf> {
        self.assets
            .iter()
            .map(|asset| asset.destination.clone())
            .collect()
    }

    pub fn materialize_all(&self, workspace: &dyn WorkspaceFileSystem) -> Result<Vec<PathBuf>> {
        let mut paths = Vec::with_capacity(self.assets.len());
        for asset in &self.assets {
            workspace.copy_file(&asset.source, &asset.destination)?;
            paths.push(asset.destination.clone());
        }
        Ok(paths)
    }

    pub fn materialize_referenced(
        &self,
        document: &str,
        workspace: &dyn WorkspaceFileSystem,
    ) -> Result<(Vec<PathBuf>, Vec<ProjectAssetReference>)> {
        let mut paths = Vec::new();
        let mut references = Vec::new();
        for asset in &self.assets {
            if document.contains(&asset.reference.reference) {
                workspace.copy_file(&asset.source, &asset.destination)?;
                paths.push(asset.destination.clone());
                references.push(asset.reference.clone());
            }
        }
        Ok((paths, references))
    }

    #[cfg(test)]
    fn referenced_names(&self, document: &str) -> Vec<String> {
        self.assets
            .iter()
            .filter(|asset| document.contains(&asset.reference.reference))
            .map(|asset| asset.reference.name.clone())
            .collect()
    }

    pub fn stage_referenced(
        &self,
        document: &str,
        root: &Path,
        workspace: &dyn WorkspaceFileSystem,
    ) -> Result<()> {
        for asset in &self.assets {
            if document.contains(&asset.reference.reference) {
                workspace.copy_file(&asset.source, &root.join(&asset.reference.reference))?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "../../tests/unit/resources_project_assets.rs"]
mod tests;

fn format_tokens(tokens: &std::collections::BTreeMap<String, String>) -> String {
    if tokens.is_empty() {
        return "unspecified".into();
    }
    tokens
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn retain_referenced_generated_assets(
    paths: Vec<PathBuf>,
    document: &str,
    reference_prefix: &str,
    workspace: &dyn WorkspaceFileSystem,
) -> Result<Vec<PathBuf>> {
    let mut retained = Vec::new();
    for path in paths {
        let filename = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                SfumatoError::render(
                    ErrorClass::Permanent,
                    "Generated artifact filename is invalid",
                )
            })?;
        let reference = format!("{}/{}", reference_prefix.trim_end_matches('/'), filename);
        if document.contains(&reference) {
            retained.push(path);
        } else {
            workspace.remove_file(&path)?;
        }
    }
    Ok(retained)
}

pub(crate) fn referenced_generated_assets(
    paths: &[PathBuf],
    document: &str,
    reference_prefix: &str,
) -> Vec<PathBuf> {
    paths
        .iter()
        .filter(|path| {
            path.file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|filename| {
                    document.contains(&format!(
                        "{}/{}",
                        reference_prefix.trim_end_matches('/'),
                        filename
                    ))
                })
        })
        .cloned()
        .collect()
}

pub(crate) fn stage_referenced_generated_assets(
    paths: &[PathBuf],
    document: &str,
    reference_prefix: &str,
    root: &Path,
    workspace: &dyn WorkspaceFileSystem,
) -> Result<()> {
    for path in referenced_generated_assets(paths, document, reference_prefix) {
        let filename = path.file_name().ok_or_else(|| {
            SfumatoError::render(
                ErrorClass::Permanent,
                "Generated artifact filename is invalid",
            )
        })?;
        workspace.copy_file(&path, &root.join(reference_prefix).join(filename))?;
    }
    Ok(())
}
