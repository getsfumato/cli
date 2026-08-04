//! Standalone HTML page generation, review, inspection, and publication.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use serde::{Deserialize, Serialize};
pub use sfumato_domain::PageDocument;
use sfumato_domain::{ArtifactKind, ReviewableDocument, strip_json_fence};
use slug::slugify;

use crate::{
    artifacts::{
        ArtifactResourceKind, ArtifactStore, ResourceArtifactFile, ResourceArtifactManifest,
    },
    config::{Capability, EffectiveConfig, GenerationToolKind, ModelRole},
    errors::{
        ErrorClass, OperationStage, ResultContext as Context, SfumatoError, SfumatoResult as Result,
    },
    filesystem::WorkspaceFileSystem,
    generation::{
        GenerationRequest, GenerationToolSummary, PageGenerationOutput, PageInspectionIssue,
        PageIssueKind, PagePluginSelection, PageReviewSummary, ReviewStatus,
    },
    operation::{OperationContext, OperationEventKind},
    page_plugins::{PagePluginCatalog, PagePluginPackage},
    project_assets::{ProjectAssetCatalog, ProjectAssetReference},
    prompts::{PromptCatalog, PromptId, PromptProvenance, PromptRenderRequest, PromptVariables},
    providers::{
        GenerationStage, ImageGenerationProvider, ProviderFactory, SpeechGenerationProvider,
        TextGenerationEvent, TextGenerationRequest, ToolDefinition, VideoGenerationProvider,
    },
    python::PythonRuntime,
    renderers::{AssembledPage, PageAssembler, PageAssemblyRequest, PageInspector},
    repositories::ThemeRepository,
    sources::{SourceDocument, SourceReader},
    templates::GenerationTemplate,
    themes::ThemePackage,
    tools::{
        AudioToolConfig, ChartToolConfig, GenerationToolFactory, GenerationToolsRequest,
        ImageToolConfig, VideoToolConfig, chart_tool_gate_warning,
    },
};

use super::{
    DryRunImageProvider, DryRunSpeechProvider, DryRunVideoProvider,
    project_assets::{
        PrepareProjectAssetsRequest, prepare_project_assets, referenced_generated_assets,
        retain_referenced_generated_assets,
    },
};

/// Complete page-generation result returned to presentation frontends.
#[derive(Debug)]
pub struct GeneratePageResult {
    pub html_path: PathBuf,
    pub published_paths: Vec<PathBuf>,
    pub output: PageGenerationOutput,
    pub prompt_preview: Option<String>,
    pub tool_summaries: Vec<GenerationToolSummary>,
    pub warnings: Vec<String>,
}

pub(crate) struct GeneratePageOptions {
    pub operation: OperationContext,
    pub title: Option<String>,
    pub template: Option<GenerationTemplate>,
    pub plugins: Vec<String>,
    pub dry_run: bool,
    pub review: bool,
    pub event_sink: Option<Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
    pub prompt_catalog: Arc<dyn PromptCatalog>,
    pub artifact_store: Arc<dyn ArtifactStore>,
    pub provider_factory: Arc<dyn ProviderFactory>,
    pub source_reader: Arc<dyn SourceReader>,
    pub tool_factory: Arc<dyn GenerationToolFactory>,
    /// Managed Python environments backing the local charting tool.
    pub python_runtime: Arc<dyn PythonRuntime>,
    pub theme_repository: Arc<dyn ThemeRepository>,
    pub plugin_catalog: Arc<dyn PagePluginCatalog>,
    pub page_assembler: Arc<dyn PageAssembler>,
    pub page_inspector: Arc<dyn PageInspector>,
    pub workspace: Arc<dyn WorkspaceFileSystem>,
    pub project_asset_catalog: Arc<dyn ProjectAssetCatalog>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PageDraft {
    title: String,
    body_html: String,
    #[serde(default)]
    css: String,
    #[serde(default)]
    javascript: String,
}

#[derive(Clone, Debug, Default, Serialize)]
struct PagePromptContext {
    learning_style: String,
    project: String,
    project_root: String,
    theme_name: String,
    theme_colors: String,
    theme_fonts: String,
    instruction: String,
    project_instructions: String,
    title: String,
    title_provided: bool,
    image_generation_available: bool,
    video_generation_available: bool,
    source_bundle: String,
    plugins: Vec<PagePromptPlugin>,
    page_snapshot: String,
    draft_response: String,
    validation_error: String,
    issue_report: String,
    max_tool_rounds: usize,
    template_enabled: bool,
    template_name: String,
    template_source: String,
    reusable_assets: Vec<ProjectAssetReference>,
}

#[derive(Clone, Debug, Serialize)]
struct PagePromptPlugin {
    id: String,
    name: String,
    version: String,
    api_global: String,
    guidance: String,
}

struct PageContextInput<'a> {
    config: &'a EffectiveConfig,
    theme: &'a ThemePackage,
    instruction: &'a str,
    title: Option<&'a str>,
    project_instructions: &'a str,
    source_bundle: &'a str,
    image_generation_available: bool,
    video_generation_available: bool,
    plugins: &'a [PagePluginPackage],
    max_tool_rounds: usize,
    template: Option<&'a GenerationTemplate>,
    reusable_assets: &'a [ProjectAssetReference],
}

