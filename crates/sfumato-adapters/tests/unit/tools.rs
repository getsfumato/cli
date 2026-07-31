use super::*;
use async_trait::async_trait;
use sfumato_core::{
    errors::{OperationStage, SfumatoResult},
    operation::OperationContext,
    prompts::{PromptError, PromptOrigin, PromptProvenance, RenderedPrompt},
    providers::{
        ImageGenerationProvider, ImageGenerationResponse, VideoGenerationProvider,
        VideoGenerationRequest, VideoGenerationResponse,
    },
    themes::{THEME_SCHEMA_VERSION, ThemeAdapters, ThemeManifest, ThemePackage, ThemeTokens},
};
use std::{collections::BTreeMap, sync::Mutex};

struct MockImageProvider {
    prompts: Arc<Mutex<Vec<String>>>,
}

struct MockVideoProvider {
    prompts: Arc<Mutex<Vec<String>>>,
}

struct TestPromptCatalog;

impl PromptCatalog for TestPromptCatalog {
    fn render(
        &self,
        request: PromptRenderRequest,
    ) -> std::result::Result<sfumato_core::prompts::RenderedPrompt, PromptError> {
        if request.id == PromptId::ToolsGenerationDescriptions {
            return Ok(RenderedPrompt {
                text: serde_json::json!({
                    "list_directory": "List a directory.",
                    "list_directory_path": "Directory path.",
                    "read_file": "Read a file.",
                    "read_file_path": "File path.",
                    "image_generation": "Generate an image.",
                    "image_prompt": "Image prompt.",
                    "image_alt_text": "Accessible alternative text.",
                    "video_generation": "Generate a video.",
                    "video_prompt": "Video prompt.",
                    "video_accessible_description": "Accessible video description.",
                    "audio_generation": "Speak a line of narration.",
                    "audio_text": "Words to speak.",
                    "audio_voice": "Voice identifier."
                })
                .to_string(),
                provenance: PromptProvenance {
                    id: request.id,
                    origin: PromptOrigin::Bundled,
                    version: 1,
                    content_hash: "test-tools".to_string(),
                },
            });
        }
        let value = |key: &str| {
            request.variables.0[key]
                .as_str()
                .unwrap_or_default()
                .to_string()
        };
        Ok(RenderedPrompt {
            text: format!(
                "{}\nTheme: {}\nSemantic colors: {}\nTypography: {}\n{}",
                value("requested_prompt"),
                value("theme_name"),
                value("theme_colors"),
                value("theme_fonts"),
                value("project_instructions")
            ),
            provenance: PromptProvenance {
                id: request.id,
                origin: PromptOrigin::Bundled,
                version: 1,
                content_hash: "test".to_string(),
            },
        })
    }

