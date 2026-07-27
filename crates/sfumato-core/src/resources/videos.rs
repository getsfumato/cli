//! Standalone video planning, review, rendering, inspection, and publication.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};

use serde::{Deserialize, Deserializer, Serialize};
use sfumato_domain::{
    ArtifactKind, ReviewableDocument, VideoEngine, VideoPlanDocument, VideoScene,
    VideoSourceDocument, VideoWorkflow,
};
use sha2::{Digest, Sha256};
use slug::slugify;

use crate::{
    artifacts::{
        ArtifactResourceKind, ArtifactStore, ResourceArtifactFile, ResourceArtifactManifest,
    },
    config::{Capability, EffectiveConfig, GenerationToolKind, ModelRole, VideoAudioMode},
    errors::{
        ErrorClass, OperationStage, ResultContext as Context, SfumatoError, SfumatoResult as Result,
    },
    filesystem::WorkspaceFileSystem,
    generation::{
        GenerationRequest, GenerationToolSummary, ReviewStatus, VideoGenerationOutput,
        VideoReviewSession, VideoReviewSummary, VideoVisualReviewMode,
    },
    operation::OperationContext,
    project_assets::{ProjectAssetCatalog, ProjectAssetReference},
    prompts::{PromptCatalog, PromptId, PromptProvenance, PromptRenderRequest, PromptVariables},
    providers::{
        GenerationStage, ImageGenerationProvider, ProviderFactory, TextGenerationEvent,
        TextGenerationRequest, ToolDefinition, VideoGenerationRequest,
    },
    renderers::{VideoInspection, VideoRenderRequest, VideoRenderer},
    repositories::ThemeRepository,
    sources::{SourceDocument, SourceReader},
    themes::ThemePackage,
    tools::{GenerationToolFactory, GenerationToolsRequest, ImageToolConfig},
};

use super::{
    DryRunImageProvider,
    project_assets::{
        PrepareProjectAssetsRequest, prepare_project_assets, retain_referenced_generated_assets,
    },
};

/// Engine-neutral command settings for one standalone video.
#[derive(Clone, Debug)]
pub struct GenerateVideoRequest {
    /// Selected engine; no automatic fallback is performed.
    pub engine: VideoEngine,
    /// Optional explicit title.
    pub title: Option<String>,
    /// Required duration in seconds.
    pub duration_seconds: u32,
    /// Resolution name such as `1080p`.
    pub resolution: String,
    /// Aspect ratio such as `16:9`.
    pub aspect_ratio: String,
    /// Frames per second for local renderers.
    pub fps: u32,
    /// Local renderer quality.
    pub quality: String,
    /// Native audio policy for direct models.
    pub audio: VideoAudioMode,
    /// One-time generated Python execution approval.
    pub allow_code_execution: bool,
    /// Requested Hyperframe production workflow.
    pub workflow: VideoWorkflow,
    /// Explicit websites to capture before planning. Hyperframe-only.
    pub urls: Vec<String>,
    /// Stop after validation and visual evidence instead of rendering an MP4.
    pub visual_review: bool,
}

/// Complete video-generation result returned to presentation frontends.
#[derive(Debug)]
pub struct GenerateVideoResult {
    /// Committed or planned MP4 path.
    pub video_path: PathBuf,
    /// Published MP4 paths.
    pub published_paths: Vec<PathBuf>,
    /// Machine-readable operation output.
    pub output: VideoGenerationOutput,
    /// Rendered planning prompt during dry-run.
    pub prompt_preview: Option<String>,
}

pub(crate) struct GenerateVideoOptions {
    pub operation: OperationContext,
    pub dry_run: bool,
    pub review: bool,
    pub event_sink: Option<Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
    pub prompt_catalog: Arc<dyn PromptCatalog>,
    pub artifact_store: Arc<dyn ArtifactStore>,
    pub provider_factory: Arc<dyn ProviderFactory>,
    pub source_reader: Arc<dyn SourceReader>,
    pub tool_factory: Arc<dyn GenerationToolFactory>,
    pub theme_repository: Arc<dyn ThemeRepository>,
    pub video_renderer: Arc<dyn VideoRenderer>,
    pub workspace: Arc<dyn WorkspaceFileSystem>,
    pub project_asset_catalog: Arc<dyn ProjectAssetCatalog>,
}

/// Inputs required to render one previously paused Hyperframe review session.
pub struct ApproveVideoReviewOptions {
    pub operation: OperationContext,
    pub artifact_store: Arc<dyn ArtifactStore>,
    pub video_renderer: Arc<dyn VideoRenderer>,
    pub workspace: Arc<dyn WorkspaceFileSystem>,
    pub publish_root_override: bool,
}

#[derive(Deserialize, Serialize)]
struct ReviewSessionRecord {
    schema_version: u32,
    review_id: String,
    project: String,
    engine: VideoEngine,
    status: String,
    resolution: String,
    aspect_ratio: String,
    fps: u32,
    quality: String,
    source_hash: String,
    plan_hash: String,
    #[serde(default)]
    publish_root: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, Serialize)]
struct VideoPromptContext {
    learning_style: String,
    project: String,
    project_root: String,
    theme_name: String,
    theme_colors: String,
    theme_fonts: String,
    instruction: String,
    project_instructions: String,
    source_bundle: String,
    engine: String,
    duration_seconds: u32,
    resolution: String,
    aspect_ratio: String,
    width: u32,
    height: u32,
    fps: u32,
    title: String,
    title_provided: bool,
    reusable_assets: Vec<ProjectAssetReference>,
    image_generation_available: bool,
    plan_snapshot: String,
    source_snapshot: String,
    validation_error: String,
    max_tool_rounds: usize,
    workflow: String,
    urls: Vec<String>,
    catalog: String,
}

struct VideoContextInput<'a> {
    config: &'a EffectiveConfig,
    theme: &'a ThemePackage,
    request: &'a GenerationRequest,
    video: &'a GenerateVideoRequest,
    source_bundle: &'a str,
    project_instructions: &'a str,
    reusable_assets: Vec<ProjectAssetReference>,
    image_generation_available: bool,
    max_tool_rounds: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VideoPlanDraft {
    #[serde(deserialize_with = "deserialize_draft_text")]
    title: String,
    #[serde(deserialize_with = "deserialize_draft_text")]
    objective: String,
    scenes: Vec<VideoScene>,
    #[serde(default)]
    artifacts: Vec<String>,
    #[serde(deserialize_with = "deserialize_draft_text")]
    visual_direction: String,
    #[serde(default, deserialize_with = "deserialize_draft_text")]
    remote_prompt: String,
    #[serde(default)]
    workflow: VideoWorkflow,
    #[serde(default, deserialize_with = "deserialize_draft_text")]
    message: String,
    #[serde(default, deserialize_with = "deserialize_draft_text")]
    narrative_arc: String,
    #[serde(default, deserialize_with = "deserialize_draft_text")]
    design_direction: String,
}