pub(crate) async fn generate_page(
    config: EffectiveConfig,
    request: GenerationRequest,
    options: GeneratePageOptions,
) -> Result<GeneratePageResult> {
    let GeneratePageOptions {
        operation,
        title: title_override,
        template,
        plugins: plugin_ids,
        dry_run,
        review,
        event_sink,
        prompt_catalog,
        artifact_store,
        provider_factory,
        source_reader,
        tool_factory,
        python_runtime,
        theme_repository,
        plugin_catalog,
        page_assembler,
        page_inspector,
        workspace,
        project_asset_catalog,
    } = options;
    operation.checkpoint(OperationStage::Resolve)?;
    let publish_root = config.publish_root()?;
    let theme = theme_repository.load(&config.theme)?;
    if theme.manifest.adapters.html.is_none() {
        return Err(SfumatoError::render(
            ErrorClass::Permanent,
            format!("Theme '{}' does not provide an HTML adapter", config.theme),
        ));
    }
    let plugins = plugin_catalog.resolve(&plugin_ids)?;
    let plugin_selections = plugin_selections(&plugins);

    let mut transaction = if dry_run {
        None
    } else {
        Some(artifact_store.begin(&config.project_name, ArtifactResourceKind::Pages)?)
    };
    let staging_root = transaction
        .as_ref()
        .map(|value| value.staging_root().to_path_buf())
        .unwrap_or_else(|| {
            artifact_store
                .project_root(&config.project_name)
                .unwrap_or_else(|_| PathBuf::from(".sfumato"))
                .join("resources/pages/dry-run")
        });
    let images_dir = staging_root.join("assets/images");
    let videos_dir = staging_root.join("assets/videos");
    let audio_dir = staging_root.join("assets/audio");
    operation.emit(
        OperationStage::ReadSources,
        OperationEventKind::Started,
        BTreeMap::new(),
    );
    let project_instructions = source_reader.project_instructions(&config.project_root)?;
    let documents = source_reader.collect(&request.sources)?;
    let source_bundle = build_source_bundle(&documents, 48_000);
    let compact_source_bundle = build_source_bundle(&documents, 12_000);
    operation.emit(
        OperationStage::ReadSources,
        OperationEventKind::Completed,
        BTreeMap::from([("documents".into(), documents.len().to_string())]),
    );

    let image_selection = config
        .generation_tool_enabled(GenerationToolKind::ImageGen)
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
        output_dir: &images_dir,
        reference_prefix: "assets/images",
        dry_run,
        operation: &operation,
    })
    .await?;
    let reusable_asset_references = prepared_assets.references();
    let reusable_asset_paths = prepared_assets.allowed_paths();
    let video_selection = config
        .generation_tool_enabled(GenerationToolKind::VideoGen)
        .then(|| config.resolve_model(Capability::Video))
        .transpose()?;
    let video_provider = video_selection
        .map(|(_, profile)| {
            if dry_run {
                Ok::<Option<Arc<dyn VideoGenerationProvider>>, SfumatoError>(None)
            } else {
                provider_factory
                    .video(&config, profile)
                    .map(Arc::from)
                    .map(Some)
            }
        })
        .transpose()?
        .flatten();
    let video_references = if video_selection.is_some() && !dry_run {
        prepared_assets.materialize_all(workspace.as_ref())?
    } else {
        Vec::new()
    };
    // A page can speak: the drafter embeds an <audio> element and, when it
    // wants captions, reads the word timings written beside the file.
    let audio_selection = config
        .generation_tool_enabled(GenerationToolKind::AudioGen)
        .then(|| config.resolve_model(Capability::Speech))
        .transpose()?;
    let audio_provider = audio_selection
        .map(|(_, profile)| {
            if dry_run {
                Ok::<Arc<dyn SpeechGenerationProvider>, SfumatoError>(Arc::new(
                    DryRunSpeechProvider,
                ))
            } else {
                provider_factory.speech(&config, profile).map(Arc::from)
            }
        })
        .transpose()?;
    let image = image_selection
        .map(|(name, _)| {
            let provider = image_provider
                .as_ref()
                .expect("image provider resolved above")
                .clone();
            Ok::<ImageToolConfig, SfumatoError>(ImageToolConfig {
                provider,
                profile_name: name.to_string(),
                output_dir: images_dir.clone(),
                reference_prefix: "assets/images".into(),
                theme: theme.clone(),
                project_instructions: project_instructions
                    .as_ref()
                    .map(|value| value.content.clone()),
            })
        })
        .transpose()?;
    let tool_set = tool_factory.create(GenerationToolsRequest {
        project_root: config.project_root.clone(),
        sources: request.sources.clone(),
        image,
        video: video_selection.map(|(name, profile)| VideoToolConfig {
            provider: video_provider.unwrap_or_else(|| Arc::new(DryRunVideoProvider)),
            profile_name: name.to_string(),
            output_dir: videos_dir,
            reference_prefix: "assets/videos".into(),
            theme: theme.clone(),
            project_instructions: project_instructions
                .as_ref()
                .map(|value| value.content.clone()),
            references: video_references,
            options: profile.options.video.clone(),
        }),
        audio: audio_selection.map(|(name, profile)| AudioToolConfig {
            provider: audio_provider.expect("speech provider resolved above"),
            profile_name: name.to_string(),
            output_dir: audio_dir,
            reference_prefix: "assets/audio".into(),
            options: profile.options.speech.clone(),
        }),
        chart: ChartToolConfig::enable(
            &config,
            python_runtime.clone(),
            images_dir.clone(),
            "assets/images",
            &theme,
            project_instructions
                .as_ref()
                .map(|value| value.content.clone()),
            false,
        ),
        prompt_catalog: prompt_catalog.clone(),
    })?;
    let tool_summaries = summarize_tools(&tool_set.definitions);
    let review_tools = tool_set
        .definitions
        .iter()
        .filter(|tool| {
            !matches!(
                tool.function.name.as_str(),
                "sfumato_image_gen" | "sfumato_video_gen"
            )
        })
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
    if let Some((name, _)) = video_selection {
        models.insert("video".into(), name.to_string());
    }
    let max_tool_rounds = draft_profile.options.tool_rounds();
    let base_context = page_context(PageContextInput {
        config: &config,
        theme: &theme,
        instruction: &request.instruction,
        title: title_override.as_deref(),
        project_instructions: project_instructions
            .as_ref()
            .map(|value| value.content.as_str())
            .unwrap_or_default(),
        source_bundle: &source_bundle,
        image_generation_available: image_selection.is_some(),
        video_generation_available: video_selection.is_some(),
        plugins: &plugins,
        max_tool_rounds,
        template: template.as_ref(),
        reusable_assets: &reusable_asset_references,
    });
    let mut draft_request = render_page_request(
        prompt_catalog.as_ref(),
        PromptId::PageDraftSystem,
        PromptId::PageDraftUser,
        &base_context,
    )?;
    draft_request.tools = tool_set.definitions.clone();
    draft_request.tool_executor = Some(tool_set.executor.clone());
    draft_request.event_sink = event_sink.clone();
    let mut compact_context = base_context.clone();
    compact_context.source_bundle = compact_source_bundle;
    compact_context.image_generation_available = false;
    compact_context.video_generation_available = false;
    let compact_request = render_page_request(
        prompt_catalog.as_ref(),
        PromptId::PageCompactDraftSystem,
        PromptId::PageCompactDraftUser,
        &compact_context,
    )?;
    let mut prompts = prepared_assets.prompts.clone();
    prompts.extend(draft_request.prompt_provenance.clone());
    let mut review_summary = PageReviewSummary::new(review);
    let project_instructions_path = project_instructions
        .as_ref()
        .map(|value| value.path.clone());

    if dry_run {
        return Ok(GeneratePageResult {
            html_path: staging_root.join("index.html"),
            published_paths: Vec::new(),
            output: PageGenerationOutput {
                project: config.project_name,
                title: title_override.unwrap_or_else(|| "model-generated-title".into()),
                html_path: staging_root.join("index.html"),
                project_instructions: project_instructions_path,
                models,
                plugins: plugin_selections,
                template: template.as_ref().map(|value| value.manifest.name.clone()),
                project_assets: reusable_asset_references.clone(),
                runtimes: Vec::new(),
                tools: tool_summaries.clone(),
                artifacts: Vec::new(),
                published_artifacts: Vec::new(),
                review: review_summary,
                prompts,
            },
            prompt_preview: Some(draft_request.user_prompt),
            tool_summaries,
            warnings: prepared_assets.warnings.clone(),
        });
    }

    emit_stage(&event_sink, GenerationStage::PageDraft, Some(draft_name));
    let provider = provider_factory.text(&config, draft_profile)?;
    let response = match provider
        .generate_text(draft_request, &operation, OperationStage::Draft)
        .await
    {
        Ok(response) => response,
        Err(error)
            if error.class == ErrorClass::ContextLimit
                || error.class == ErrorClass::InvalidOutput =>
        {
            prompts.extend(compact_request.prompt_provenance.clone());
            provider
                .generate_text(compact_request, &operation, OperationStage::Draft)
                .await
                .map_err(|retry| {
                    retry.context(format!("Page draft failed after compact retry: {error}"))
                })?
        }
        Err(error) => return Err(error),
    };
    let generated_tool_assets = tool_set.generated_artifacts()?;
    let generated_image_assets = generated_tool_assets
        .iter()
        .filter(|path| path.extension().and_then(|value| value.to_str()) != Some("mp4"))
        .cloned()
        .collect::<Vec<_>>();
    let generated_video_assets = generated_tool_assets
        .iter()
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("mp4"))
        .cloned()
        .collect::<Vec<_>>();
    let mut generated_assets = reusable_asset_paths.clone();
    generated_assets.extend(generated_tool_assets.clone());
    prompts.extend(tool_set.generated_prompts()?);
    let mut page =
        parse_page_document(&response.text, title_override.as_deref(), template.as_ref()).and_then(
            |page| {
                validate_page(&page_assembler, &page, &theme, &plugins, &generated_assets)
                    .map(|_| page)
            },
        );
    if let Err(validation_error) = &page {
        emit_stage(&event_sink, GenerationStage::PageRepair, Some(draft_name));
        let mut repair_context = base_context.clone();
        repair_context.draft_response = response.text.clone();
        repair_context.validation_error = validation_error.to_string();
        let mut repair_request = render_page_request(
            prompt_catalog.as_ref(),
            PromptId::PageValidationRepairSystem,
            PromptId::PageValidationRepairUser,
            &repair_context,
        )?;
        repair_request.event_sink = event_sink.clone();
        prompts.extend(repair_request.prompt_provenance.clone());
        let repaired = provider
            .generate_text(repair_request, &operation, OperationStage::Repair)
            .await?;
        page = parse_page_document(&repaired.text, title_override.as_deref(), template.as_ref())
            .and_then(|page| {
                validate_page(&page_assembler, &page, &theme, &plugins, &generated_assets)
                    .map(|_| page)
            });
    }
    let mut page =
        page.map_err(|error| error.context("Generated page remained invalid after repair"))?;
    let mut warnings = prepared_assets.warnings.clone();
    // A charting tool the project enabled but the Python gate withheld leaves no
    // other trace: the resource simply comes back without charts.
    warnings.extend(chart_tool_gate_warning(&config, false));

    if let Some((reviewer_name, reviewer_profile)) = reviewer {
        emit_stage(
            &event_sink,
            GenerationStage::PageReview,
            Some(reviewer_name),
        );
        let snapshot = page.snapshot().map_err(review_error)?;
        let mut context = base_context.clone();
        context.page_snapshot = serde_json::to_string_pretty(&snapshot)
            .context("Could not serialize page review snapshot")?;
        let mut review_request = render_page_request(
            prompt_catalog.as_ref(),
            PromptId::PageReviewSystem,
            PromptId::PageReviewUser,
            &context,
        )?;
        review_request.tools = review_tools;
        review_request.tool_executor = Some(tool_set.executor.clone());
        review_request.event_sink = event_sink.clone();
        prompts.extend(review_request.prompt_provenance.clone());
        let reviewer_provider = provider_factory.text(&config, reviewer_profile)?;
        let mut candidate = page.clone();
        match reviewer_provider
            .generate_text(review_request, &operation, OperationStage::Review)
            .await
            .and_then(|response| {
                crate::review::parse_json_patch(&response.text).map_err(review_error)
            })
            .and_then(|patch| candidate.apply_patch(&patch).map_err(review_error))
            .and_then(|_| {
                validate_page(
                    &page_assembler,
                    &candidate,
                    &theme,
                    &plugins,
                    &generated_assets,
                )
            }) {
            Ok(_) => {
                page = candidate;
                review_summary.semantic_review = ReviewStatus::Completed;
            }
            Err(error) => {
                review_summary.semantic_review = ReviewStatus::Failed;
                warnings.push(format!(
                    "Page semantic review failed; using the validated draft: {error}"
                ));
            }
        }
    }

    let initial_asset_text = page_asset_text(&page);
    let (initial_project_paths, _) =
        prepared_assets.materialize_referenced(&initial_asset_text, workspace.as_ref())?;
    let mut initial_generated_paths = referenced_generated_assets(
        &generated_image_assets,
        &initial_asset_text,
        "assets/images",
    );
    initial_generated_paths.extend(referenced_generated_assets(
        &generated_video_assets,
        &initial_asset_text,
        "assets/videos",
    ));
    let mut initial_inspection_assets = initial_project_paths;
    initial_inspection_assets.extend(initial_generated_paths);
    let assembled = validate_page(&page_assembler, &page, &theme, &plugins, &generated_assets)?;
    let mut html = assembled.html;
    let mut runtimes = assembled.runtimes;
    let inspection_workspace = workspace.temporary_directory("sfumato-page-inspection")?;
    let inspection_root = inspection_workspace.path().to_path_buf();
    let inspection_path = inspection_root.join("index.html");
    stage_inspection_assets(
        workspace.as_ref(),
        &inspection_root,
        &initial_inspection_assets,
    )?;
    let inspection_html = assemble_page(
        &page_assembler,
        &page,
        &theme,
        &plugins,
        &generated_assets,
        true,
    )?;
    workspace.write(&inspection_path, inspection_html.html.as_bytes())?;
    emit_stage(&event_sink, GenerationStage::LayoutCheck, None);
    let initial_issues = match page_inspector
        .inspect(
            &inspection_path,
            config.marp.browser_path.as_deref(),
            &operation,
        )
        .await
    {
        Ok(issues) => {
            review_summary.browser_check = ReviewStatus::Completed;
            issues
        }
        Err(error) if error.class == ErrorClass::Unavailable => {
            review_summary.browser_check = ReviewStatus::Skipped;
            warnings.push(format!("Page browser inspection skipped: {error}"));
            Vec::new()
        }
        Err(error) => return Err(error),
    };

    let mut final_issues = initial_issues.clone();
    if !initial_issues.is_empty() && review {
        let (reviewer_name, reviewer_profile) =
            reviewer.expect("reviewer resolved when review is enabled");
        emit_stage(
            &event_sink,
            GenerationStage::PageRepair,
            Some(reviewer_name),
        );
        let mut context = base_context.clone();
        context.page_snapshot =
            serde_json::to_string_pretty(&page.snapshot().map_err(review_error)?)
                .context("Could not serialize page repair snapshot")?;
        context.issue_report = serde_json::to_string_pretty(&initial_issues)
            .context("Could not serialize page browser issues")?;
        let mut repair_request = render_page_request(
            prompt_catalog.as_ref(),
            PromptId::PageBrowserRepairSystem,
            PromptId::PageBrowserRepairUser,
            &context,
        )?;
        repair_request.event_sink = event_sink.clone();
        prompts.extend(repair_request.prompt_provenance.clone());
        let reviewer_provider = provider_factory.text(&config, reviewer_profile)?;
        let repair_result = async {
            let response = reviewer_provider
                .generate_text(repair_request, &operation, OperationStage::Repair)
                .await?;
            let patch = crate::review::parse_json_patch(&response.text).map_err(review_error)?;
            let mut candidate = page.clone();
            candidate
                .apply_browser_repair_patch(&patch)
                .map_err(review_error)?;
            let candidate_asset_text = page_asset_text(&candidate);
            let (candidate_project_paths, _) = prepared_assets
                .materialize_referenced(&candidate_asset_text, workspace.as_ref())?;
            let mut candidate_generated_paths = referenced_generated_assets(
                &generated_image_assets,
                &candidate_asset_text,
                "assets/images",
            );
            candidate_generated_paths.extend(referenced_generated_assets(
                &generated_video_assets,
                &candidate_asset_text,
                "assets/videos",
            ));
            let mut candidate_inspection_assets = candidate_project_paths;
            candidate_inspection_assets.extend(candidate_generated_paths);
            stage_inspection_assets(
                workspace.as_ref(),
                &inspection_root,
                &candidate_inspection_assets,
            )?;
            let candidate_assembly = validate_page(
                &page_assembler,
                &candidate,
                &theme,
                &plugins,
                &generated_assets,
            )?;
            let candidate_inspection_html = assemble_page(
                &page_assembler,
                &candidate,
                &theme,
                &plugins,
                &generated_assets,
                true,
            )?;
            workspace.write(&inspection_path, candidate_inspection_html.html.as_bytes())?;
            let issues = page_inspector
                .inspect(
                    &inspection_path,
                    config.marp.browser_path.as_deref(),
                    &operation,
                )
                .await?;
            Ok::<_, SfumatoError>((candidate, candidate_assembly, issues))
        }
        .await;
        match repair_result {
            Ok((candidate, candidate_assembly, issues))
                if issue_score(&issues) < issue_score(&initial_issues) =>
            {
                page = candidate;
                html = candidate_assembly.html;
                runtimes = candidate_assembly.runtimes;
                final_issues = issues;
                review_summary.repair = ReviewStatus::Accepted;
            }
            Ok(_) => {
                review_summary.repair = ReviewStatus::Rejected;
                warnings.push("Page browser repair did not improve validation metrics; retained the reviewed page.".into());
            }
            Err(error) => {
                review_summary.repair = ReviewStatus::Failed;
                warnings.push(format!(
                    "Page browser repair failed; retained the reviewed page: {error}"
                ));
            }
        }
    }
    review_summary.remaining_issues = final_issues.clone();
    if final_issues.iter().any(fatal_issue) {
        return Err(SfumatoError::render(
            ErrorClass::InvalidOutput,
            "Generated page still has runtime errors or missing required content after repair",
        )
        .with_detail(
            "issues",
            serde_json::to_string(&final_issues).unwrap_or_default(),
        ));
    }
    if !final_issues.is_empty() {
        warnings.push(format!(
            "Page layout still has {} responsive overflow issue(s).",
            final_issues.len()
        ));
    }

    emit_stage(&event_sink, GenerationStage::PageRendering, None);
    let final_asset_text = page_asset_text(&page);
    let (used_project_paths, used_project_assets) =
        prepared_assets.materialize_referenced(&final_asset_text, workspace.as_ref())?;
    for path in reusable_asset_paths
        .iter()
        .filter(|path| !used_project_paths.contains(path))
    {
        workspace.remove_file(path)?;
    }
    let _used_generated_images = retain_referenced_generated_assets(
        generated_image_assets,
        &final_asset_text,
        "assets/images",
        workspace.as_ref(),
    )?;
    let _used_generated_videos = retain_referenced_generated_assets(
        generated_video_assets,
        &final_asset_text,
        "assets/videos",
        workspace.as_ref(),
    )?;
    let title = page.title().to_string();
    let slug = slugify(&title);
    if slug.is_empty() {
        return Err(SfumatoError::validation(
            "Page title cannot produce an artifact name",
        ));
    }
    let html_path = staging_root.join("index.html");
    workspace.write(&html_path, html.as_bytes())?;
    let revision_id = transaction
        .as_ref()
        .context("Page artifact transaction is unavailable")?
        .revision_id()
        .to_string();
    let index_path = staging_root.join("index.md");
    let index_markdown = obsidian_page_index(&title, &config.project_name, &revision_id);
    workspace.write(&index_path, index_markdown.as_bytes())?;
    let staged_files = workspace.list_files(&staging_root, &["manifest.json"])?;
    let files = staged_files
        .iter()
        .map(|path| page_artifact_file(&staging_root, path))
        .collect::<Result<Vec<_>>>()?;
    let transaction = transaction
        .take()
        .context("Page artifact transaction is unavailable")?;
    deduplicate_prompts(&mut prompts);
    let manifest = ResourceArtifactManifest {
        schema_version: 1,
        job_id: transaction.job_id().clone(),
        revision_id: transaction.revision_id().clone(),
        parent_revision: transaction.parent_revision().cloned(),
        project: config.project_name.clone(),
        resource_kind: ArtifactResourceKind::Pages,
        resource_id: slug.clone(),
        title: title.clone(),
        files,
        models: models.clone(),
        prompts: prompts.clone(),
        plugins: plugin_selections.clone(),
        runtimes: runtimes.clone(),
        warnings: warnings.clone(),
    };
    let staging = transaction.staging_root().to_path_buf();
    let committed = transaction.commit(manifest)?;
    let committed_html = committed.root.join(
        html_path
            .strip_prefix(&staging)
            .context("Page HTML escaped its transaction")?,
    );
    let mut artifacts = workspace.list_files(&committed.root, &[])?;
    artifacts.sort();
    let published_paths = publish_page(
        workspace.as_ref(),
        publish_root.as_deref(),
        &committed.root,
        &slug,
        &mut warnings,
    )?;
    Ok(GeneratePageResult {
        html_path: committed_html.clone(),
        published_paths: published_paths.clone(),
        output: PageGenerationOutput {
            project: config.project_name,
            title,
            html_path: committed_html.clone(),
            project_instructions: project_instructions_path,
            models,
            plugins: plugin_selections,
            template: template.as_ref().map(|value| value.manifest.name.clone()),
            project_assets: used_project_assets,
            runtimes,
            tools: tool_summaries.clone(),
            artifacts,
            published_artifacts: published_paths.clone(),
            review: review_summary,
            prompts,
        },
        prompt_preview: None,
        tool_summaries,
        warnings,
    })
}

