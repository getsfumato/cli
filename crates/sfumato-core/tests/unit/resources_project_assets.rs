use super::*;
use async_trait::async_trait;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use crate::{
    errors::SfumatoError,
    project_assets::{
        AddProjectAssetRequest, ProjectAsset, ProjectAssetCatalog, ProjectAssetMetadata,
        ProjectAssetVariant, UpdateProjectAssetRequest,
    },
    prompts::{
        PromptError, PromptOrigin, PromptProvenance, PromptRenderRequest, PromptValidation,
        RenderedPrompt,
    },
    providers::{ImageGenerationProvider, ImageGenerationResponse},
    themes::{ThemeAdapters, ThemeManifest, ThemeTokens},
};

struct MemoryAssetCatalog(Mutex<ProjectAsset>);

impl ProjectAssetCatalog for MemoryAssetCatalog {
    fn list(&self, _project_root: &Path) -> Result<Vec<ProjectAsset>> {
        Ok(vec![self.0.lock().unwrap().clone()])
    }

    fn load(&self, _project_root: &Path, _name: &str) -> Result<ProjectAsset> {
        Ok(self.0.lock().unwrap().clone())
    }

    fn add(
        &self,
        _project_root: &Path,
        _request: AddProjectAssetRequest<'_>,
    ) -> Result<ProjectAsset> {
        Err(SfumatoError::config("not used by this test"))
    }

    fn add_generated_variant(
        &self,
        _project_root: &Path,
        _name: &str,
        theme: &str,
        media_type: &str,
        bytes: &[u8],
    ) -> Result<ProjectAsset> {
        let mut asset = self.0.lock().unwrap();
        asset.variants.insert(
            theme.into(),
            ProjectAssetVariant {
                theme: theme.into(),
                media_type: media_type.into(),
                filename: "spectrum-ferrari.png".into(),
                path: PathBuf::from("/catalog/spectrum-ferrari.png"),
                content_hash: format!("{}-bytes", bytes.len()),
            },
        );
        Ok(asset.clone())
    }

    fn update(
        &self,
        _project_root: &Path,
        _name: &str,
        _changes: UpdateProjectAssetRequest,
    ) -> Result<ProjectAsset> {
        Err(SfumatoError::config("not used by this test"))
    }

    fn remove(&self, _project_root: &Path, _name: &str) -> Result<ProjectAsset> {
        Err(SfumatoError::config("not used by this test"))
    }
}

struct FixturePrompts;

impl PromptCatalog for FixturePrompts {
    fn render(
        &self,
        request: PromptRenderRequest,
    ) -> std::result::Result<RenderedPrompt, PromptError> {
        let text = match request.id {
            PromptId::ProjectAssetRegenerationUser => {
                "Preserve the odd-harmonic spectrum composition".to_string()
            }
            PromptId::ImageGenerationUser => "Render with the Ferrari visual theme".to_string(),
            _ => unreachable!("unexpected prompt in artifact preparation"),
        };
        Ok(RenderedPrompt {
            text,
            provenance: PromptProvenance {
                id: request.id,
                origin: PromptOrigin::Bundled,
                version: 1,
                content_hash: request.id.as_str().into(),
            },
        })
    }

    fn validate(&self) -> std::result::Result<PromptValidation, PromptError> {
        Ok(PromptValidation::default())
    }
}

#[derive(Default)]
struct FixtureImageProvider(Mutex<Vec<String>>);

#[async_trait]
impl ImageGenerationProvider for FixtureImageProvider {
    async fn generate_image(
        &self,
        request: ImageGenerationRequest,
        _operation: &OperationContext,
        _stage: OperationStage,
    ) -> Result<ImageGenerationResponse> {
        self.0.lock().unwrap().push(request.prompt);
        Ok(ImageGenerationResponse {
            bytes: b"ferrari-spectrum".to_vec(),
            media_type: "image/png".into(),
        })
    }
}

fn ferrari_theme() -> ThemePackage {
    ThemePackage {
        root: PathBuf::from("/themes/ferrari"),
        manifest: ThemeManifest {
            schema_version: 1,
            name: "ferrari".into(),
            description: "Ferrari".into(),
            tokens: ThemeTokens {
                colors: BTreeMap::from([("primary".into(), "#d40000".into())]),
                fonts: BTreeMap::from([("body".into(), "Inter".into())]),
            },
            adapters: ThemeAdapters {
                marp_css: "marp/theme.css".into(),
                html: None,
                document: None,
            },
        },
    }
}

fn prepared(name: &str, reference: &str) -> PreparedProjectAsset {
    PreparedProjectAsset {
        source: PathBuf::from(format!("/catalog/{name}.png")),
        destination: PathBuf::from(format!("/staging/{name}.png")),
        reference: ProjectAssetReference {
            name: name.into(),
            description: format!("{name} description"),
            alt_text: format!("{name} alt"),
            tags: Vec::new(),
            theme: "ferrari".into(),
            media_type: "image/png".into(),
            reference: reference.into(),
            content_hash: "hash".into(),
        },
    }
}

#[test]
fn selects_only_project_artifacts_referenced_by_the_final_document() {
    let assets = PreparedProjectAssets {
        assets: vec![
            prepared("used", "assets/images/used.png"),
            prepared("unused", "assets/images/unused.png"),
        ],
        prompts: Vec::new(),
        warnings: Vec::new(),
    };

    assert_eq!(
        assets.referenced_names("<img src=\"assets/images/used.png\">"),
        vec!["used"]
    );
}

#[tokio::test]
async fn regenerates_and_caches_a_missing_theme_variant_before_drafting() {
    let catalog = MemoryAssetCatalog(Mutex::new(ProjectAsset {
        name: "square-wave-spectrum".into(),
        metadata: ProjectAssetMetadata {
            description: "Odd-harmonic square-wave spectrum".into(),
            alt_text: "Bars at odd harmonics".into(),
            tags: vec!["fourier".into()],
            generation_prompt: Some("Recreate the same logical diagram".into()),
        },
        variants: BTreeMap::from([(
            "gruvbox".into(),
            ProjectAssetVariant {
                theme: "gruvbox".into(),
                media_type: "image/png".into(),
                filename: "spectrum-gruvbox.png".into(),
                path: PathBuf::from("/catalog/spectrum-gruvbox.png"),
                content_hash: "gruvbox-hash".into(),
            },
        )]),
    }));
    let image_provider = Arc::new(FixtureImageProvider::default());
    let provider: Arc<dyn ImageGenerationProvider> = image_provider.clone();

    let prepared = prepare_project_assets(PrepareProjectAssetsRequest {
        catalog: &catalog,
        project_root: Path::new("/project"),
        theme: &ferrari_theme(),
        image_provider: Some(&provider),
        prompt_catalog: &FixturePrompts,
        project_instructions: "Teach visually",
        output_dir: Path::new("/output/images"),
        reference_prefix: "images",
        dry_run: false,
        operation: &OperationContext::detached(),
    })
    .await
    .unwrap();

    assert_eq!(image_provider.0.lock().unwrap().len(), 1);
    assert_eq!(prepared.prompts.len(), 2);
    assert!(prepared.warnings.is_empty());
    assert_eq!(prepared.assets[0].reference.theme, "ferrari");
    assert_eq!(
        prepared.assets[0].reference.reference,
        "images/spectrum-ferrari.png"
    );
    assert!(catalog.0.lock().unwrap().variants.contains_key("ferrari"));
}