fn deserialize_draft_text<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::String(value) => value,
        value => value.to_string(),
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VideoSourceDraft {
    files: BTreeMap<String, String>,
}

/// Renders the immutable source bundle from a previously approved review session.
pub async fn approve_video_review(
    config: EffectiveConfig,
    review_id: &str,
    options: ApproveVideoReviewOptions,
) -> Result<GenerateVideoResult> {
    if review_id.is_empty() || review_id.contains(['/', '\\']) || review_id.contains("..") {
        return Err(SfumatoError::validation("Invalid video review identifier"));
    }
    let ApproveVideoReviewOptions {
        operation,
        artifact_store,
        video_renderer,
        workspace,
        publish_root_override,
    } = options;
    let review_root = artifact_store
        .project_root(&config.project_name)?
        .join("review-sessions")
        .join(review_id);
    let record: ReviewSessionRecord = serde_json::from_str(
        &workspace.read_text(&review_root.join("review.json"))?,
    )
    .map_err(|error| SfumatoError::validation(format!("Invalid review session: {error}")))?;
    if record.schema_version != 1
        || record.review_id != review_id
        || record.project != config.project_name
        || record.engine != VideoEngine::Hyperframe
        || record.status != "pending_approval"
    {
        return Err(SfumatoError::validation(
            "Review session is not an approvable Hyperframe session",
        ));
    }
    let source_text = workspace.read_text(&review_root.join("source.json"))?;
    if hash_text(&source_text) != record.source_hash {
        return Err(SfumatoError::validation(
            "Review source hash no longer matches its immutable session",
        ));
    }
    let source: VideoSourceDocument = serde_json::from_str(&source_text)
        .map_err(|error| SfumatoError::validation(format!("Review source is invalid: {error}")))?;
    validate_source(&source)?;
    let plan_text = workspace.read_text(&review_root.join("plan.json"))?;
    if hash_text(&plan_text) != record.plan_hash {
        return Err(SfumatoError::validation(
            "Review plan hash no longer matches its immutable session",
        ));
    }
    let plan: VideoPlanDocument = serde_json::from_str(&plan_text)
        .map_err(|error| SfumatoError::validation(format!("Review plan is invalid: {error}")))?;
    let configured_publish_root = config.publish_root()?;
    let publish_root = review_publish_root(
        record.publish_root.clone(),
        configured_publish_root,
        publish_root_override,
    );
    let transaction = artifact_store.begin(&config.project_name, ArtifactResourceKind::Videos)?;
    let staging_root = transaction.staging_root().to_path_buf();
    let source_root = staging_root.join("source");
    workspace.copy_tree(&review_root.join("source"), &source_root, &[])?;
    workspace.write(
        &source_root.join("source.json"),
        source.render().map_err(review_error)?.as_bytes(),
    )?;
    let slug = slugify(plan.title());
    let video = GenerateVideoRequest {
        engine: VideoEngine::Hyperframe,
        title: Some(plan.title().into()),
        duration_seconds: plan.duration_seconds(),
        resolution: record.resolution.clone(),
        aspect_ratio: record.aspect_ratio.clone(),
        fps: record.fps,
        quality: record.quality.clone(),
        audio: VideoAudioMode::Off,
        allow_code_execution: false,
        workflow: plan.workflow(),
        urls: Vec::new(),
        visual_review: false,
    };
    let output_path = staging_root.join(format!("{slug}.mp4"));
    let request = local_render_request(&source_root, &output_path, &video)?;
    video_renderer
        .validate(VideoEngine::Hyperframe, &request, &operation)
        .await?;
    video_renderer
        .render(VideoEngine::Hyperframe, &request, &operation)
        .await?;
    let inspection = video_renderer.inspect(&output_path, &operation).await?;
    validate_inspection(&inspection, &video, Some(false))?;
    let snapshots = review_root.join("snapshots");
    if snapshots.is_dir() {
        workspace.copy_tree(&snapshots, &staging_root.join("snapshots"), &[])?;
    }
    for name in [
        "plan.json",
        "STORYBOARD.md",
        "DESIGN.md",
        "SCRIPT.md",
        "contact-sheet.md",
    ] {
        let source_path = review_root.join(name);
        if workspace.is_file(&source_path) {
            workspace.copy_file(&source_path, &staging_root.join(name))?;
        }
    }
    let files = workspace
        .list_files(&staging_root, &["manifest.json"])?
        .iter()
        .map(|path| artifact_file(&staging_root, path))
        .collect::<Result<Vec<_>>>()?;
    let mut warnings = vec![format!("Rendered approved review session {review_id}")];
    let manifest = ResourceArtifactManifest {
        schema_version: 1,
        job_id: transaction.job_id().clone(),
        revision_id: transaction.revision_id().clone(),
        parent_revision: transaction.parent_revision().cloned(),
        project: config.project_name.clone(),
        resource_kind: ArtifactResourceKind::Videos,
        resource_id: slug.clone(),
        title: plan.title().into(),
        files,
        models: BTreeMap::new(),
        prompts: Vec::new(),
        plugins: Vec::new(),
        runtimes: Vec::new(),
        warnings: warnings.clone(),
    };
    let committed = transaction.commit(manifest)?;
    let video_path = committed.root.join(format!("{slug}.mp4"));
    let published_paths = publish_video(
        workspace.as_ref(),
        publish_root.as_deref(),
        &video_path,
        &slug,
        &mut warnings,
    )?;
    workspace.write(
        &review_root.join("review.json"),
        serde_json::to_vec_pretty(&ReviewSessionRecord {
            status: "approved".into(),
            ..record
        })?
        .as_slice(),
    )?;
    let artifacts = workspace.list_files(&committed.root, &[])?;
    Ok(GenerateVideoResult {
        video_path: video_path.clone(),
        published_paths: published_paths.clone(),
        prompt_preview: None,
        output: VideoGenerationOutput {
            project: config.project_name,
            title: plan.title().into(),
            engine: VideoEngine::Hyperframe,
            video_path,
            models: BTreeMap::new(),
            tools: Vec::new(),
            project_assets: Vec::new(),
            artifacts,
            published_artifacts: published_paths,
            review: VideoReviewSummary {
                enabled: false,
                semantic_review: ReviewStatus::Skipped,
                source_repair: ReviewStatus::NotNeeded,
                visual_review: ReviewStatus::Completed,
                media_inspection: ReviewStatus::Completed,
            },
            visual_review_mode: VideoVisualReviewMode::HumanApprovalRequired,
            review_session: None,
            visual_report: None,
            prompts: Vec::new(),
            warnings,
        },
    })
}

fn review_publish_root(
    saved: Option<PathBuf>,
    configured: Option<PathBuf>,
    explicit_override: bool,
) -> Option<PathBuf> {
    if explicit_override {
        configured
    } else {
        saved.or(configured)
    }
}