fn page_asset_text(page: &PageDocument) -> String {
    format!(
        "{}\n{}\n{}",
        page.body_html(),
        page.css(),
        page.javascript()
    )
}

fn page_context(input: PageContextInput<'_>) -> PagePromptContext {
    let PageContextInput {
        config,
        theme,
        instruction,
        title,
        project_instructions,
        source_bundle,
        image_generation_available,
        video_generation_available,
        plugins,
        max_tool_rounds,
        template,
        reusable_assets,
    } = input;
    PagePromptContext {
        learning_style: if config.user.learning_style.is_empty() {
            "not specified".into()
        } else {
            config.user.learning_style.join(", ")
        },
        project: config.project_name.clone(),
        project_root: config.project_root.display().to_string(),
        theme_name: theme.manifest.name.clone(),
        theme_colors: format_tokens(&theme.manifest.tokens.colors),
        theme_fonts: format_tokens(&theme.manifest.tokens.fonts),
        instruction: instruction.into(),
        project_instructions: project_instructions.into(),
        title: title.unwrap_or_default().into(),
        title_provided: title.is_some(),
        image_generation_available,
        video_generation_available,
        source_bundle: source_bundle.into(),
        plugins: plugins
            .iter()
            .map(|plugin| PagePromptPlugin {
                id: plugin.summary.id.clone(),
                name: plugin.summary.name.clone(),
                version: plugin.summary.version.clone(),
                api_global: plugin.summary.api_global.clone(),
                guidance: plugin.guidance.clone(),
            })
            .collect(),
        max_tool_rounds,
        template_enabled: template.is_some(),
        template_name: template
            .map(|template| template.manifest.name.clone())
            .unwrap_or_default(),
        template_source: template
            .map(|template| template.source.clone())
            .unwrap_or_default(),
        reusable_assets: reusable_assets.to_vec(),
        ..Default::default()
    }
}

