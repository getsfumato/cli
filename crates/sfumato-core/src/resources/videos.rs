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
    config::{
        Capability, EffectiveConfig, GenerationToolKind, ModelRole, SpeechModelOptions,
        VideoAudioMode,
    },
    errors::{
        ErrorClass, OperationStage, ResultContext as Context, SfumatoError, SfumatoResult as Result,
    },
    filesystem::WorkspaceFileSystem,
    generation::{
        GenerationRequest, GenerationToolSummary, ReviewStatus, VideoFrameDefect,
        VideoFrameDefectKind, VideoFrameMeasurement, VideoGenerationOutput, VideoNarrationSummary,
        VideoReviewSession, VideoReviewSummary, VideoVisualReport, VideoVisualReviewMode,
    },
    operation::OperationContext,
    project_assets::{ProjectAssetCatalog, ProjectAssetReference},
    prompts::{PromptCatalog, PromptId, PromptProvenance, PromptRenderRequest, PromptVariables},
    providers::{
        GenerationStage, ImageGenerationProvider, ProviderFactory, SpeechGenerationProvider,
        TextGenerationEvent, TextGenerationRequest, ToolDefinition, VideoGenerationRequest,
    },
    renderers::{
        VideoCatalog, VideoCatalogViolation, VideoInspection, VideoRenderRequest, VideoRenderer,
    },
    repositories::ThemeRepository,
    sources::{SourceDocument, SourceReader},
    themes::ThemePackage,
    tools::{GenerationToolFactory, GenerationToolsRequest, ImageToolConfig},
};

mod assembly;

use assembly::{
    CAPTIONS_COMPOSITION_ID, NarrationClip, NarrationLayer, captions_composition_html,
    master_index_html, master_meta_json, scene_composition_path, strip_markup_fence,
    validate_scene_composition,
};

use super::narration::{
    CaptionGroup, NarrationSegmentRequest, NarrationTrack, SynthesizeNarrationRequest,
    caption_groups, synthesize_narration,
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
    /// Audio policy: narration for Hyperframe, native audio for direct models.
    pub audio: VideoAudioMode,
    /// Voice override for this film, replacing the speech profile's own.
    pub voice: Option<String>,
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
    /// Whether a speech profile will voice this film's planned narration.
    narration_available: bool,
    plan_snapshot: String,
    source_snapshot: String,
    validation_error: String,
    max_tool_rounds: usize,
    workflow: String,
    urls: Vec<String>,
    catalog: String,
    retry_present: bool,
    retry_error: String,
    retry_invalid_response: String,
    scene_id: String,
    scene_position: usize,
    scene_count: usize,
    scene_snapshot: String,
    /// The words spoken over this beat, so the visuals land with the voice.
    scene_narration: String,
    scene_start_seconds: f32,
    scene_duration_seconds: f32,
    scene_catalog_items: Vec<SceneCatalogReference>,
    scene_artifacts: Vec<String>,
    /// How the previous beat leaves the frame.
    ///
    /// The seam rule is only actionable if the author knows what it is entering
    /// from, so the plan's exit for the preceding scene travels with the request.
    previous_scene_exit: String,
    /// What the deterministic frame measurements found, for the visual reviewer.
    ///
    /// Handed over so the model spends its attention on what pixels counting
    /// cannot see — legibility, overlap, composition — instead of re-deriving
    /// coverage numbers it would only guess at.
    frame_measurements: String,
}

/// One managed catalog piece offered to a scene author as a worked example.
///
/// Carries the item's own source rather than a path to mount. Measured against the
/// registry: these files are showcase documents whose copy is a demonstration, so
/// mounting one put "Should I learn to code?" into a film about fibre optics, and
/// the author — forbidden to edit a mounted block — covered it with its own ground
/// until the renderer rejected the scene for hidden text. The technique is the part
/// worth having, so the technique is what travels.
#[derive(Clone, Debug, Serialize)]
struct SceneCatalogReference {
    /// Registry identifier.
    id: String,
    /// The item's authored markup.
    source: String,
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
    narration_available: bool,
    max_tool_rounds: usize,
    catalog: Option<&'a VideoCatalog>,
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
        voice: None,
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
    // The approved source decides this rather than the request: a narrated film
    // reaches approval with its audio already on the timeline, and asserting
    // silence here would reject exactly the films narration was built for.
    let narrated_source = source
        .files()
        .get("index.html")
        .is_some_and(|html| html.contains("<audio"));
    validate_inspection(&inspection, &video, Some(narrated_source))?;
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
                frame_defects: Vec::new(),
            },
            visual_review_mode: VideoVisualReviewMode::HumanApprovalRequired,
            review_session: None,
            visual_report: None,
            narration: None,
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

/// Describes the managed catalog for the planner, or says it is unavailable.
///
/// Generated from the renderer's own manifest rather than restated here: a
/// hand-written list drifts from what is installed, and the planner is told to
/// select only catalog IDs, so a stale name becomes a broken composition.
fn catalog_summary(catalog: Option<&VideoCatalog>) -> String {
    match catalog {
        Some(catalog) => catalog.summary(),
        None => {
            "no managed catalog is available for this engine; author every element directly".into()
        }
    }
}

/// Reports every way a drafted plan disagrees with the managed catalog.
fn catalog_violations(
    plan: &VideoPlanDocument,
    catalog: Option<&VideoCatalog>,
) -> Vec<VideoCatalogViolation> {
    let Some(catalog) = catalog else {
        return Vec::new();
    };
    plan.scenes()
        .iter()
        .flat_map(|scene| {
            catalog.validate_selection(
                &scene.id,
                scene.duration_seconds,
                &scene.production.catalog_items,
            )
        })
        .collect()
}

/// The warning one catalog violation contributes, in a single stable spelling.
///
/// Shared by the draft and post-review checks so a violation the draft already
/// reported is recognisable and never repeated after a reviewer patch.
fn catalog_warning(violation: &VideoCatalogViolation) -> String {
    format!("Video plan catalog: {violation}")
}

/// Truncates text destined for a prompt.
fn excerpt(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }
    value.chars().take(max_chars).collect()
}