fn hyperframe_catalog_summary() -> String {
    "managed catalog v1: data-chart, code-snippet-dark-modern, x-post, lower-third-clean-bar, us-map, caption-highlight, transitions-cover, grain-overlay, vignette, shimmer-sweep".into()
}

fn hash_text(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

/// Executes one engine-explicit video generation workflow.
pub(crate) async fn generate_video(
    config: EffectiveConfig,
    request: GenerationRequest,
    video: GenerateVideoRequest,
    options: GenerateVideoOptions,
) -> Result<GenerateVideoResult> {
    let GenerateVideoOptions {
        operation,
        dry_run,
        review,
        event_sink,
        prompt_catalog,
        artifact_store,
        provider_factory,
        source_reader,
        tool_factory,
        theme_repository,
        video_renderer,
        workspace,
        project_asset_catalog,
    } = options;
    operation.checkpoint(OperationStage::Resolve)?;
    validate_video_options(&config, &video)?;
    let publish_root = config.publish_root()?;
    let theme = theme_repository.load(&config.theme)?;
    let mut transaction = if dry_run {
        None
    } else {
        Some(artifact_store.begin(&config.project_name, ArtifactResourceKind::Videos)?)
    };
    let staging_root = transaction
        .as_ref()
        .map(|value| value.staging_root().to_path_buf())
        .unwrap_or_else(|| {
            artifact_store
                .project_root(&config.project_name)
                .unwrap_or_else(|_| PathBuf::from(".sfumato"))
                .join("resources/videos/dry-run")
        });
    let source_root = staging_root.join("source");
    let asset_root = if video.engine == VideoEngine::Model {
        staging_root.join("assets")
    } else {
        source_root.join("assets")
    };
    let image_dir = asset_root.join("images");

    let project_instructions = source_reader.project_instructions(&config.project_root)?;
    let documents = source_reader.collect(&request.sources)?;
    let source_bundle = build_source_bundle(&documents, 48_000);
    let image_tool_enabled = config.generation_tool_enabled(GenerationToolKind::ImageGen);
    let image_selection = image_tool_enabled
        .then(|| config.resolve_model(Capability::Image))
        .transpose()?;
    let image_provider = image_selection
        .map(|(_, profile)| {
            if dry_run {
                Ok::<Arc<dyn ImageGenerationProvider>, SfumatoError>(Arc::new(DryRunImageProvider))
            } else {
                provider_factory.image(&config, profile).map(Arc::from)
            }
        })
        .transpose()?;
    let prepared_assets = prepare_project_assets(PrepareProjectAssetsRequest {
        catalog: project_asset_catalog.as_ref(),
        project_root: &config.project_root,
        theme: &theme,
        image_provider: image_provider.as_ref(),
        prompt_catalog: prompt_catalog.as_ref(),
        project_instructions: project_instructions
            .as_ref()
            .map(|value| value.content.as_str())
            .unwrap_or_default(),
        output_dir: &asset_root,
        reference_prefix: "assets",
        dry_run,
        operation: &operation,
    })
    .await?;
    let reusable_assets = prepared_assets.references();
    let image = image_selection.map(|(name, _)| ImageToolConfig {
        provider: image_provider
            .as_ref()
            .expect("image provider resolved")
            .clone(),
        profile_name: name.to_string(),
        output_dir: image_dir.clone(),
        reference_prefix: "assets/images".into(),
        theme: theme.clone(),
        project_instructions: project_instructions
            .as_ref()
            .map(|value| value.content.clone()),
    });
    let tool_set = tool_factory.create(GenerationToolsRequest {
        project_root: config.project_root.clone(),
        sources: request.sources.clone(),
        image,
        video: None,
        prompt_catalog: prompt_catalog.clone(),
    })?;
    let tool_summaries = summarize_tools(&tool_set.definitions);
    let review_tools = tool_set
        .definitions
        .iter()
        .filter(|tool| tool.function.name != "sfumato_image_gen")
        .cloned()
        .collect::<Vec<_>>();

    let (draft_name, draft_profile) = config.resolve_model(Capability::Text)?;
    let reviewer = review
        .then(|| config.resolve_model_role(ModelRole::Reviewer))
        .transpose()?;
    let mut models = BTreeMap::from([("text".into(), draft_name.to_string())]);
    if let Some((name, _)) = reviewer {
        models.insert("reviewer".into(), name.to_string());
    }
    if let Some((name, _)) = image_selection {
        models.insert("image".into(), name.to_string());
    }
    let engine_profile = match video.engine {
        VideoEngine::Model => {
            let (name, profile) = config.resolve_model(Capability::Video)?;
            models.insert("video".into(), name.into());
            Some((name, profile))
        }
        VideoEngine::Hyperframe | VideoEngine::Manim => {
            let selected = config.resolve_model(Capability::Code).or_else(|_| {
                if draft_profile.capabilities.contains(&Capability::Code) {
                    Ok((draft_name, draft_profile))
                } else {
                    Err(SfumatoError::config(
                        "Local video engines require a code-capable model profile",
                    ))
                }
            })?;
            models.insert("code".into(), selected.0.into());
            Some(selected)
        }
    };
    let mut context = video_context(VideoContextInput {
        config: &config,
        theme: &theme,
        request: &request,
        video: &video,
        source_bundle: &source_bundle,
        project_instructions: project_instructions
            .as_ref()
            .map(|value| value.content.as_str())
            .unwrap_or_default(),
        reusable_assets: reusable_assets.clone(),
        image_generation_available: image_selection.is_some(),
        max_tool_rounds: draft_profile.options.tool_rounds(),
    })?;
    let mut plan_request = render_request(
        prompt_catalog.as_ref(),
        PromptId::VideoPlanSystem,
        PromptId::VideoPlanUser,
        &context,
    )?;
    plan_request.tools = tool_set.definitions.clone();
    plan_request.tool_executor = Some(tool_set.executor.clone());
    plan_request.event_sink = event_sink.clone();
    let mut prompts = prepared_assets.prompts.clone();
    prompts.extend(plan_request.prompt_provenance.clone());
    let mut review_summary = VideoReviewSummary::new(review);
    let planned_path = staging_root.join("video.mp4");
    if dry_run {
        return Ok(GenerateVideoResult {
            video_path: planned_path.clone(),
            published_paths: Vec::new(),
            output: VideoGenerationOutput {
                project: config.project_name,
                title: video
                    .title
                    .unwrap_or_else(|| "model-generated-title".into()),
                engine: video.engine,
                video_path: planned_path,
                models,
                tools: tool_summaries,
                project_assets: reusable_assets,
                artifacts: Vec::new(),
                published_artifacts: Vec::new(),
                review: review_summary,
                visual_review_mode: VideoVisualReviewMode::Disabled,
                review_session: None,
                visual_report: None,
                prompts,
                warnings: prepared_assets.warnings,
            },
            prompt_preview: Some(plan_request.user_prompt),
        });
    }

    emit_stage(
        &event_sink,
        GenerationStage::VideoPlanning,
        Some(draft_name),
    );
    let planner = provider_factory.text(&config, draft_profile)?;
    let response = planner
        .generate_text(plan_request, &operation, OperationStage::Draft)
        .await?;
    prompts.extend(tool_set.generated_prompts()?);
    let generated_assets = tool_set.generated_artifacts()?;
    let mut plan = parse_plan(
        &response.text,
        video.engine,
        video.duration_seconds,
        video.title.as_deref(),
        video.workflow,
    )?;
    let mut warnings = prepared_assets.warnings.clone();
    if let Some((reviewer_name, reviewer_profile)) = reviewer {
        emit_stage(
            &event_sink,
            GenerationStage::VideoReview,
            Some(reviewer_name),
        );
        context.plan_snapshot =
            serde_json::to_string_pretty(&plan.snapshot().map_err(review_error)?)?;
        let mut review_request = render_request(
            prompt_catalog.as_ref(),
            PromptId::VideoReviewSystem,
            PromptId::VideoReviewUser,
            &context,
        )?;
        review_request.tools = review_tools;
        review_request.tool_executor = Some(tool_set.executor.clone());
        review_request.event_sink = event_sink.clone();
        prompts.extend(review_request.prompt_provenance.clone());
        let reviewer_provider = provider_factory.text(&config, reviewer_profile)?;
        let mut candidate = plan.clone();
        match reviewer_provider
            .generate_text(review_request, &operation, OperationStage::Review)
            .await
            .and_then(|response| {
                crate::review::parse_json_patch(&response.text).map_err(review_error)
            })
            .and_then(|patch| candidate.apply_patch(&patch).map_err(review_error))
        {
            Ok(_) => {
                plan = candidate;
                review_summary.semantic_review = ReviewStatus::Completed;
            }
            Err(error) => {
                review_summary.semantic_review = ReviewStatus::Failed;
                warnings.push(format!(
                    "Video semantic review failed; using validated plan: {error}"
                ));
            }
        }
    }
    let plan_text = plan.render().map_err(review_error)?;
    let (project_asset_paths, used_project_assets) =
        prepared_assets.materialize_referenced(&plan_text, workspace.as_ref())?;
    let used_generated_assets = retain_referenced_generated_assets(
        generated_assets,
        &plan_text,
        "assets/images",
        workspace.as_ref(),
    )?;
    let title = plan.title().to_string();
    let slug = slugify(&title);
    if slug.is_empty() {
        return Err(SfumatoError::validation(
            "Video title cannot produce an artifact name",
        ));
    }
    let video_path = staging_root.join(format!("{slug}.mp4"));
    workspace.write(&staging_root.join("plan.json"), plan_text.as_bytes())?;
    workspace.write(
        &staging_root.join("STORYBOARD.md"),
        storyboard(&plan).as_bytes(),
    )?;
    workspace.write(
        &staging_root.join("DESIGN.md"),
        design_document(&plan, &theme).as_bytes(),
    )?;
    workspace.write(
        &staging_root.join("SCRIPT.md"),
        script_document(&plan).as_bytes(),
    )?;

    let expected_audio = match video.audio {
        VideoAudioMode::Auto => None,
        VideoAudioMode::On => Some(true),
        VideoAudioMode::Off => Some(false),
    };
    match video.engine {
        VideoEngine::Model => {
            emit_stage(
                &event_sink,
                GenerationStage::VideoRendering,
                engine_profile.map(|value| value.0),
            );
            let (_, profile) = engine_profile.expect("model profile resolved");
            let provider = provider_factory.video(&config, profile)?;
            let mut references = project_asset_paths;
            references.extend(used_generated_assets);
            let generated = provider
                .generate_video(
                    VideoGenerationRequest {
                        prompt: plan.remote_prompt().to_string(),
                        duration_seconds: video.duration_seconds,
                        resolution: video.resolution.clone(),
                        aspect_ratio: video.aspect_ratio.clone(),
                        generate_audio: expected_audio,
                        seed: profile.options.video.seed,
                        references,
                    },
                    &operation,
                    OperationStage::Render,
                )
                .await?;
            if generated.media_type != "video/mp4"
                && generated.media_type != "application/octet-stream"
            {
                return Err(SfumatoError::provider(
                    ErrorClass::InvalidOutput,
                    format!(
                        "Video provider returned unsupported media type '{}'",
                        generated.media_type
                    ),
                ));
            }
            workspace.write(&video_path, &generated.bytes)?;
        }
        VideoEngine::Hyperframe | VideoEngine::Manim => {
            emit_stage(
                &event_sink,
                GenerationStage::VideoAuthoring,
                engine_profile.map(|value| value.0),
            );
            let (_, profile) = engine_profile.expect("code profile resolved");
            context.plan_snapshot =
                serde_json::to_string_pretty(&plan.snapshot().map_err(review_error)?)?;
            let ids = if video.engine == VideoEngine::Hyperframe {
                (
                    PromptId::VideoHyperframeSystem,
                    PromptId::VideoHyperframeUser,
                )
            } else {
                (PromptId::VideoManimSystem, PromptId::VideoManimUser)
            };
            let mut author_request =
                render_request(prompt_catalog.as_ref(), ids.0, ids.1, &context)?;
            author_request.event_sink = event_sink.clone();
            prompts.extend(author_request.prompt_provenance.clone());
            let author = provider_factory.text(&config, profile)?;
            let response = author
                .generate_text(author_request, &operation, OperationStage::Draft)
                .await?;
            let mut source = parse_source(&response.text, video.engine)?;
            let local_request = local_render_request(&source_root, &video_path, &video)?;
            if let Err(error) = validate_write_source(
                &source,
                &source_root,
                workspace.as_ref(),
                video_renderer.as_ref(),
                video.engine,
                &local_request,
                &operation,
            )
            .await
            {
                if !review {
                    return Err(error);
                }
                review_summary.source_repair = ReviewStatus::Pending;
                let (reviewer_name, reviewer_profile) = reviewer
                    .or(engine_profile)
                    .expect("local video engine has an author profile");
                emit_stage(
                    &event_sink,
                    GenerationStage::VideoRepair,
                    Some(reviewer_name),
                );
                context.source_snapshot =
                    serde_json::to_string_pretty(&source.snapshot().map_err(review_error)?)?;
                context.validation_error = error.to_string();
                let mut repair_request = render_request(
                    prompt_catalog.as_ref(),
                    PromptId::VideoSourceRepairSystem,
                    PromptId::VideoSourceRepairUser,
                    &context,
                )?;
                repair_request.event_sink = event_sink.clone();
                prompts.extend(repair_request.prompt_provenance.clone());
                let repairer = provider_factory.text(&config, reviewer_profile)?;
                let response = repairer
                    .generate_text(repair_request, &operation, OperationStage::Repair)
                    .await?;
                let patch =
                    crate::review::parse_json_patch(&response.text).map_err(review_error)?;
                source.apply_patch(&patch).map_err(review_error)?;
                source = normalize_hyperframe_parent_paths(source)?;
                validate_write_source(
                    &source,
                    &source_root,
                    workspace.as_ref(),
                    video_renderer.as_ref(),
                    video.engine,
                    &local_request,
                    &operation,
                )
                .await?;
                review_summary.source_repair = ReviewStatus::Completed;
            }
            let snapshots = video_renderer
                .snapshot(
                    video.engine,
                    &local_request,
                    &snapshot_times(&plan),
                    &staging_root.join("snapshots"),
                    &operation,
                )
                .await?;
            workspace.write(
                &staging_root.join("source.json"),
                source.render().map_err(review_error)?.as_bytes(),
            )?;
            workspace.write(
                &staging_root.join("contact-sheet.md"),
                contact_sheet(&plan, &snapshots).as_bytes(),
            )?;
            review_summary.visual_review = ReviewStatus::Completed;
            if video.visual_review {
                return pause_for_visual_review(
                    &config,
                    artifact_store.as_ref(),
                    workspace.as_ref(),
                    &staging_root,
                    &slug,
                    &video,
                    title,
                    video.engine,
                    models,
                    tool_summaries,
                    used_project_assets,
                    review_summary,
                    prompts,
                    warnings,
                    publish_root.as_deref(),
                );
            }
            video_renderer
                .render(video.engine, &local_request, &operation)
                .await?;
        }
    }
    let inspection = video_renderer.inspect(&video_path, &operation).await?;
    validate_inspection(&inspection, &video, expected_audio)?;
    review_summary.media_inspection = ReviewStatus::Completed;
    deduplicate_prompts(&mut prompts);
    let staged_files = workspace.list_files(&staging_root, &["manifest.json"])?;
    let files = staged_files
        .iter()
        .map(|path| artifact_file(&staging_root, path))
        .collect::<Result<Vec<_>>>()?;
    let transaction = transaction
        .take()
        .context("Video artifact transaction is unavailable")?;
    let manifest = ResourceArtifactManifest {
        schema_version: 1,
        job_id: transaction.job_id().clone(),
        revision_id: transaction.revision_id().clone(),
        parent_revision: transaction.parent_revision().cloned(),
        project: config.project_name.clone(),
        resource_kind: ArtifactResourceKind::Videos,
        resource_id: slug.clone(),
        title: title.clone(),
        files,
        models: models.clone(),
        prompts: prompts.clone(),
        plugins: Vec::new(),
        runtimes: Vec::new(),
        warnings: warnings.clone(),
    };
    let staging = transaction.staging_root().to_path_buf();
    let committed = transaction.commit(manifest)?;
    let committed_video = committed.root.join(
        video_path
            .strip_prefix(&staging)
            .context("Video escaped its transaction")?,
    );
    let mut artifacts = workspace.list_files(&committed.root, &[])?;
    artifacts.sort();
    let published_paths = publish_video(
        workspace.as_ref(),
        publish_root.as_deref(),
        &committed_video,
        &slug,
        &mut warnings,
    )?;
    Ok(GenerateVideoResult {
        video_path: committed_video.clone(),
        published_paths: published_paths.clone(),
        prompt_preview: None,
        output: VideoGenerationOutput {
            project: config.project_name,
            title,
            engine: video.engine,
            video_path: committed_video,
            models,
            tools: tool_summaries,
            project_assets: used_project_assets,
            artifacts,
            published_artifacts: published_paths,
            review: review_summary,
            visual_review_mode: if video.engine == VideoEngine::Hyperframe {
                VideoVisualReviewMode::EvidenceOnly
            } else {
                VideoVisualReviewMode::Disabled
            },
            review_session: None,
            visual_report: None,
            prompts,
            warnings,
        },
    })
}

fn video_context(input: VideoContextInput<'_>) -> Result<VideoPromptContext> {
    let VideoContextInput {
        config,
        theme,
        request,
        video,
        source_bundle,
        project_instructions,
        reusable_assets,
        image_generation_available,
        max_tool_rounds,
    } = input;
    let (width, height) = resolution_dimensions(&video.resolution, &video.aspect_ratio)?;
    Ok(VideoPromptContext {
        learning_style: config.user.learning_style.join(", "),
        project: config.project_name.clone(),
        project_root: config.project_root.display().to_string(),
        theme_name: theme.manifest.name.clone(),
        theme_colors: format_tokens(&theme.manifest.tokens.colors),
        theme_fonts: format_tokens(&theme.manifest.tokens.fonts),
        instruction: request.instruction.clone(),
        project_instructions: project_instructions.into(),
        source_bundle: source_bundle.into(),
        engine: engine_name(video.engine).into(),
        duration_seconds: video.duration_seconds,
        resolution: video.resolution.clone(),
        aspect_ratio: video.aspect_ratio.clone(),
        width,
        height,
        fps: video.fps,
        title: video.title.clone().unwrap_or_default(),
        title_provided: video.title.is_some(),
        reusable_assets,
        image_generation_available,
        max_tool_rounds,
        workflow: video.workflow.as_str().into(),
        urls: video.urls.clone(),
        catalog: hyperframe_catalog_summary(),
        ..Default::default()
    })
}

fn render_request(
    catalog: &dyn PromptCatalog,
    system: PromptId,
    user: PromptId,
    context: &VideoPromptContext,
) -> Result<TextGenerationRequest> {
    let variables = PromptVariables::from_serializable(context)?;
    let system = catalog.render(PromptRenderRequest {
        id: system,
        variables: variables.clone(),
    })?;
    let user = catalog.render(PromptRenderRequest {
        id: user,
        variables,
    })?;
    let exhausted = catalog.render(PromptRenderRequest {
        id: PromptId::VideoToolExhaustedUser,
        variables: PromptVariables::from_serializable(
            &serde_json::json!({"max_tool_rounds": context.max_tool_rounds}),
        )?,
    })?;
    let mut request = TextGenerationRequest::new(system.text, user.text);
    request.max_tool_rounds = context.max_tool_rounds;
    request.tool_exhausted_prompt = Some(exhausted.text);
    request.prompt_provenance = vec![system.provenance, user.provenance, exhausted.provenance];
    Ok(request)
}

fn parse_plan(
    response: &str,
    engine: VideoEngine,
    duration: u32,
    title: Option<&str>,
    requested_workflow: VideoWorkflow,
) -> Result<VideoPlanDocument> {
    let draft: VideoPlanDraft =
        serde_json::from_str(strip_json_fence(response)).map_err(|error| {
            SfumatoError::provider(
                ErrorClass::InvalidOutput,
                format!("Video plan must be strict JSON: {error}"),
            )
        })?;
    let design_direction = if draft.design_direction.trim().is_empty() {
        draft.visual_direction.clone()
    } else {
        draft.design_direction
    };
    let message = if draft.message.trim().is_empty() {
        draft.objective.clone()
    } else {
        draft.message
    };
    let narrative_arc = if draft.narrative_arc.trim().is_empty() {
        "hook, explanation, payoff".into()
    } else {
        draft.narrative_arc
    };
    VideoPlanDocument::new_with_pipeline(
        engine,
        title.unwrap_or(&draft.title),
        draft.objective,
        duration,
        draft.scenes,
        draft.artifacts,
        draft.visual_direction,
        draft.remote_prompt,
        if requested_workflow == VideoWorkflow::Auto {
            draft.workflow
        } else {
            requested_workflow
        },
        message,
        narrative_arc,
        design_direction,
    )
    .map_err(review_error)
}

fn parse_source(response: &str, engine: VideoEngine) -> Result<VideoSourceDocument> {
    let draft: VideoSourceDraft =
        serde_json::from_str(strip_json_fence(response)).map_err(|error| {
            SfumatoError::provider(
                ErrorClass::InvalidOutput,
                format!("Video source must be strict JSON: {error}"),
            )
        })?;
    let source = VideoSourceDocument::new(engine, draft.files).map_err(review_error)?;
    normalize_hyperframe_parent_paths(source)
}

fn validate_source(source: &VideoSourceDocument) -> Result<()> {
    let combined = source
        .files()
        .values()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n")
        .replace("http://www.w3.org/2000/svg", "")
        .replace("http://www.w3.org/1999/xlink", "")
        .replace("http://www.w3.org/1999/xhtml", "")
        .to_ascii_lowercase();
    for forbidden in [
        "http://",
        "https://",
        "file:",
        "../",
        "fetch(",
        "xmlhttprequest",
        "websocket(",
        "@import",
        "src=\"/",
        "href=\"/",
    ] {
        if combined.contains(forbidden) {
            return Err(SfumatoError::render(
                ErrorClass::InvalidOutput,
                format!("Video source contains forbidden operation '{forbidden}'"),
            ));
        }
    }
    match source.engine() {
        VideoEngine::Hyperframe => {
            for required in ["meta.json", "index.html"] {
                if !source.files().contains_key(required) {
                    return Err(SfumatoError::render(
                        ErrorClass::InvalidOutput,
                        format!("Hyperframe source is missing {required}"),
                    ));
                }
            }
            let html = source
                .files()
                .get("index.html")
                .expect("required file checked");
            for wrapper in ["<!doctype html", "<html", "<body"] {
                if !html.to_ascii_lowercase().contains(wrapper) {
                    return Err(SfumatoError::render(
                        ErrorClass::InvalidOutput,
                        format!("Hyperframe index.html is missing required wrapper '{wrapper}'"),
                    ));
                }
            }
            for attribute in ["data-composition-id", "data-width", "data-height"] {
                if !html.contains(attribute) {
                    return Err(SfumatoError::render(
                        ErrorClass::InvalidOutput,
                        format!("Hyperframe index.html is missing {attribute}"),
                    ));
                }
            }
            for contract in [
                "./vendor/gsap.min.js",
                "gsap.timeline",
                "window.__timelines",
            ] {
                if !html.contains(contract) {
                    return Err(SfumatoError::render(
                        ErrorClass::InvalidOutput,
                        format!("Hyperframe index.html is missing required contract '{contract}'"),
                    ));
                }
            }
            let compact_html = html
                .chars()
                .filter(|value| !value.is_whitespace())
                .collect::<String>();
            if !compact_html.contains("paused:true") {
                return Err(SfumatoError::render(
                    ErrorClass::InvalidOutput,
                    "Hyperframe index.html is missing required contract 'paused: true'",
                ));
            }
            let compositions = source
                .files()
                .keys()
                .filter(|path| path.starts_with("compositions/") && path.ends_with(".html"))
                .count();
            if compositions == 0 || !html.contains("compositions/") {
                return Err(SfumatoError::render(
                    ErrorClass::InvalidOutput,
                    "Hyperframe source must assemble at least one local compositions/*.html scene",
                ));
            }
        }
        VideoEngine::Manim => {
            let python = source
                .files()
                .get("scene.py")
                .context("Manim source is missing scene.py")?;
            if !python.contains("class SfumatoScene") {
                return Err(SfumatoError::render(
                    ErrorClass::InvalidOutput,
                    "Manim source must define class SfumatoScene",
                ));
            }
            let lowercase = python.to_ascii_lowercase();
            for forbidden in [
                "import os",
                "import sys",
                "subprocess",
                "socket",
                "requests",
                "urllib",
                "open(",
                "exec(",
                "eval(",
                "__import__",
                "environ",
            ] {
                if lowercase.contains(forbidden) {
                    return Err(SfumatoError::render(
                        ErrorClass::InvalidOutput,
                        format!("Manim source contains forbidden operation '{forbidden}'"),
                    ));
                }
            }
        }
        VideoEngine::Model => {
            return Err(SfumatoError::internal(
                "Direct model video unexpectedly has source files",
            ));
        }
    }
    Ok(())
}

fn normalize_hyperframe_parent_paths(source: VideoSourceDocument) -> Result<VideoSourceDocument> {
    if source.engine() != VideoEngine::Hyperframe {
        return Ok(source);
    }
    let mut files = source.files().clone();
    for (relative, content) in &mut files {
        if !content.contains("../") {
            continue;
        }
        let direct_composition = Path::new(relative).parent() == Some(Path::new("compositions"));
        if !direct_composition || content.contains("../../") || content.contains("..\\..") {
            return Err(SfumatoError::render(
                ErrorClass::InvalidOutput,
                format!(
                    "Video source file '{relative}' contains a parent path that escapes the managed source root"
                ),
            ));
        }
        *content = content.replace("../", "");
    }
    if let Some(index) = files.get_mut("index.html") {
        let lowercase = index.to_ascii_lowercase();
        if !lowercase.contains("<!doctype html")
            || !lowercase.contains("<html")
            || !lowercase.contains("<body")
        {
            *index = format!(
                "<!DOCTYPE html><html><head><meta charset=\"UTF-8\"></head><body>{index}</body></html>"
            );
        }
    }
    VideoSourceDocument::new(VideoEngine::Hyperframe, files).map_err(review_error)
}

fn write_source(
    source: &VideoSourceDocument,
    root: &Path,
    workspace: &dyn WorkspaceFileSystem,
) -> Result<()> {
    workspace.create_dir_all(root)?;
    for (relative, content) in source.files() {
        workspace.write(&root.join(relative), content.as_bytes())?;
    }
    Ok(())
}

async fn validate_write_source(
    source: &VideoSourceDocument,
    source_root: &Path,
    workspace: &dyn WorkspaceFileSystem,
    renderer: &dyn VideoRenderer,
    engine: VideoEngine,
    request: &VideoRenderRequest,
    operation: &OperationContext,
) -> Result<()> {
    validate_source(source)?;
    write_source(source, source_root, workspace)?;
    renderer.validate(engine, request, operation).await
}

fn local_render_request(
    source_root: &Path,
    output_path: &Path,
    video: &GenerateVideoRequest,
) -> Result<VideoRenderRequest> {
    let (width, height) = resolution_dimensions(&video.resolution, &video.aspect_ratio)?;
    Ok(VideoRenderRequest {
        source_root: source_root.into(),
        output_path: output_path.into(),
        duration_seconds: video.duration_seconds,
        width,
        height,
        fps: video.fps,
        quality: video.quality.clone(),
    })
}

fn validate_video_options(config: &EffectiveConfig, video: &GenerateVideoRequest) -> Result<()> {
    if video.duration_seconds == 0 || video.duration_seconds > 3_600 {
        return Err(SfumatoError::validation(
            "Video duration must be between 1 and 3600 seconds",
        ));
    }
    if video.fps == 0 || video.fps > 120 {
        return Err(SfumatoError::validation(
            "Video FPS must be between 1 and 120",
        ));
    }
    if !matches!(video.quality.as_str(), "draft" | "standard" | "high") {
        return Err(SfumatoError::validation(
            "Video quality must be draft, standard, or high",
        ));
    }
    if video.engine != VideoEngine::Model && video.audio != VideoAudioMode::Off {
        return Err(SfumatoError::validation(
            "Hyperframe and Manim are silent in this version; use --audio off",
        ));
    }
    if video.engine == VideoEngine::Manim
        && !(video.allow_code_execution || config.security.allow_manim)
    {
        return Err(SfumatoError::validation(
            "Manim executes generated Python. Pass --allow-code-execution or enable project security.allow_manim.",
        ));
    }
    if !video.urls.is_empty() && video.engine != VideoEngine::Hyperframe {
        return Err(SfumatoError::validation(
            "Website capture sources are only supported by Hyperframe",
        ));
    }
    for url in &video.urls {
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err(SfumatoError::validation(
                "Each --url value must be an absolute http(s) URL",
            ));
        }
    }
    resolution_dimensions(&video.resolution, &video.aspect_ratio).map(|_| ())
}