fn render_page_request(
    catalog: &dyn PromptCatalog,
    system_id: PromptId,
    user_id: PromptId,
    context: &PagePromptContext,
) -> Result<TextGenerationRequest> {
    let variables = PromptVariables::from_serializable(context)?;
    let system = catalog.render(PromptRenderRequest {
        id: system_id,
        variables: variables.clone(),
    })?;
    let user = catalog.render(PromptRenderRequest {
        id: user_id,
        variables: variables.clone(),
    })?;
    let exhausted = catalog.render(PromptRenderRequest {
        id: PromptId::PageToolExhaustedUser,
        variables,
    })?;
    let mut request = TextGenerationRequest::new(system.text, user.text);
    request.max_tool_rounds = context.max_tool_rounds;
    request.tool_exhausted_prompt = Some(exhausted.text);
    request.prompt_provenance = vec![system.provenance, user.provenance, exhausted.provenance];
    Ok(request)
}

fn parse_page_document(
    response: &str,
    title_override: Option<&str>,
    template: Option<&GenerationTemplate>,
) -> Result<PageDocument> {
    let raw = strip_json_fence(response.trim());
    let draft: PageDraft = serde_json::from_str(raw).map_err(|error| {
        SfumatoError::provider(
            ErrorClass::InvalidOutput,
            format!("Page response must be strict JSON: {error}"),
        )
    })?;
    let body_html = template
        .map(|template| template.compose(&draft.body_html))
        .transpose()?
        .unwrap_or(draft.body_html);
    PageDocument::new(
        title_override.unwrap_or(&draft.title),
        body_html,
        draft.css,
        draft.javascript,
    )
    .map_err(review_error)
}