fn hash_text(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

/// Executes one engine-explicit video generation workflow.
pub(crate) async fn generate_video(
    config: EffectiveConfig,
    request: GenerationRequest,
    mut video: GenerateVideoRequest,
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
    let audio_dir = asset_root.join("audio");

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
    // Narration is a Hyperframe-only stage: a direct model produces its own
    // audio, and Manim has no timeline to hang an audio track on.
    let narration_requested = video.engine == VideoEngine::Hyperframe
        && video.audio != VideoAudioMode::Off
        && config.generation_tool_enabled(GenerationToolKind::AudioGen);
    let speech_selection = match (narration_requested, video.audio) {
        (false, _) => None,
        // An explicit `--audio on` that cannot be honoured is a failure, not a
        // silent film: the caller asked for a voice and would otherwise only
        // discover its absence by watching the result.
        (true, VideoAudioMode::On) => Some(config.resolve_model(Capability::Speech)?),
        (true, _) => config.resolve_model(Capability::Speech).ok(),
    };
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
        // Narration is not a tool here. The planner writes the spoken line for
        // each beat and Sfumato speaks the whole script at once, because the
        // timeline is retimed around it: a tool that returned audio mid-plan
        // would have nowhere to put the seconds it just created.
        audio: None,
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
    // Looking at frames needs a reviewer that can receive them. A text-only
    // profile leaves the snapshots as evidence for a human, which is what
    // `EvidenceOnly` reports rather than pretending an inspection happened.
    let visual_reviewer = reviewer.filter(|(_, profile)| {
        video.engine == VideoEngine::Hyperframe && profile.capabilities.contains(&Capability::Image)
    });
    let mut models = BTreeMap::from([("text".into(), draft_name.to_string())]);
    if let Some((name, _)) = reviewer {
        models.insert("reviewer".into(), name.to_string());
    }
    if let Some((name, _)) = visual_reviewer {
        models.insert("visual_reviewer".into(), name.to_string());
    }
    if let Some((name, _)) = image_selection {
        models.insert("image".into(), name.to_string());
    }
    if let Some((name, _)) = speech_selection {
        models.insert("speech".into(), name.to_string());
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
    // Read once and reuse: the planner is shown these items and the drafted
    // plan is checked against the same list, so both must see one snapshot.
    let managed_catalog = video_renderer.catalog(video.engine)?;
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
        narration_available: speech_selection.is_some(),
        max_tool_rounds: draft_profile.options.tool_rounds(),
        catalog: managed_catalog.as_ref(),
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
                narration: None,
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
    let (mut plan, catalog_warnings) = parse_plan(
        &response.text,
        video.engine,
        video.duration_seconds,
        video.title.as_deref(),
        video.workflow,
        managed_catalog.as_ref(),
    )?;
    let mut warnings = prepared_assets.warnings.clone();
    warnings.extend(catalog_warnings);
    if let Some((reviewer_name, reviewer_profile)) = reviewer {
        emit_stage(
            &event_sink,
            GenerationStage::VideoReview,
            Some(reviewer_name),
        );
        context.plan_snapshot =
            serde_json::to_string_pretty(&plan.snapshot().map_err(review_error)?)?;
        let reviewer_provider = provider_factory.text(&config, reviewer_profile)?;
        // One corrective retry, the same allowance slides gets. A reviewer that
        // answers with prose instead of a patch used to cost the entire review,
        // and that is the most common way a good review is thrown away.
        let mut attempt = 0;
        let outcome = loop {
            attempt += 1;
            let mut review_request = render_request(
                prompt_catalog.as_ref(),
                PromptId::VideoReviewSystem,
                PromptId::VideoReviewUser,
                &context,
            )?;
            review_request.tools = review_tools.clone();
            review_request.tool_executor = Some(tool_set.executor.clone());
            review_request.event_sink = event_sink.clone();
            prompts.extend(review_request.prompt_provenance.clone());
            let mut candidate = plan.clone();
            let response = reviewer_provider
                .generate_text(review_request, &operation, OperationStage::Review)
                .await;
            let attempted = match response {
                Ok(response) => crate::review::parse_json_patch(&response.text)
                    .map_err(review_error)
                    .and_then(|patch| candidate.apply_patch(&patch).map_err(review_error))
                    .map(|_| candidate)
                    .map_err(|error| (error, response.text)),
                // A transport failure is not something a corrective prompt fixes.
                Err(error) => break Err(error),
            };
            match attempted {
                Ok(candidate) => break Ok(candidate),
                Err((error, invalid)) if attempt == 1 => {
                    context.retry_present = true;
                    context.retry_error = format!("{error:#}");
                    context.retry_invalid_response = excerpt(&invalid, 4_000);
                }
                Err((error, _)) => break Err(error),
            }
        };
        context.retry_present = false;
        match outcome {
            Ok(candidate) => {
                // The reviewer patches the plan freely, so re-check its
                // selections: a patch can introduce an ID the catalog lacks.
                // Only an unusable selection costs the patch, and by the same
                // rule the draft was held to: rejecting a tolerated truncated
                // reveal here would throw away every unrelated correction the
                // patch carried, and would do it on every plan that already
                // shipped that violation.
                let violations = catalog_violations(&candidate, managed_catalog.as_ref());
                let unusable = violations
                    .iter()
                    .filter(|violation| violation.is_unusable())
                    .map(VideoCatalogViolation::to_string)
                    .collect::<Vec<_>>();
                if unusable.is_empty() {
                    for warning in violations.iter().map(catalog_warning) {
                        if !warnings.contains(&warning) {
                            warnings.push(warning);
                        }
                    }
                    plan = candidate;
                    review_summary.semantic_review = ReviewStatus::Completed;
                } else {
                    review_summary.semantic_review = ReviewStatus::Failed;
                    warnings.push(format!(
                        "Video semantic review introduced unusable catalog selections; using validated plan: {}",
                        unusable.join("; ")
                    ));
                }
            }
            Err(error) => {
                review_summary.semantic_review = ReviewStatus::Failed;
                warnings.push(format!(
                    "Video semantic review failed after one corrective retry; using validated plan: {error}"
                ));
            }
        }
    }
    // Narration is spoken before anything is written, because it decides the
    // timeline: a beat's window has to be at least as long as the words carried
    // over it, and every downstream document — plan, storyboard, master
    // composition, render request, inspection — has to agree on the result.
    let mut narrated: Option<NarratedFilm> = None;
    if let Some((speech_name, speech_profile)) = speech_selection {
        if plan
            .scenes()
            .iter()
            .all(|scene| scene.narration.trim().is_empty())
        {
            warnings.push(
                "A speech profile is configured but the plan carries no narration; the film stays silent"
                    .into(),
            );
        } else {
            emit_stage(
                &event_sink,
                GenerationStage::VideoNarration,
                Some(speech_name),
            );
            let provider = provider_factory.speech(&config, speech_profile)?;
            let mut speech_options = speech_profile.options.speech.clone();
            if let Some(voice) = video.voice.clone() {
                speech_options.voice = Some(voice);
            }
            let film = narrate_film(NarrateFilmRequest {
                plan: &mut plan,
                requested_duration: video.duration_seconds,
                provider: provider.as_ref(),
                options: &speech_options,
                output_dir: &audio_dir,
                workspace: workspace.as_ref(),
                operation: &operation,
            })
            .await?;
            if film.duration_seconds != video.duration_seconds {
                warnings.push(format!(
                    "Narration runs {}s, so the film was retimed from the requested {}s",
                    film.duration_seconds, video.duration_seconds
                ));
                video.duration_seconds = film.duration_seconds;
                context.duration_seconds = film.duration_seconds;
            }
            narrated = Some(film);
        }
    }
    let plan_text = plan.render().map_err(review_error)?;
    // Which assets survive is decided by the document that actually references
    // them, and for a local engine that document is the authored source, not the
    // plan. Gating on the plan deleted every generated image the planner did not
    // happen to name in its JSON, before the author could ever embed one.
    // Set by whichever engine arm runs; both assign before anything reads it.
    let used_project_assets;
    // Declared out here so the committed output can report the verdict that the
    // render branch produced.
    let mut visual_report = None;
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
    // The spoken track is evidence: it is what the captions were built from, so
    // a mis-timed caption can be traced to the alignment that produced it.
    if let Some(film) = &narrated {
        workspace.write(
            &staging_root.join("narration.json"),
            serde_json::to_vec_pretty(&film.track)?.as_slice(),
        )?;
    }

    let expected_audio = match video.engine {
        // A local render's audio is exactly what Sfumato put on the timeline, so
        // the inspection asserts that rather than restating the request: a film
        // that lost its narration between synthesis and mux is a defect, and a
        // silent film that grew an audio stream is one too.
        VideoEngine::Hyperframe => Some(narrated.is_some()),
        VideoEngine::Manim => Some(false),
        VideoEngine::Model => match video.audio {
            VideoAudioMode::Auto => None,
            VideoAudioMode::On => Some(true),
            VideoAudioMode::Off => Some(false),
        },
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
            // A direct model receives the plan's prompt, so the plan is the
            // document that decides which references travel with it.
            let (mut references, used) =
                prepared_assets.materialize_referenced(&plan_text, workspace.as_ref())?;
            used_project_assets = used;
            references.extend(retain_referenced_generated_assets(
                generated_assets,
                &plan_text,
                "assets/images",
                workspace.as_ref(),
            )?);
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
            let author = provider_factory.text(&config, profile)?;
            let mut source = if video.engine == VideoEngine::Hyperframe {
                // One scene per request. A single response used to carry the whole
                // film, which capped the practical duration and spent the model's
                // attention on restating the master timeline's contract instead of
                // on the beat it was writing.
                let (width, height) =
                    resolution_dimensions(&video.resolution, &video.aspect_ratio)?;
                author_hyperframe_scenes(AuthorScenesRequest {
                    plan: &plan,
                    narration: narrated.as_ref(),
                    catalog: managed_catalog.as_ref(),
                    renderer: video_renderer.as_ref(),
                    engine: video.engine,
                    slug: &slug,
                    width,
                    height,
                    author: author.as_ref(),
                    prompt_catalog: prompt_catalog.as_ref(),
                    context: &mut context,
                    prompts: &mut prompts,
                    event_sink: &event_sink,
                    operation: &operation,
                })
                .await?
            } else {
                let mut author_request = render_request(
                    prompt_catalog.as_ref(),
                    PromptId::VideoManimSystem,
                    PromptId::VideoManimUser,
                    &context,
                )?;
                author_request.event_sink = event_sink.clone();
                prompts.extend(author_request.prompt_provenance.clone());
                let response = author
                    .generate_text(author_request, &operation, OperationStage::Draft)
                    .await?;
                parse_source(&response.text, video.engine)?
            };
            let local_request = local_render_request(&source_root, &video_path, &video)?;
            // Every project asset goes to disk before validation, because the
            // renderer's own check fails on a referenced file that is not there
            // and the authored source is what decides which ones are referenced.
            // The unused copies are removed once the source has settled.
            prepared_assets.materialize_all(workspace.as_ref())?;
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
                // The reason for the repair is worth keeping: without it a caller
                // sees a repair happened but never learns what was wrong, which is
                // exactly the position this project was in while being debugged.
                warnings.push(format!("Video source needed repair: {error}"));
                let named_scene = if video.engine == VideoEngine::Hyperframe {
                    failing_scene(&source, &error)
                } else {
                    None
                };
                if let Some(sink) = &event_sink {
                    sink(TextGenerationEvent::SourceRepairStarted {
                        reason: error.to_string(),
                        scene: named_scene.clone(),
                    });
                }
                let (reviewer_name, reviewer_profile) = reviewer
                    .or(engine_profile)
                    .expect("local video engine has an author profile");
                emit_stage(
                    &event_sink,
                    GenerationStage::VideoRepair,
                    Some(reviewer_name),
                );
                // A whole-film patch is not viable once scenes are authored
                // separately: the snapshot carries every scene, and a patch that
                // replaces a file has to restate it in full, so the response runs
                // past the model's output limit. Re-authoring the one scene that
                // failed is both smaller and the path already known to work.
                if let Some(first_scene) = named_scene {
                    // One scene per round, with a budget sized to how much is wrong.
                    // A film that started with nine faults was cut off at three
                    // remaining by a fixed budget of four, while a film with one
                    // fault never needed four. Progress is what actually decides
                    // when to stop: a round that fixes nothing is not going to be
                    // rescued by another.
                    let mut faults = reported_faults(&error.to_string());
                    let budget = repair_rounds(faults);
                    let mut scene_id = first_scene;
                    let mut failure = error.to_string();
                    let mut round = 0;
                    let mut stalled = 0;
                    loop {
                        round += 1;
                        source = reauthor_scene(ReauthorSceneRequest {
                            source,
                            catalog: managed_catalog.as_ref(),
                            renderer: video_renderer.as_ref(),
                            engine: video.engine,
                            scene_id: &scene_id,
                            plan: &plan,
                            failure: &failure,
                            author: provider_factory.text(&config, profile)?.as_ref(),
                            prompt_catalog: prompt_catalog.as_ref(),
                            context: &mut context,
                            prompts: &mut prompts,
                            event_sink: &event_sink,
                            operation: &operation,
                        })
                        .await?;
                        match validate_write_source(
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
                            Ok(()) => {
                                review_summary.source_repair = ReviewStatus::Completed;
                                break;
                            }
                            // A named scene means there is still something focused
                            // to fix. Anything else, or a film that will not settle,
                            // is reported rather than retried forever.
                            Err(next) => {
                                let remaining = reported_faults(&next.to_string());
                                // Two rounds without a single fault cleared: the
                                // author is circling rather than converging, and
                                // every further round costs a model call plus a
                                // full renderer check.
                                stalled = if remaining < faults { 0 } else { stalled + 1 };
                                faults = remaining;
                                let next_scene = another_repair_round(round, budget, stalled)
                                    .then(|| failing_scene(&source, &next))
                                    .flatten();
                                let Some(next_scene) = next_scene else {
                                    return Err(next);
                                };
                                warnings.push(format!("Video source needed repair: {next}"));
                                if let Some(sink) = &event_sink {
                                    sink(TextGenerationEvent::SourceRepairStarted {
                                        reason: next.to_string(),
                                        scene: Some(next_scene.clone()),
                                    });
                                }
                                scene_id = next_scene;
                                failure = next.to_string();
                            }
                        }
                    }
                } else {
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
            }
            // The source has settled: keep exactly the assets it references.
            let source_text = source_reference_text(&source);
            let (paths, used) =
                prepared_assets.materialize_referenced(&source_text, workspace.as_ref())?;
            for unused in prepared_assets
                .allowed_paths()
                .into_iter()
                .filter(|path| !paths.contains(path))
            {
                workspace.remove_file(&unused)?;
            }
            used_project_assets = used;
            // The retained list is not needed further: the revision manifest
            // discovers its files by walking the staging root. This call is here
            // for its effect, which is deleting the images nothing references.
            retain_referenced_generated_assets(
                generated_assets,
                &source_text,
                "assets/images",
                workspace.as_ref(),
            )?;
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
                &staging_root.join("contact-sheet.md"),
                contact_sheet(&plan, &snapshots).as_bytes(),
            )?;

            // Snapshots were already being captured and written for a human to
            // look at; measuring them is what lets the workflow act on the same
            // evidence. A scene that opens on nothing is the defect this catches,
            // and it is the one real videos kept shipping with.
            let timed = snapshot_times(&plan)
                .into_iter()
                .zip(snapshots.iter().cloned())
                .collect::<Vec<_>>();
            let measurements = video_renderer.measure_snapshots(&timed, &operation).await?;
            let defects = classify_frames(&plan, &measurements);

            // What counting pixels cannot see: legibility, overlap, composition.
            // Only run when a reviewer can actually look — a text-only connector
            // would answer from the prompt alone, which is worse than not asking.
            if let Some((visual_name, visual_profile)) = visual_reviewer {
                emit_stage(
                    &event_sink,
                    GenerationStage::VideoVisualReview,
                    Some(visual_name),
                );
                let outcome = review_frames_visually(VisualReviewRequest {
                    plan: &plan,
                    timed: &timed,
                    measurements: &measurements,
                    reviewer: provider_factory.text(&config, visual_profile)?.as_ref(),
                    prompt_catalog: prompt_catalog.as_ref(),
                    context: &mut context,
                    prompts: &mut prompts,
                    event_sink: &event_sink,
                    operation: &operation,
                })
                .await;
                match outcome {
                    Ok((report, reviewed)) => {
                        if reviewed < timed.len() {
                            warnings.push(format!(
                                "Visual review looked at the first {reviewed} of {} frames",
                                timed.len()
                            ));
                        }
                        for finding in &report.findings {
                            warnings.push(format!("Visual review: {finding}"));
                        }
                        visual_report = Some(report);
                        review_summary.visual_review = ReviewStatus::Completed;
                    }
                    Err(error) => {
                        // Advisory by design: the frames and the deterministic
                        // verdict still ship, so a reviewer that fails is a lost
                        // opinion rather than a lost film.
                        warnings.push(format!("Visual review did not complete: {error}"));
                        review_summary.visual_review = ReviewStatus::Failed;
                    }
                }
            }

            if !defects.is_empty() {
                let described = defects
                    .iter()
                    .map(describe_frame_defect)
                    .collect::<Vec<_>>()
                    .join("; ");
                if review {
                    let (repair_name, repair_profile) = reviewer
                        .or(engine_profile)
                        .expect("local video engine has an author profile");
                    emit_stage(&event_sink, GenerationStage::VideoRepair, Some(repair_name));
                    context.source_snapshot =
                        serde_json::to_string_pretty(&source.snapshot().map_err(review_error)?)?;
                    context.validation_error = format!(
                        "The rendered film has empty frames. Every timed element must be visible at the instant its scene begins, so an entrance animates from a state that is already on screen rather than from nothing: {described}"
                    );
                    let mut repair_request = render_request(
                        prompt_catalog.as_ref(),
                        PromptId::VideoSourceRepairSystem,
                        PromptId::VideoSourceRepairUser,
                        &context,
                    )?;
                    repair_request.event_sink = event_sink.clone();
                    prompts.extend(repair_request.prompt_provenance.clone());
                    let repairer = provider_factory.text(&config, repair_profile)?;
                    let repaired = repairer
                        .generate_text(repair_request, &operation, OperationStage::Repair)
                        .await
                        .and_then(|response| {
                            let patch = crate::review::parse_json_patch(&response.text)
                                .map_err(review_error)?;
                            let mut candidate = source.clone();
                            candidate.apply_patch(&patch).map_err(review_error)?;
                            normalize_hyperframe_parent_paths(candidate)
                        });
                    match repaired {
                        Ok(candidate) => {
                            match validate_write_source(
                                &candidate,
                                &source_root,
                                workspace.as_ref(),
                                video_renderer.as_ref(),
                                video.engine,
                                &local_request,
                                &operation,
                            )
                            .await
                            {
                                Ok(()) => {
                                    source = candidate;
                                    // Re-measure: a repair is only worth keeping
                                    // when the frames actually filled in.
                                    let snapshots = video_renderer
                                        .snapshot(
                                            video.engine,
                                            &local_request,
                                            &snapshot_times(&plan),
                                            &staging_root.join("snapshots"),
                                            &operation,
                                        )
                                        .await?;
                                    let timed = snapshot_times(&plan)
                                        .into_iter()
                                        .zip(snapshots.iter().cloned())
                                        .collect::<Vec<_>>();
                                    let measurements = video_renderer
                                        .measure_snapshots(&timed, &operation)
                                        .await?;
                                    let remaining = classify_frames(&plan, &measurements);
                                    if remaining.len() < defects.len() {
                                        review_summary.source_repair = ReviewStatus::Completed;
                                    }
                                    if !remaining.is_empty() {
                                        warnings.push(format!(
                                            "Empty frames remain after one repair: {}",
                                            remaining
                                                .iter()
                                                .map(describe_frame_defect)
                                                .collect::<Vec<_>>()
                                                .join("; ")
                                        ));
                                    }
                                    review_summary.frame_defects = remaining;
                                    workspace.write(
                                        &staging_root.join("contact-sheet.md"),
                                        contact_sheet(&plan, &snapshots).as_bytes(),
                                    )?;
                                }
                                Err(error) => {
                                    warnings.push(format!(
                                        "Empty-frame repair produced invalid source; keeping the validated film: {error}"
                                    ));
                                    review_summary.frame_defects = defects;
                                }
                            }
                        }
                        Err(error) => {
                            warnings.push(format!(
                                "Empty-frame repair failed; keeping the validated film: {error}"
                            ));
                            review_summary.frame_defects = defects;
                        }
                    }
                } else {
                    warnings.push(format!("The rendered film has empty frames: {described}"));
                    review_summary.frame_defects = defects;
                }
            }
            // Written last so a repaired film ships the source that produced it.
            workspace.write(
                &staging_root.join("source.json"),
                source.render().map_err(review_error)?.as_bytes(),
            )?;
            // Capturing and measuring the evidence is itself the visual review when
            // no model looked at it. A reviewer that ran already set its own
            // verdict, and overwriting a failure here would report an inspection
            // that did not happen.
            if review_summary.visual_review == ReviewStatus::Skipped {
                review_summary.visual_review = ReviewStatus::Completed;
            }
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
                    narration_summary(narrated.as_ref(), models.get("speech")),
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
    let narration_summary = narration_summary(narrated.as_ref(), models.get("speech"));
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
            visual_review_mode: match (video.engine, visual_report.is_some()) {
                (_, true) => VideoVisualReviewMode::Automated,
                (VideoEngine::Hyperframe, false) => VideoVisualReviewMode::EvidenceOnly,
                _ => VideoVisualReviewMode::Disabled,
            },
            review_session: None,
            visual_report,
            narration: narration_summary,
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
        narration_available,
        max_tool_rounds,
        catalog,
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
        narration_available,
        max_tool_rounds,
        workflow: video.workflow.as_str().into(),
        urls: video.urls.clone(),
        catalog: catalog_summary(catalog),
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

/// Parses one planning response, returning the plan and catalog warnings.
///
/// Catalog policy is applied here, before the revision-guarded document exists,
/// because a selection that cannot render has to be removed rather than patched
/// out later: the source generator would reference a composition file that was
/// never installed.
fn parse_plan(
    response: &str,
    engine: VideoEngine,
    duration: u32,
    title: Option<&str>,
    requested_workflow: VideoWorkflow,
    catalog: Option<&VideoCatalog>,
) -> Result<(VideoPlanDocument, Vec<String>)> {
    let mut draft: VideoPlanDraft =
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
    let warnings = apply_catalog_policy(&mut draft.scenes, catalog);
    let plan = VideoPlanDocument::new_with_pipeline(
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
    .map_err(review_error)?;
    Ok((plan, warnings))
}

/// Drops catalog selections that cannot render and reports every violation.
///
/// Unknown IDs and whole-film treatments are removed because keeping them
/// guarantees a broken composition; a scene shorter than its block keeps the
/// selection and warns, since a truncated reveal still teaches something.
fn apply_catalog_policy(scenes: &mut [VideoScene], catalog: Option<&VideoCatalog>) -> Vec<String> {
    let Some(catalog) = catalog else {
        return Vec::new();
    };
    let mut warnings = Vec::new();
    for scene in scenes.iter_mut() {
        let violations = catalog.validate_selection(
            &scene.id,
            scene.duration_seconds,
            &scene.production.catalog_items,
        );
        let mut unusable = BTreeSet::new();
        for violation in &violations {
            warnings.push(catalog_warning(violation));
            if violation.is_unusable() {
                match violation {
                    VideoCatalogViolation::UnknownItem { id, .. }
                    | VideoCatalogViolation::GradePerScene { id, .. } => {
                        unusable.insert(id.clone());
                    }
                    VideoCatalogViolation::SceneTooShort { .. } => {}
                }
            }
        }
        scene
            .production
            .catalog_items
            .retain(|id| !unusable.contains(id));
    }
    warnings
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

/// A film whose narration has been spoken and whose timeline follows it.
struct NarratedFilm {
    /// Every spoken passage, written to disk and timed.
    track: NarrationTrack,
    /// Audio placed on the film's timeline, and whether captions accompany it.
    layer: NarrationLayer,
    /// Caption groups already offset onto the film's timeline.
    captions: Vec<CaptionGroup>,
    /// The film's length once every beat holds the words spoken over it.
    duration_seconds: u32,
}

struct NarrateFilmRequest<'a> {
    plan: &'a mut VideoPlanDocument,
    /// The duration the caller asked for, kept as a floor for the timeline.
    requested_duration: u32,
    provider: &'a dyn SpeechGenerationProvider,
    options: &'a SpeechModelOptions,
    output_dir: &'a Path,
    workspace: &'a dyn WorkspaceFileSystem,
    operation: &'a OperationContext,
}

/// Speaks the plan's narration and retimes the film around what came back.
///
/// The plan's own durations are an estimate written before a single word
/// existed. Once the audio exists, a beat lasts at least as long as its line
/// plus the pause after it, and the film lasts as long as its beats — so this
/// stretches windows and never shortens them: cutting a scene down to fit the
/// audio would throw away pacing the planner chose deliberately.
async fn narrate_film(request: NarrateFilmRequest<'_>) -> Result<NarratedFilm> {
    let NarrateFilmRequest {
        plan,
        requested_duration,
        provider,
        options,
        output_dir,
        workspace,
        operation,
    } = request;
    let track = synthesize_narration(SynthesizeNarrationRequest {
        segments: plan
            .scenes()
            .iter()
            .map(|scene| NarrationSegmentRequest {
                id: scene.id.clone(),
                text: scene.narration.trim().to_string(),
            })
            .collect(),
        provider,
        options,
        output_dir,
        reference_prefix: "assets/audio",
        workspace,
        operation,
        stage: OperationStage::Draft,
    })
    .await?;

    let mut scenes = plan.scenes().to_vec();
    let mut clips = Vec::new();
    let mut captions = Vec::new();
    let mut cursor = 0.0_f32;
    for scene in &mut scenes {
        let spoken = track.segment(&scene.id);
        let duration = spoken
            .map(|segment| segment.duration_seconds + track.segment_gap_seconds)
            .unwrap_or(0.0)
            .max(scene.duration_seconds);
        scene.start_seconds = cursor;
        scene.duration_seconds = duration;
        if let Some(segment) = spoken {
            clips.push(NarrationClip {
                reference: segment.reference.clone(),
                start_seconds: cursor,
                duration_seconds: segment.duration_seconds,
            });
            captions.extend(caption_groups(&segment.words, cursor));
        }
        cursor += duration;
    }
    // Rounded up so the last word is never clipped by a timeline that ends mid
    // syllable, and floored at the request so a short script still fills the
    // film the caller asked for.
    let duration_seconds = cursor.ceil().max(1.0) as u32;
    let duration_seconds = duration_seconds.max(requested_duration);
    if duration_seconds > MAX_NARRATED_SECONDS {
        return Err(SfumatoError::validation(format!(
            "Narration would make this film {duration_seconds}s, past the {MAX_NARRATED_SECONDS}s limit; shorten the script or split the video"
        )));
    }
    plan.set_timeline(scenes, duration_seconds)
        .map_err(review_error)?;
    Ok(NarratedFilm {
        layer: NarrationLayer {
            clips,
            captions: !captions.is_empty(),
        },
        captions,
        track,
        duration_seconds,
    })
}

/// Longest film narration may produce, matching the domain's own ceiling.
const MAX_NARRATED_SECONDS: u32 = 3_600;

struct AuthorScenesRequest<'a> {
    plan: &'a VideoPlanDocument,
    /// Narration to layer over the assembled film, when the film speaks.
    narration: Option<&'a NarratedFilm>,
    /// Installed catalog, so a selected piece reaches the author as an example.
    catalog: Option<&'a VideoCatalog>,
    renderer: &'a dyn VideoRenderer,
    engine: VideoEngine,
    slug: &'a str,
    width: u32,
    height: u32,
    author: &'a dyn crate::providers::TextGenerationProvider,
    prompt_catalog: &'a dyn PromptCatalog,
    context: &'a mut VideoPromptContext,
    prompts: &'a mut Vec<PromptProvenance>,
    event_sink: &'a Option<Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
    operation: &'a OperationContext,
}

/// Authors one composition per planned scene and assembles the project.
///
/// The entry composition is generated, so the model never restates the renderer's
/// contract; each request carries one beat, its own window, and how the previous
/// beat leaves the frame.
async fn author_hyperframe_scenes(request: AuthorScenesRequest<'_>) -> Result<VideoSourceDocument> {
    let AuthorScenesRequest {
        plan,
        narration,
        catalog,
        renderer,
        engine,
        slug,
        width,
        height,
        author,
        prompt_catalog,
        context,
        prompts,
        event_sink,
        operation,
    } = request;
    let layer = narration
        .map(|narration| narration.layer.clone())
        .unwrap_or_default();
    let mut files = BTreeMap::from([
        ("meta.json".to_string(), master_meta_json(slug)),
        (
            "index.html".to_string(),
            master_index_html(plan, width, height, &layer),
        ),
    ]);
    if let Some(narration) = narration.filter(|narration| narration.layer.captions) {
        files.insert(
            format!("compositions/{CAPTIONS_COMPOSITION_ID}.html"),
            captions_composition_html(&narration.captions, width, height),
        );
    }
    let scenes = plan.scenes();
    for (index, scene) in scenes.iter().enumerate() {
        operation.checkpoint(OperationStage::Draft)?;
        context.scene_id = scene.id.clone();
        context.scene_position = index + 1;
        context.scene_count = scenes.len();
        context.scene_start_seconds = scene.start_seconds;
        context.scene_duration_seconds = scene.duration_seconds;
        context.scene_catalog_items = scene_catalog_references(scene, catalog, renderer, engine);
        context.scene_artifacts = scene.artifacts.clone();
        context.scene_narration = scene.narration.clone();
        context.previous_scene_exit = index
            .checked_sub(1)
            .and_then(|previous| scenes.get(previous))
            .map(|previous| previous.production.exit.clone())
            .unwrap_or_default();
        context.scene_snapshot = serde_json::to_string_pretty(scene)?;

        let mut markup = String::new();
        // One corrective attempt per scene: a scene that comes back malformed is
        // re-asked with the exact complaint, and only that scene is re-generated.
        for attempt in 1..=2 {
            let mut scene_request = render_request(
                prompt_catalog,
                PromptId::VideoHyperframeSceneSystem,
                PromptId::VideoHyperframeSceneUser,
                context,
            )?;
            scene_request.event_sink = event_sink.clone();
            prompts.extend(scene_request.prompt_provenance.clone());
            let response = author
                .generate_text(scene_request, operation, OperationStage::Draft)
                .await?;
            let candidate = strip_markup_fence(&response.text);
            match validate_scene_composition(&scene.id, &candidate) {
                Ok(()) => {
                    markup = candidate;
                    break;
                }
                Err(error) if attempt == 1 => {
                    context.retry_present = true;
                    context.retry_error = error;
                    context.retry_invalid_response = excerpt(&candidate, 4_000);
                }
                Err(error) => {
                    return Err(SfumatoError::provider(
                        ErrorClass::InvalidOutput,
                        format!("Scene authoring failed after one corrective retry: {error}"),
                    ));
                }
            }
        }
        context.retry_present = false;
        files.insert(scene_composition_path(&scene.id), markup);
    }
    VideoSourceDocument::new(VideoEngine::Hyperframe, files).map_err(review_error)
}

/// Names the scene a validation failure points at, when it points at one.
///
/// Both the core validator and the renderer's own check quote the offending path,
/// so the scene can be recovered from the message rather than guessed at.
fn failing_scene(source: &VideoSourceDocument, error: &SfumatoError) -> Option<String> {
    let message = error.to_string();
    let scenes = source
        .files()
        .keys()
        .filter(|path| path.starts_with("compositions/") && path.ends_with(".html"))
        .filter_map(|path| {
            path.strip_prefix("compositions/")
                .and_then(|name| name.strip_suffix(".html"))
        })
        // The caption overlay is generated, not authored: naming it here would
        // send a re-author request for a scene the plan does not contain.
        .filter(|name| *name != CAPTIONS_COMPOSITION_ID)
        .collect::<Vec<_>>();
    // The path when the renderer quotes one, and otherwise the scene ID, because
    // the errors that matter most for quality — overflowing, occluded and
    // overlapping text — name the offending element rather than its file, and
    // authored element IDs carry the scene they belong to.
    scenes
        .iter()
        .find(|scene| message.contains(&format!("compositions/{scene}.html")))
        .or_else(|| scenes.iter().find(|scene| message.contains(**scene)))
        .map(|scene| (*scene).to_owned())
}

struct ReauthorSceneRequest<'a> {
    source: VideoSourceDocument,
    scene_id: &'a str,
    plan: &'a VideoPlanDocument,
    catalog: Option<&'a VideoCatalog>,
    renderer: &'a dyn VideoRenderer,
    engine: VideoEngine,
    failure: &'a str,
    author: &'a dyn crate::providers::TextGenerationProvider,
    prompt_catalog: &'a dyn PromptCatalog,
    context: &'a mut VideoPromptContext,
    prompts: &'a mut Vec<PromptProvenance>,
    event_sink: &'a Option<Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
    operation: &'a OperationContext,
}

/// Re-authors one scene against the failure it caused.
///
/// Replaces the whole-film patch for the case that matters: the response only has
/// to carry one scene, and it goes through the same prompt that produced every
/// other scene rather than a second, differently-shaped repair contract.
async fn reauthor_scene(request: ReauthorSceneRequest<'_>) -> Result<VideoSourceDocument> {
    let ReauthorSceneRequest {
        source,
        scene_id,
        plan,
        catalog,
        renderer,
        engine,
        failure,
        author,
        prompt_catalog,
        context,
        prompts,
        event_sink,
        operation,
    } = request;
    let scenes = plan.scenes();
    let position = scenes
        .iter()
        .position(|scene| scene.id == scene_id)
        .context("The failing scene is not in the plan")?;
    let scene = &scenes[position];

    context.scene_id = scene.id.clone();
    context.scene_position = position + 1;
    context.scene_count = scenes.len();
    context.scene_start_seconds = scene.start_seconds;
    context.scene_duration_seconds = scene.duration_seconds;
    context.scene_catalog_items = scene_catalog_references(scene, catalog, renderer, engine);
    context.scene_artifacts = scene.artifacts.clone();
    context.scene_narration = scene.narration.clone();
    context.previous_scene_exit = position
        .checked_sub(1)
        .and_then(|previous| scenes.get(previous))
        .map(|previous| previous.production.exit.clone())
        .unwrap_or_default();
    context.scene_snapshot = serde_json::to_string_pretty(scene)?;
    context.retry_present = true;
    context.retry_error = failure.to_string();
    context.retry_invalid_response = excerpt(
        source
            .files()
            .get(&scene_composition_path(scene_id))
            .map(String::as_str)
            .unwrap_or_default(),
        4_000,
    );

    let mut scene_request = render_request(
        prompt_catalog,
        PromptId::VideoHyperframeSceneSystem,
        PromptId::VideoHyperframeSceneUser,
        context,
    )?;
    scene_request.event_sink = event_sink.clone();
    prompts.extend(scene_request.prompt_provenance.clone());
    let response = author
        .generate_text(scene_request, operation, OperationStage::Repair)
        .await?;
    context.retry_present = false;
    let markup = strip_markup_fence(&response.text);
    validate_scene_composition(scene_id, &markup).map_err(|error| {
        SfumatoError::provider(
            ErrorClass::InvalidOutput,
            format!("Re-authored scene is still invalid: {error}"),
        )
    })?;
    let mut files = source.files().clone();
    files.insert(scene_composition_path(scene_id), markup);
    VideoSourceDocument::new(VideoEngine::Hyperframe, files).map_err(review_error)
}

/// How many frames one visual review may carry.
///
/// Every attachment is a full-resolution PNG inlined in the request, so the cap is
/// what keeps a long film from producing a request no connector will accept. Two
/// frames per scene means this covers a six-scene film whole.
const MAX_REVIEWED_FRAMES: usize = 12;

struct VisualReviewRequest<'a> {
    plan: &'a VideoPlanDocument,
    timed: &'a [(f32, PathBuf)],
    measurements: &'a [VideoFrameMeasurement],
    reviewer: &'a dyn crate::providers::TextGenerationProvider,
    prompt_catalog: &'a dyn PromptCatalog,
    context: &'a mut VideoPromptContext,
    prompts: &'a mut Vec<PromptProvenance>,
    event_sink: &'a Option<Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
    operation: &'a OperationContext,
}

/// Asks an image-capable model what the rendered frames actually look like.
///
/// The deterministic gate answers "is anything on screen"; nothing answers "is it
/// legible, does it overlap, does it read" without looking. The measurements travel
/// with the frames so the model spends its attention on what counting cannot see.
async fn review_frames_visually(
    request: VisualReviewRequest<'_>,
) -> Result<(VideoVisualReport, usize)> {
    let VisualReviewRequest {
        plan,
        timed,
        measurements,
        reviewer,
        prompt_catalog,
        context,
        prompts,
        event_sink,
        operation,
    } = request;
    let reviewed = timed.len().min(MAX_REVIEWED_FRAMES);
    let mut images = Vec::with_capacity(reviewed);
    for (at_seconds, path) in timed.iter().take(reviewed) {
        images.push(crate::providers::ImageAttachment {
            label: format!(
                "Frame at {at_seconds:.2}s{}",
                scene_at(plan, *at_seconds)
                    .map(|scene| format!(", scene {scene}"))
                    .unwrap_or_default()
            ),
            media_type: "image/png".to_string(),
            path: path.clone(),
        });
    }
    context.frame_measurements = measurements
        .iter()
        .map(|measurement| {
            format!(
                "- {:.2}s: {:.1}% of the frame differs from its dominant colour, {} distinct colours",
                measurement.at_seconds,
                measurement.ink_ratio * 100.0,
                measurement.distinct_colours
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut review_request = render_request(
        prompt_catalog,
        PromptId::VideoVisualReviewSystem,
        PromptId::VideoVisualReviewUser,
        context,
    )?;
    review_request.event_sink = event_sink.clone();
    review_request.images = images;
    prompts.extend(review_request.prompt_provenance.clone());
    let response = reviewer
        .generate_text(review_request, operation, OperationStage::Review)
        .await?;
    let report: VideoVisualReport =
        serde_json::from_str(strip_json_fence(&response.text)).map_err(|error| {
            review_error(format!(
                "Visual reviewer response must be a JSON object with `approved` and `findings`: {error}"
            ))
        })?;
    Ok((report, reviewed))
}

/// How many faults a renderer failure reports.
///
/// The renderer closes its report with a count and marks each fault with a cross,
/// so both spellings are read and the count wins when present. A failure that
/// itemises nothing at all is still one thing to fix.
fn reported_faults(message: &str) -> usize {
    if let Some(counted) = message
        .split(" error(s)")
        .next()
        .filter(|_| message.contains(" error(s)"))
        && let Some(digits) = counted
            .rsplit(|value: char| !value.is_ascii_digit())
            .next()
            .filter(|value| !value.is_empty())
        && let Ok(count) = digits.parse::<usize>()
        && count > 0
    {
        return count;
    }
    let crosses = message.matches('\u{2717}').count();
    crosses.max(1)
}

/// How many focused repair rounds a failure of this size is worth.
///
/// Proportional because the two ends behave differently: a scene with one clipped
/// label settles in a round or two, while a film reported with nine faults needs
/// roughly a round per scene at fault plus a second pass on the stubborn one. The
/// ceiling is what stops a pathological film from spending an afternoon, since each
/// round is a model call and a full renderer check.
fn repair_rounds(faults: usize) -> usize {
    faults.clamp(3, 8)
}

/// Rounds allowed to clear nothing before the film is reported instead.
///
/// One is too strict: a round that fixes the named scene can surface a fault the
/// renderer could not see behind it, which leaves the count level without meaning
/// the author is lost.
const MAX_STALLED_REPAIRS: usize = 2;

/// Whether another focused repair round is worth spending.
fn another_repair_round(round: usize, budget: usize, stalled: usize) -> bool {
    round < budget && stalled < MAX_STALLED_REPAIRS
}

/// The catalog pieces one scene selected, with the source the author adapts.
///
/// An item whose source cannot be read is dropped rather than named: an example the
/// author cannot see is worse than no example, and the beat is still authorable by
/// hand. An unknown selection is dropped for the same reason.
fn scene_catalog_references(
    scene: &VideoScene,
    catalog: Option<&VideoCatalog>,
    renderer: &dyn VideoRenderer,
    engine: VideoEngine,
) -> Vec<SceneCatalogReference> {
    let Some(catalog) = catalog else {
        return Vec::new();
    };
    scene
        .production
        .catalog_items
        .iter()
        .filter(|id| catalog.find(id).is_some())
        .filter_map(|id| {
            renderer
                .catalog_item_source(engine, id)
                .ok()
                .map(|source| SceneCatalogReference {
                    id: id.clone(),
                    source,
                })
        })
        .collect()
}

/// The one-based scene a timeline position falls in.
fn scene_at(plan: &VideoPlanDocument, at_seconds: f32) -> Option<usize> {
    plan.scenes()
        .iter()
        .position(|scene| {
            at_seconds >= scene.start_seconds
                && at_seconds < scene.start_seconds + scene.duration_seconds
        })
        .map(|index| index + 1)
}

/// The text that decides which assets one authored source references.
///
/// Every file counts, not just the entry composition: a diagram or image is just
/// as likely to be referenced from a scene sub-composition.
fn source_reference_text(source: &VideoSourceDocument) -> String {
    source
        .files()
        .values()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n")
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
    if video.engine == VideoEngine::Manim && video.audio == VideoAudioMode::On {
        return Err(SfumatoError::validation(
            "Manim renders silently; use --audio off or generate with --engine hyperframe",
        ));
    }
    if video.engine == VideoEngine::Hyperframe
        && video.audio == VideoAudioMode::On
        && !config.generation_tool_enabled(GenerationToolKind::AudioGen)
    {
        return Err(SfumatoError::validation(
            "--audio on needs the audio-gen tool; enable it with `sfumato tool enable audio-gen` or --tool audio-gen",
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
    narration: Option<VideoNarrationSummary>,
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
            narration,
            prompts,
            warnings,
        },
    })
}

/// Describes a narrated film for callers that never open its audio.
fn narration_summary(
    narrated: Option<&NarratedFilm>,
    profile: Option<&String>,
) -> Option<VideoNarrationSummary> {
    let narrated = narrated?;
    Some(VideoNarrationSummary {
        profile: profile.cloned().unwrap_or_default(),
        segments: narrated.track.segments.len(),
        spoken_seconds: narrated.track.total_seconds(),
        caption_groups: narrated.captions.len(),
    })
}

/// Ink share below which a frame carries nothing a viewer would call content.
///
/// Calibrated against a real 45s video: its scene-start frames measured 0.0000 to
/// 0.0071 while every mid-scene frame measured 0.05 or more, so this sits in the
/// gap rather than at a guessed round number.
const EMPTY_INK_RATIO: f32 = 0.02;

/// Distinct colours at or below which a frame is background plus almost nothing.
const SPARSE_COLOURS: u32 = 2;

/// Frames are matched to scene starts within this many seconds.
const FRAME_MATCH_EPSILON: f32 = 0.05;

/// Judges what the captured frames show against what the plan promised.
///
/// A scene that opens on an empty frame is the defect worth naming: the cut lands
/// on nothing, which reads as a stutter rather than as a transition. A mid-scene
/// frame is allowed to be sparse, because holding on one word is a real choice.
fn classify_frames(
    plan: &VideoPlanDocument,
    measurements: &[VideoFrameMeasurement],
) -> Vec<VideoFrameDefect> {
    let starts = plan
        .scenes()
        .iter()
        .enumerate()
        .map(|(index, scene)| (index + 1, scene.start_seconds))
        .collect::<Vec<_>>();
    measurements
        .iter()
        .filter_map(|measurement| {
            let empty = measurement.ink_ratio < EMPTY_INK_RATIO
                || measurement.distinct_colours <= SPARSE_COLOURS;
            if !empty {
                return None;
            }
            let scene = starts
                .iter()
                .find(|(_, start)| (start - measurement.at_seconds).abs() <= FRAME_MATCH_EPSILON)
                .map(|(position, _)| *position);
            // Off a scene boundary, only a frame with literally nothing on it is
            // a defect; a sparse held beat is legitimate.
            let kind = match scene {
                Some(_) => VideoFrameDefectKind::EmptySceneStart,
                None if measurement.ink_ratio == 0.0 => VideoFrameDefectKind::BlankFrame,
                None => return None,
            };
            Some(VideoFrameDefect {
                at_seconds: measurement.at_seconds,
                scene,
                kind,
                measurement: measurement.clone(),
            })
        })
        .collect()
}

/// One line per defect, for the warnings and the repair prompt.
fn describe_frame_defect(defect: &VideoFrameDefect) -> String {
    match defect.kind {
        VideoFrameDefectKind::EmptySceneStart => format!(
            "scene {} opens on an empty frame at {:.2}s: {:.2}% of pixels carry content across {} colours",
            defect.scene.unwrap_or(0),
            defect.at_seconds,
            defect.measurement.ink_ratio * 100.0,
            defect.measurement.distinct_colours
        ),
        VideoFrameDefectKind::BlankFrame => {
            format!("the frame at {:.2}s is blank", defect.at_seconds)
        }
    }
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
            "## {} ({:.2}s-{:.2}s)\n\nOn screen:\n{}\n\n",
            scene.id,
            scene.start_seconds,
            scene.start_seconds + scene.duration_seconds,
            scene.production.on_screen_copy.join("\n")
        ));
        if !scene.narration.trim().is_empty() {
            output.push_str(&format!("Spoken:\n{}\n\n", scene.narration.trim()));
        }
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
        "mp3" => (ArtifactKind::Audio, Some("audio/mpeg".into())),
        "wav" => (ArtifactKind::Audio, Some("audio/wav".into())),
        "opus" => (ArtifactKind::Audio, Some("audio/ogg".into())),
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