fn resolution_dimensions(resolution: &str, aspect: &str) -> Result<(u32, u32)> {
    let height = match resolution {
        "480p" => 480,
        "720p" => 720,
        "1080p" => 1080,
        _ => {
            return Err(SfumatoError::validation(format!(
                "Unsupported local resolution '{resolution}'"
            )));
        }
    };
    let (x, y) = aspect
        .split_once(':')
        .context("Aspect ratio must use WIDTH:HEIGHT")?;
    let x: u32 = x.parse().map_err(SfumatoError::validation)?;
    let y: u32 = y.parse().map_err(SfumatoError::validation)?;
    if x == 0 || y == 0 {
        return Err(SfumatoError::validation(
            "Aspect ratio values must be positive",
        ));
    }
    let width = ((height as f64 * x as f64 / y as f64) / 2.0).round() as u32 * 2;
    Ok((width, height))
}

fn validate_inspection(
    inspection: &VideoInspection,
    video: &GenerateVideoRequest,
    expected_audio: Option<bool>,
) -> Result<()> {
    let expected = video.duration_seconds as f64;
    if (inspection.duration_seconds - expected).abs() > expected.mul_add(0.10, 1.0).max(1.0) {
        return Err(SfumatoError::render(
            ErrorClass::InvalidOutput,
            format!(
                "Rendered video duration {:.2}s does not match requested {}s",
                inspection.duration_seconds, video.duration_seconds
            ),
        ));
    }
    let (width, height) = resolution_dimensions(&video.resolution, &video.aspect_ratio)?;
    if inspection.width != width || inspection.height != height {
        return Err(SfumatoError::render(
            ErrorClass::InvalidOutput,
            format!(
                "Rendered video is {}x{}; expected {}x{}",
                inspection.width, inspection.height, width, height
            ),
        ));
    }
    if expected_audio.is_some_and(|expected| inspection.has_audio != expected) {
        return Err(SfumatoError::render(
            ErrorClass::InvalidOutput,
            "Rendered video audio streams do not match the requested policy",
        ));
    }
    Ok(())
}