    fn validate(&self) -> std::result::Result<Vec<PromptProvenance>, PromptError> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl ImageGenerationProvider for MockImageProvider {
    async fn generate_image(
        &self,
        request: ImageGenerationRequest,
        _operation: &OperationContext,
        _stage: OperationStage,
    ) -> SfumatoResult<ImageGenerationResponse> {
        self.prompts.lock().unwrap().push(request.prompt);
        Ok(ImageGenerationResponse {
            bytes: b"fake-png".to_vec(),
            media_type: "image/png".to_string(),
        })
    }
}

#[async_trait]
impl VideoGenerationProvider for MockVideoProvider {
    async fn generate_video(
        &self,
        request: VideoGenerationRequest,
        _operation: &OperationContext,
        _stage: OperationStage,
    ) -> SfumatoResult<VideoGenerationResponse> {
        self.prompts.lock().unwrap().push(request.prompt);
        Ok(VideoGenerationResponse {
            bytes: b"fake-mp4".to_vec(),
            media_type: "video/mp4".into(),
            provider_job_id: Some("job-1".into()),
            cost: Some(0.01),
        })
    }
}

#[tokio::test]
async fn lists_and_reads_inside_allowed_roots() {
    let temp = tempfile::tempdir().unwrap();
    let note = temp.path().join("note.md");
    fs::write(&note, "# Note").unwrap();
    let executor = FilesystemToolExecutor::new(vec![temp.path().to_path_buf()]).unwrap();

    let listing = executor
        .execute(
            ToolExecutionRequest {
                name: "sfumato_list_directory".to_string(),
                arguments: json!({ "path": temp.path() }),
            },
            &OperationContext::detached(),
            OperationStage::Draft,
        )
        .await
        .unwrap();
    assert!(listing.contains("note.md"));

    let content = executor
        .execute(
            ToolExecutionRequest {
                name: "sfumato_read_file".to_string(),
                arguments: json!({ "path": note }),
            },
            &OperationContext::detached(),
            OperationStage::Draft,
        )
        .await
        .unwrap();
    assert!(content.contains("# Note"));
}

#[tokio::test]
async fn rejects_paths_outside_allowed_roots() {
    let allowed = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let secret = outside.path().join("secret.md");
    fs::write(&secret, "nope").unwrap();
    let executor = FilesystemToolExecutor::new(vec![allowed.path().to_path_buf()]).unwrap();

    assert!(
        executor
            .execute(
                ToolExecutionRequest {
                    name: "sfumato_read_file".to_string(),
                    arguments: json!({ "path": secret }),
                },
                &OperationContext::detached(),
                OperationStage::Draft
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn tool_arguments_accept_json_strings() {
    let temp = tempfile::tempdir().unwrap();
    let note = temp.path().join("note.md");
    fs::write(&note, "# Note").unwrap();
    let executor = FilesystemToolExecutor::new(vec![temp.path().to_path_buf()]).unwrap();

    let content = executor
        .execute(
            ToolExecutionRequest {
                name: "sfumato_read_file".to_string(),
                arguments: Value::String(format!(r#"{{"path":"{}"}}"#, note.display())),
            },
            &OperationContext::detached(),
            OperationStage::Draft,
        )
        .await
        .unwrap();
    assert!(content.contains("# Note"));
}

#[tokio::test]
async fn image_tool_injects_theme_and_tracks_the_artifact() {
    let temp = tempfile::tempdir().unwrap();
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let theme = ThemePackage {
        root: temp.path().to_path_buf(),
        manifest: ThemeManifest {
            schema_version: THEME_SCHEMA_VERSION,
            name: "gruvbox".to_string(),
            description: "Test".to_string(),
            tokens: ThemeTokens {
                colors: BTreeMap::from([
                    ("background".to_string(), "#282828".to_string()),
                    ("accent".to_string(), "#fabd2f".to_string()),
                ]),
                fonts: BTreeMap::from([("body".to_string(), "Inter".to_string())]),
            },
            adapters: ThemeAdapters {
                marp_css: "marp/theme.css".into(),
                html: None,
                document: None,
            },
        },
    };
    let tools = FilesystemGenerationToolFactory
        .create(GenerationToolsRequest {
            project_root: temp.path().to_path_buf(),
            sources: Vec::new(),
            image: Some(ImageToolConfig {
                provider: Arc::new(MockImageProvider {
                    prompts: prompts.clone(),
                }),
                profile_name: "openrouter-image".to_string(),
                output_dir: temp.path().join("slides/images"),
                reference_prefix: "images".to_string(),
                theme,
                project_instructions: Some("Use Spanish labels.".to_string()),
            }),
            video: None,
            audio: None,
            prompt_catalog: Arc::new(TestPromptCatalog),
        })
        .unwrap();

    assert!(
        tools
            .definitions
            .iter()
            .any(|tool| tool.function.name == "sfumato_image_gen")
    );
    let result = tools
        .executor
        .execute(
            ToolExecutionRequest {
                name: "sfumato_image_gen".to_string(),
                arguments: json!({
                    "prompt": "A labeled unit circle",
                    "alt_text": "Unit circle with sine and cosine"
                }),
            },
            &OperationContext::detached(),
            OperationStage::Draft,
        )
        .await
        .unwrap();
    let result: Value = serde_json::from_str(&result).unwrap();

    let markdown_path = result["markdown_path"].as_str().unwrap();
    assert!(markdown_path.starts_with("images/image-"));
    assert!(markdown_path.ends_with(".png"));
    assert_eq!(tools.generated_artifacts().unwrap().len(), 1);
    assert_eq!(tools.generated_prompts().unwrap().len(), 2);
    assert!(tools.generated_artifacts().unwrap()[0].is_file());
    let prompt = prompts.lock().unwrap()[0].clone();
    assert!(prompt.contains("Theme: gruvbox"));
    assert!(prompt.contains("background=#282828"));
    assert!(prompt.contains("A labeled unit circle"));
    assert!(prompt.contains("Use Spanish labels."));
}

#[test]
fn filesystem_only_tools_do_not_declare_image_generation() {
    let temp = tempfile::tempdir().unwrap();
    let tools = FilesystemGenerationToolFactory
        .create(GenerationToolsRequest {
            project_root: temp.path().to_path_buf(),
            sources: Vec::new(),
            image: None,
            video: None,
            audio: None,
            prompt_catalog: Arc::new(TestPromptCatalog),
        })
        .unwrap();

    assert!(
        tools
            .definitions
            .iter()
            .all(|tool| tool.function.name != "sfumato_image_gen")
    );
}

#[tokio::test]
async fn page_video_tool_injects_theme_tracks_mp4_and_allows_one_call() {
    let temp = tempfile::tempdir().unwrap();
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let theme = ThemePackage {
        root: temp.path().to_path_buf(),
        manifest: ThemeManifest {
            schema_version: THEME_SCHEMA_VERSION,
            name: "gruvbox".into(),
            description: "Test".into(),
            tokens: ThemeTokens {
                colors: BTreeMap::from([("accent".into(), "#fabd2f".into())]),
                fonts: BTreeMap::from([("body".into(), "Inter".into())]),
            },
            adapters: ThemeAdapters {
                marp_css: "marp/theme.css".into(),
                html: None,
                document: None,
            },
        },
    };
    let tools = FilesystemGenerationToolFactory
        .create(GenerationToolsRequest {
            project_root: temp.path().to_path_buf(),
            sources: Vec::new(),
            image: None,
            video: Some(VideoToolConfig {
                provider: Arc::new(MockVideoProvider {
                    prompts: prompts.clone(),
                }),
                profile_name: "remote-video".into(),
                output_dir: temp.path().join("pages/assets/videos"),
                reference_prefix: "assets/videos".into(),
                theme,
                project_instructions: Some("Use Spanish labels.".into()),
                references: Vec::new(),
                options: Default::default(),
            }),
            audio: None,
            prompt_catalog: Arc::new(TestPromptCatalog),
        })
        .unwrap();

    assert!(
        tools
            .definitions
            .iter()
            .any(|tool| tool.function.name == "sfumato_video_gen")
    );
    let request = ToolExecutionRequest {
        name: "sfumato_video_gen".into(),
        arguments: json!({
            "prompt": "Animate harmonic synthesis",
            "accessible_description": "Sine waves combine into a square wave"
        }),
    };
    let result = tools
        .executor
        .execute(
            request.clone(),
            &OperationContext::detached(),
            OperationStage::Draft,
        )
        .await
        .unwrap();
    let result: Value = serde_json::from_str(&result).unwrap();

    assert!(
        result["html_path"]
            .as_str()
            .unwrap()
            .starts_with("assets/videos/video-")
    );
    assert_eq!(tools.generated_artifacts().unwrap().len(), 1);
    let prompt = prompts.lock().unwrap()[0].clone();
    assert!(prompt.contains("Theme: gruvbox"));
    assert!(prompt.contains("Animate harmonic synthesis"));
    assert!(prompt.contains("Use Spanish labels."));

    let error = tools
        .executor
        .execute(
            request,
            &OperationContext::detached(),
            OperationStage::Draft,
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("at most once"));
}