fn validate_page(
    assembler: &Arc<dyn PageAssembler>,
    page: &PageDocument,
    theme: &ThemePackage,
    plugins: &[PagePluginPackage],
    assets: &[PathBuf],
) -> Result<AssembledPage> {
    assemble_page(assembler, page, theme, plugins, assets, false)
}

fn assemble_page(
    assembler: &Arc<dyn PageAssembler>,
    page: &PageDocument,
    theme: &ThemePackage,
    plugins: &[PagePluginPackage],
    assets: &[PathBuf],
    inspection: bool,
) -> Result<AssembledPage> {
    assembler.assemble(PageAssemblyRequest {
        document: page,
        theme,
        plugins,
        allowed_assets: assets,
        inspection,
    })
}

fn plugin_selections(plugins: &[PagePluginPackage]) -> Vec<PagePluginSelection> {
    plugins
        .iter()
        .map(|plugin| PagePluginSelection {
            id: plugin.summary.id.clone(),
            name: plugin.summary.name.clone(),
            version: plugin.summary.version.clone(),
            runtime_hash: plugin.summary.runtime_hash.clone(),
        })
        .collect()
}

fn stage_inspection_assets(
    workspace: &dyn WorkspaceFileSystem,
    inspection_root: &Path,
    assets: &[PathBuf],
) -> Result<()> {
    for asset in assets {
        let filename = asset
            .file_name()
            .context("Generated page asset does not have a filename")?;
        let kind = if asset.extension().and_then(|value| value.to_str()) == Some("mp4") {
            "videos"
        } else {
            "images"
        };
        workspace.copy_file(
            asset,
            &inspection_root.join("assets").join(kind).join(filename),
        )?;
    }
    Ok(())
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
        if output.chars().count() >= limit {
            break;
        }
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

fn issue_score(issues: &[PageInspectionIssue]) -> u64 {
    issues
        .iter()
        .map(|issue| match issue.kind {
            PageIssueKind::RuntimeError | PageIssueKind::RejectedPromise => 100_000,
            PageIssueKind::MissingImage
            | PageIssueKind::MissingVideo
            | PageIssueKind::BlankContent
            | PageIssueKind::UnrenderedMath => 50_000,
            PageIssueKind::HorizontalOverflow => u64::from(issue.overflow_px.max(1)),
        })
        .sum()
}

fn fatal_issue(issue: &PageInspectionIssue) -> bool {
    matches!(
        issue.kind,
        PageIssueKind::RuntimeError
            | PageIssueKind::RejectedPromise
            | PageIssueKind::MissingImage
            | PageIssueKind::MissingVideo
            | PageIssueKind::BlankContent
            | PageIssueKind::UnrenderedMath
    )
}

fn page_artifact_file(root: &Path, path: &Path) -> Result<ResourceArtifactFile> {
    let relative = path
        .strip_prefix(root)
        .context("Page artifact escaped its transaction")?
        .to_path_buf();
    let (kind, media_type) = match path.extension().and_then(|value| value.to_str()) {
        Some("html") => (ArtifactKind::Html, Some("text/html")),
        Some("md") => (ArtifactKind::Markdown, Some("text/markdown")),
        Some("png") => (ArtifactKind::Image, Some("image/png")),
        Some("jpg" | "jpeg") => (ArtifactKind::Image, Some("image/jpeg")),
        Some("webp") => (ArtifactKind::Image, Some("image/webp")),
        _ => (ArtifactKind::Data, None),
    };
    Ok(ResourceArtifactFile {
        path: relative,
        kind,
        media_type: media_type.map(str::to_owned),
    })
}

fn publish_page(
    workspace: &dyn WorkspaceFileSystem,
    publish_root: Option<&Path>,
    committed_root: &Path,
    slug: &str,
    warnings: &mut Vec<String>,
) -> Result<Vec<PathBuf>> {
    let Some(root) = publish_root else {
        return Ok(Vec::new());
    };
    let destination = page_publish_destination(root, slug);
    let temp = workspace.temporary_directory("sfumato-page-publish")?;
    let payload = temp.path().join(slug);
    workspace.copy_tree(committed_root, &payload, &["manifest.json"])?;
    match workspace.publish_tree_atomic(&payload, &destination) {
        Ok(path) => {
            for stale_file in [root.join(format!("{slug}.html"))] {
                if let Err(error) = workspace.remove_file(&stale_file) {
                    warnings.push(format!("Could not remove stale page output: {error}"));
                }
            }
            let stale_tree = root.join(slug);
            if stale_tree != destination
                && let Err(error) = workspace.remove_tree(&stale_tree)
            {
                warnings.push(format!("Could not remove stale page output: {error}"));
            }
            let mut files = workspace.list_files(&path, &[])?;
            files.sort();
            Ok(files)
        }
        Err(error) => {
            warnings.push(format!(
                "Committed the page, but could not publish it: {error}"
            ));
            Ok(Vec::new())
        }
    }
}

fn page_publish_destination(root: &Path, slug: &str) -> PathBuf {
    root.join("_sfumato").join("pages").join(slug)
}

fn obsidian_page_index(title: &str, project: &str, revision: &str) -> String {
    let heading = title.replace(['\r', '\n'], " ");
    let yaml_title = serde_json::to_string(title).unwrap_or_else(|_| "\"Sfumato page\"".into());
    let yaml_project = serde_json::to_string(project).unwrap_or_else(|_| "\"unknown\"".into());
    let yaml_revision = serde_json::to_string(revision).unwrap_or_else(|_| "\"unknown\"".into());
    format!(
        "---\nsfumato: generated\nresource: page\ntitle: {yaml_title}\nproject: {yaml_project}\nrevision: {yaml_revision}\n---\n\n# {heading}\n\n> [!info] Generated by Sfumato\n> This resource is managed automatically. Edit its source through Sfumato rather than changing generated files directly.\n\n[Open interactive page](./index.html)\n"
    )
}

fn deduplicate_prompts(prompts: &mut Vec<PromptProvenance>) {
    let mut unique = Vec::new();
    for prompt in prompts.drain(..) {
        if !unique.contains(&prompt) {
            unique.push(prompt);
        }
    }
    *prompts = unique;
}

fn review_error(error: sfumato_domain::ReviewError) -> SfumatoError {
    SfumatoError::provider(ErrorClass::InvalidOutput, error)
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../../tests/unit/resources_pages.rs"]
mod tests;