fn publish_video(
    workspace: &dyn WorkspaceFileSystem,
    root: Option<&Path>,
    video: &Path,
    slug: &str,
    warnings: &mut Vec<String>,
) -> Result<Vec<PathBuf>> {
    let Some(root) = root else {
        return Ok(Vec::new());
    };
    let destination = video_publish_destination(root, slug);
    match workspace.publish_atomic(video, &destination) {
        Ok(path) => Ok(vec![path]),
        Err(error) => {
            warnings.push(format!(
                "Video publication failed; managed revision was preserved: {error}"
            ));
            Ok(Vec::new())
        }
    }
}

fn video_publish_destination(root: &Path, slug: &str) -> PathBuf {
    root.join("_sfumato/videos").join(slug)
}

#[allow(clippy::too_many_arguments)]
fn pause_for_visual_review(
    config: &EffectiveConfig,
    artifact_store: &dyn ArtifactStore,
    workspace: &dyn WorkspaceFileSystem,
    staging_root: &Path,
    slug: &str,
    video: &GenerateVideoRequest,
    title: String,
    engine: VideoEngine,
    models: BTreeMap<String, String>,
    tools: Vec<GenerationToolSummary>,
    project_assets: Vec<ProjectAssetReference>,
    review: VideoReviewSummary,
    prompts: Vec<PromptProvenance>,
    mut warnings: Vec<String>,
    publish_root: Option<&Path>,
) -> Result<GenerateVideoResult> {
    let suffix = staging_root
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("review");
    let review_id = format!("{slug}-{suffix}");
    let root = artifact_store
        .project_root(&config.project_name)?
        .join("review-sessions")
        .join(&review_id);
    if workspace.read_dir(&root).is_ok() {
        return Err(SfumatoError::validation(
            "Generated review session identifier already exists; refusing to overwrite immutable review evidence",
        ));
    }
    workspace.copy_tree(staging_root, &root, &[])?;
    workspace.write(
        &root.join("review.json"),
        serde_json::to_vec_pretty(&ReviewSessionRecord {
            schema_version: 1,
            review_id: review_id.clone(),
            project: config.project_name.clone(),
            engine,
            status: "pending_approval".into(),
            resolution: video.resolution.clone(),
            aspect_ratio: video.aspect_ratio.clone(),
            fps: video.fps,
            quality: video.quality.clone(),
            source_hash: hash_text(&workspace.read_text(&root.join("source.json"))?),
            plan_hash: hash_text(&workspace.read_text(&root.join("plan.json"))?),
            publish_root: publish_root.map(Path::to_path_buf),
        })?
        .as_slice(),
    )?;
    warnings.push(format!(
        "Visual review is pending: {review_id}. Run `sfumato video preview {review_id}` then `sfumato video approve {review_id}`."
    ));
    let contact_sheet = root.join("contact-sheet.md");
    let artifacts = workspace.list_files(&root, &[])?;
    Ok(GenerateVideoResult {
        video_path: contact_sheet.clone(),
        published_paths: Vec::new(),
        prompt_preview: None,
        output: VideoGenerationOutput {
            project: config.project_name.clone(),
            title,
            engine,
            video_path: contact_sheet,
            models,
            tools,
            project_assets,
            artifacts,
            published_artifacts: Vec::new(),
            review,
            visual_review_mode: VideoVisualReviewMode::HumanApprovalRequired,
            review_session: Some(VideoReviewSession {
                review_id,
                status: "pending_approval".into(),
                root,
            }),
            visual_report: None,
            prompts,
            warnings,
        },
    })
}

fn snapshot_times(plan: &VideoPlanDocument) -> Vec<f32> {
    let mut values = BTreeSet::new();
    for scene in plan.scenes() {
        values.insert((scene.start_seconds + scene.duration_seconds / 2.0).to_bits());
        if scene.start_seconds > 0.0 {
            values.insert(scene.start_seconds.to_bits());
        }
    }
    values.into_iter().map(f32::from_bits).collect()
}

fn contact_sheet(plan: &VideoPlanDocument, snapshots: &[PathBuf]) -> String {
    let mut output = format!("# Contact sheet: {}\n\n", plan.title());
    if snapshots.is_empty() {
        output.push_str("No PNG snapshots were emitted by the managed renderer. Inspect the Hyperframe preview before approval.\n");
    } else {
        for snapshot in snapshots {
            output.push_str(&format!("- {}\n", snapshot.display()));
        }
    }
    output
}

fn design_document(plan: &VideoPlanDocument, theme: &ThemePackage) -> String {
    format!(
        "# DESIGN\n\nWorkflow: {:?}\n\nMessage: {}\n\nDirection: {}\n\nTheme: {}\n",
        plan.workflow(),
        plan.message(),
        plan.design_direction(),
        theme.manifest.name
    )
}

fn script_document(plan: &VideoPlanDocument) -> String {
    let mut output = format!("# SCRIPT\n\n{}\n\n", plan.narrative_arc());
    for scene in plan.scenes() {
        output.push_str(&format!(
            "## {}\n{}\n\n",
            scene.id,
            scene.production.on_screen_copy.join("\n")
        ));
    }
    output
}

fn storyboard(plan: &VideoPlanDocument) -> String {
    let mut output = format!(
        "# {}\n\nDuration: {} seconds\n\nMessage: {}\n\nArc: {}\n\n",
        plan.title(),
        plan.duration_seconds(),
        plan.message(),
        plan.narrative_arc()
    );
    for scene in plan.scenes() {
        output.push_str(&format!(
            "## {} ({:.2}s–{:.2}s)\n\nRole: {}\n\n{}\n\nVisual: {}\n\nLayout: {}\n\nMotion: {}\n\nTransition: {}\n\nAcceptance: {}\n\n",
            scene.id,
            scene.start_seconds,
            scene.start_seconds + scene.duration_seconds,
            scene.production.narrative_role,
            scene.content,
            scene.visual,
            scene.production.layout,
            scene.production.motion_rules.join(", "),
            scene.production.transition,
            scene.production.acceptance.join("; "),
        ));
    }
    output
}

fn artifact_file(root: &Path, path: &Path) -> Result<ResourceArtifactFile> {
    let relative = path
        .strip_prefix(root)
        .context("Video artifact escaped staging")?
        .to_path_buf();
    let extension = relative
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let (kind, media_type) = match extension {
        "mp4" => (ArtifactKind::Video, Some("video/mp4".into())),
        "json" => (ArtifactKind::Data, Some("application/json".into())),
        "md" => (ArtifactKind::Markdown, Some("text/markdown".into())),
        "py" | "js" | "css" => (ArtifactKind::Source, Some("text/plain".into())),
        "html" => (ArtifactKind::Source, Some("text/html".into())),
        _ => (ArtifactKind::Source, None),
    };
    Ok(ResourceArtifactFile {
        path: relative,
        kind,
        media_type,
    })
}

fn build_source_bundle(documents: &[SourceDocument], limit: usize) -> String {
    let mut output = String::new();
    for document in documents {
        let header = format!("\n\n## {}\n", document.path.display());
        if output.chars().count() + header.chars().count() >= limit {
            break;
        }
        output.push_str(&header);
        let remaining = limit.saturating_sub(output.chars().count());
        output.extend(document.content.chars().take(remaining));
    }
    if output.is_empty() {
        "No source files were supplied.".into()
    } else {
        output
    }
}

fn summarize_tools(tools: &[ToolDefinition]) -> Vec<GenerationToolSummary> {
    tools
        .iter()
        .map(|tool| GenerationToolSummary {
            name: tool.function.name.clone(),
            description: tool.function.description.clone(),
        })
        .collect()
}

fn format_tokens(tokens: &BTreeMap<String, String>) -> String {
    if tokens.is_empty() {
        return "unspecified".into();
    }
    tokens
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn emit_stage(
    sink: &Option<Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
    stage: GenerationStage,
    profile: Option<&str>,
) {
    if let Some(sink) = sink {
        sink(TextGenerationEvent::StageStarted {
            stage,
            profile: profile.map(str::to_owned),
        });
    }
}

fn deduplicate_prompts(prompts: &mut Vec<PromptProvenance>) {
    let mut seen = BTreeSet::new();
    prompts.retain(|prompt| seen.insert((prompt.id, prompt.content_hash.clone())));
}

fn strip_json_fence(value: &str) -> &str {
    value
        .trim()
        .strip_prefix("```json")
        .or_else(|| value.trim().strip_prefix("```JSON"))
        .or_else(|| value.trim().strip_prefix("```"))
        .unwrap_or(value.trim())
        .strip_suffix("```")
        .unwrap_or(value.trim())
        .trim()
}

fn review_error(error: impl std::fmt::Display) -> SfumatoError {
    SfumatoError::provider(ErrorClass::InvalidOutput, error.to_string())
}

fn engine_name(engine: VideoEngine) -> &'static str {
    match engine {
        VideoEngine::Hyperframe => "hyperframe",
        VideoEngine::Manim => "manim",
        VideoEngine::Model => "model",
    }
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private workflow invariants.
#[path = "../../tests/unit/resources_videos.rs"]
mod tests;
