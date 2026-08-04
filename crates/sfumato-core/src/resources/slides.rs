use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use serde::Serialize;
use sfumato_domain::ArtifactKind;
use sha2::{Digest, Sha256};
use slug::slugify;

use crate::resources::excerpt;
use crate::sfumato_bail as bail;
use crate::{
    artifacts::{
        ArtifactResourceKind, ArtifactStore, ResourceArtifactFile, ResourceArtifactManifest,
    },
    config::{Capability, EffectiveConfig, ModelRole},
    errors::{
        ErrorClass, ErrorCode, OperationStage, ResultContext as Context, SfumatoError,
        SfumatoResult as Result,
    },
    filesystem::WorkspaceFileSystem,
    generation::{
        GenerationOutput, GenerationRequest, GenerationToolSummary, ReviewStatus, SlideLayoutIssue,
        SlideReviewSummary,
    },
    operation::{OperationContext, OperationEventKind},
    project_assets::{ProjectAssetCatalog, ProjectAssetReference},
    prompts::{
        PromptCatalog, PromptId, PromptPair, PromptProvenance, PromptRenderRequest, PromptVariables,
    },
    providers::{
        GenerationStage, ImageGenerationProvider, ProviderFactory, TextGenerationEvent,
        TextGenerationProvider, TextGenerationRequest, TextGenerationResponse, ToolDefinition,
    },
    python::PythonRuntime,
    renderers::{DiagramRenderer, MermaidThemeConfig, SlideRenderer},
    repositories::ThemeRepository,
    review::{ReviewSnapshot, decks::SlideDeckDocument, parse_json_patch},
    sources::SourceReader,
    templates::GenerationTemplate,
    themes::{ThemePackage, ThemeTokens},
    tools::{
        ChartToolConfig, GenerationToolFactory, GenerationToolsRequest, ImageToolConfig,
        chart_tool_gate_warning,
    },
};

pub(crate) mod document;
mod edit;
mod layout;
pub(crate) mod mermaid;
mod prompting;
mod publishing;
mod source_bundle;

use document::*;
pub(crate) use edit::{EditSlidesOptions, edit_slides};
pub use edit::{EditSlidesRequest, EditSlidesResult};
use layout::LayoutAssessment;
#[cfg(test)]
use layout::layout_score;
use mermaid::{
    MermaidRenderRequest, extract_mermaid_blocks, mermaid_image_markdown, render_mermaid_diagrams,
};
#[cfg(test)]
use mermaid::{mermaid_theme_config, normalize_mermaid_source};
use prompting::*;
use publishing::publish_slides;
use source_bundle::{build_compact_source_bundle, build_source_bundle};

use super::{
    DryRunImageProvider,
    project_assets::{
        PrepareProjectAssetsRequest, prepare_project_assets, retain_referenced_generated_assets,
        stage_referenced_generated_assets,
    },
};

const GENERATED_IMAGE_MARP_HEIGHT: &str = "420px";
const MAX_SOURCE_BUNDLE_CHARS: usize = 48_000;

pub(crate) struct GenerateSlidesOptions {
    pub operation: OperationContext,
    pub title: Option<String>,
    pub template: Option<GenerationTemplate>,
    pub dry_run: bool,
    pub review: bool,
    pub event_sink: Option<std::sync::Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
    pub prompt_catalog: Arc<dyn PromptCatalog>,
    pub artifact_store: Arc<dyn ArtifactStore>,
    pub provider_factory: Arc<dyn ProviderFactory>,
    pub diagram_renderer: Arc<dyn DiagramRenderer>,
    pub slide_renderer: Arc<dyn SlideRenderer>,
    pub source_reader: Arc<dyn SourceReader>,
    pub tool_factory: Arc<dyn GenerationToolFactory>,
    /// Managed Python environments backing the local charting tool.
    pub python_runtime: Arc<dyn PythonRuntime>,
    pub theme_repository: Arc<dyn ThemeRepository>,
    pub workspace: Arc<dyn WorkspaceFileSystem>,
    pub project_asset_catalog: Arc<dyn ProjectAssetCatalog>,
}

#[derive(Debug)]
pub struct GenerateSlidesResult {
    pub markdown_path: PathBuf,
    pub pdf_path: Option<PathBuf>,
    pub published_pdf_path: Option<PathBuf>,
    pub output: GenerationOutput,
    pub prompt_preview: Option<String>,
    pub tool_summaries: Vec<GenerationToolSummary>,
    pub warnings: Vec<String>,
}

pub(crate) async fn generate_slides(
    config: EffectiveConfig,
    request: GenerationRequest,
    options: GenerateSlidesOptions,
) -> Result<GenerateSlidesResult> {
    let GenerateSlidesOptions {
        operation,
        title: title_override,
        template,
        dry_run,
        review,
        event_sink,
        prompt_catalog,
        artifact_store,
        provider_factory,
        diagram_renderer,
        slide_renderer,
        source_reader,
        tool_factory,
        python_runtime,
        theme_repository,
        workspace,
        project_asset_catalog,
    } = options;
    operation.checkpoint(OperationStage::Resolve)?;
    let publish_root = config.publish_root()?;
    let mut artifact_transaction = if dry_run {
        None
    } else {
        Some(artifact_store.begin(&config.project_name, ArtifactResourceKind::Slides)?)
    };
    let slides_dir = artifact_transaction
        .as_ref()
        .map(|transaction| transaction.staging_root().to_path_buf())
        .unwrap_or_else(|| {
            artifact_store
                .project_root(&config.project_name)
                .unwrap_or_else(|_| PathBuf::from(".sfumato"))
                .join("resources")
                .join("slides")
                .join("dry-run")
        });
    let artifact_root = slides_dir.clone();
    let title_override = title_override
        .map(|title| validate_title(&title))
        .transpose()?;
    let theme_css_path = slides_dir
        .join("themes")
        .join(format!("{}.css", config.theme));
    let diagrams_dir = slides_dir.join("diagrams");
    let images_dir = slides_dir.join("images");

    ensure_inside(&artifact_root, &theme_css_path)?;
    ensure_inside(&artifact_root, &diagrams_dir)?;
    ensure_inside(&artifact_root, &images_dir)?;

    operation.emit(
        OperationStage::ReadSources,
        OperationEventKind::Started,
        BTreeMap::new(),
    );
    operation.checkpoint(OperationStage::ReadSources)?;
    let project_instructions = source_reader.project_instructions(&config.project_root)?;
    let project_instructions_prompt = project_instructions
        .as_ref()
        .map(|instructions| instructions.content.clone())
        .unwrap_or_default();
    let project_instructions_path = project_instructions
        .as_ref()
        .map(|instructions| instructions.path.clone());
    let theme = theme_repository.load(&config.theme)?;
    let documents = source_reader.collect(&request.sources)?;
    operation.emit(
        OperationStage::ReadSources,
        OperationEventKind::Completed,
        BTreeMap::from([("documents".to_string(), documents.len().to_string())]),
    );
    let image_selection = config
        .generation_tool_enabled(crate::config::GenerationToolKind::ImageGen)
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
        project_instructions: &project_instructions_prompt,
        output_dir: &images_dir,
        reference_prefix: "images",
        dry_run,
        operation: &operation,
    })
    .await?;
    let reusable_asset_references = prepared_assets.references();
    let reusable_asset_paths = prepared_assets.allowed_paths();
    let image_tool = image_selection
        .map(|(profile_name, _)| {
            let provider = image_provider
                .as_ref()
                .expect("image provider resolved above")
                .clone();
            Ok::<ImageToolConfig, SfumatoError>(ImageToolConfig {
                provider,
                profile_name: profile_name.to_string(),
                output_dir: images_dir.clone(),
                reference_prefix: "images".into(),
                theme: theme.clone(),
                project_instructions: project_instructions
                    .as_ref()
                    .map(|instructions| instructions.content.clone()),
            })
        })
        .transpose()?;
    let tool_set = tool_factory.create(GenerationToolsRequest {
        project_root: config.project_root.clone(),
        sources: request.sources.clone(),
        image: image_tool,
        video: None,
        // Neither a deck nor a printable document has a timeline to hang audio
        // on, so speech is not offered here.
        audio: None,
        chart: ChartToolConfig::enable(
            &config,
            python_runtime.clone(),
            images_dir.clone(),
            "images",
            &theme,
            project_instructions
                .as_ref()
                .map(|value| value.content.clone()),
            false,
        ),
        prompt_catalog: prompt_catalog.clone(),
    })?;
    let review_tool_definitions = tool_set
        .definitions
        .iter()
        .filter(|tool| tool.function.name != "sfumato_image_gen")
        .cloned()
        .collect::<Vec<_>>();
    let tool_summaries = summarize_tools(&tool_set.definitions);
    let source_bundle = build_source_bundle(&documents);
    let compact_source_bundle = build_compact_source_bundle(&documents, 12_000);
    let (draft_profile_name, draft_profile) = config.resolve_model(Capability::Text)?;
    let draft_tool_rounds = model_tool_rounds(draft_profile);
    operation.checkpoint(OperationStage::RenderPrompt)?;
    operation.emit(
        OperationStage::RenderPrompt,
        OperationEventKind::Started,
        BTreeMap::new(),
    );
    let mut provider_request = build_generation_request(DraftPromptRequestContext {
        catalog: prompt_catalog.as_ref(),
        config: &config,
        theme: &theme,
        instruction: &request.instruction,
        title: title_override.as_deref(),
        source_bundle: &source_bundle,
        image_generation_available: image_selection.is_some(),
        project_instructions: &project_instructions_prompt,
        tools: &tool_summaries,
        max_tool_rounds: draft_tool_rounds,
        event_sink: None,
        template: template.as_ref(),
        reusable_assets: &reusable_asset_references,
    })?;
    provider_request.tools = tool_set.definitions.clone();
    provider_request.tool_executor = Some(tool_set.executor.clone());
    provider_request.event_sink = event_sink.clone();
    provider_request.max_tool_rounds = draft_tool_rounds;
    operation.emit(
        OperationStage::RenderPrompt,
        OperationEventKind::Completed,
        BTreeMap::new(),
    );
    let mut used_prompts = prepared_assets.prompts.clone();
    used_prompts.extend(provider_request.prompt_provenance.clone());
    let reviewer_selection = review
        .then(|| config.resolve_model_role(ModelRole::Reviewer))
        .transpose()?;
    let mut selected_models =
        BTreeMap::from([("text".to_string(), draft_profile_name.to_string())]);
    if let Some((reviewer_name, _)) = reviewer_selection {
        selected_models.insert("reviewer".to_string(), reviewer_name.to_string());
    }
    if let Some((image_profile_name, _)) = image_selection {
        selected_models.insert("image".to_string(), image_profile_name.to_string());
    }
    let mut review_summary = if review {
        SlideReviewSummary::enabled()
    } else {
        SlideReviewSummary::disabled()
    };
    let mut warnings = prepared_assets.warnings.clone();
    // A charting tool the project enabled but the Python gate withheld leaves no
    // other trace: the resource simply comes back without charts.
    warnings.extend(chart_tool_gate_warning(&config, false));

    if dry_run {
        let dry_run_title = title_override.as_deref().unwrap_or("model-generated-title");
        let (markdown_path, _) = slide_artifact_paths(&slides_dir, dry_run_title)?;
        ensure_inside(&artifact_root, &markdown_path)?;
        return Ok(GenerateSlidesResult {
            markdown_path,
            pdf_path: None,
            output: GenerationOutput {
                project: config.project_name,
                project_instructions: project_instructions_path,
                models: selected_models,
                tools: tool_summaries.clone(),
                template: template.as_ref().map(|value| value.manifest.name.clone()),
                project_assets: reusable_asset_references.clone(),
                artifacts: Vec::new(),
                published_artifacts: Vec::new(),
                review: review_summary,
                prompts: used_prompts,
            },
            prompt_preview: Some(provider_request.user_prompt),
            tool_summaries,
            warnings: prepared_assets.warnings.clone(),
            published_pdf_path: None,
        });
    }

    emit_stage(
        &event_sink,
        GenerationStage::Draft,
        Some(draft_profile_name),
    );
    operation.emit(
        OperationStage::Draft,
        OperationEventKind::Started,
        BTreeMap::from([("model".to_string(), draft_profile_name.to_string())]),
    );
    let provider = provider_factory.text(&config, draft_profile)?;
    let compact_request = build_compact_generation_request(DraftPromptRequestContext {
        catalog: prompt_catalog.as_ref(),
        config: &config,
        theme: &theme,
        instruction: &request.instruction,
        title: title_override.as_deref(),
        source_bundle: &compact_source_bundle,
        image_generation_available: false,
        project_instructions: &project_instructions_prompt,
        tools: &[],
        max_tool_rounds: draft_tool_rounds,
        event_sink: event_sink.clone(),
        template: template.as_ref(),
        reusable_assets: &reusable_asset_references,
    })?;
    let compact_prompt_provenance = compact_request.prompt_provenance.clone();
    let draft_outcome = generate_with_compact_retry(
        provider.as_ref(),
        provider_request,
        compact_request,
        GenerationStage::Draft,
        &operation,
        OperationStage::Draft,
        &event_sink,
    )
    .await
    .context("Draft generation failed")?;
    if let Some(error) = draft_outcome.limit_error {
        used_prompts.extend(compact_prompt_provenance);
        warnings.push(format!(
            "Draft generation exceeded the model limit and was retried with compacted context: {error}"
        ));
    }
    let mut response = draft_outcome.response;
    let generated_tool_assets = tool_set.generated_artifacts()?;
    if let Some(template) = &template {
        response.text = template.compose(&response.text)?;
    }
    operation.emit(
        OperationStage::Draft,
        OperationEventKind::Completed,
        BTreeMap::new(),
    );
    let title = match title_override {
        Some(title) => title,
        None => match validate_draft_title(&response.text, &request.instruction) {
            Ok(title) => title,
            Err(error) => {
                let error = format!("{error:#}");
                emit_title_repair(&event_sink, &error);
                let mut title_request = build_title_repair_request(
                    prompt_catalog.as_ref(),
                    &config,
                    &request.instruction,
                    &response.text,
                    &error,
                    &project_instructions_prompt,
                )?;
                used_prompts.extend(title_request.prompt_provenance.clone());
                title_request.event_sink = event_sink.clone();
                let repaired = provider
                    .generate_text(title_request, &operation, OperationStage::Repair)
                    .await
                    .context("The drafter could not repair the missing deck title")?;
                parse_repaired_title(&repaired.text, &request.instruction)
                    .context("The drafter returned an invalid title after one focused repair")?
            }
        },
    };
    let (markdown_path, pdf_path) = slide_artifact_paths(&slides_dir, &title)?;
    let slug = slugify(&title);
    ensure_inside(&artifact_root, &markdown_path)?;
    ensure_inside(&artifact_root, &pdf_path)?;
    let validation_repair = normalize_and_repair_draft_once(
        prompt_catalog.as_ref(),
        provider.as_ref(),
        &config,
        &theme,
        &request.instruction,
        &project_instructions_prompt,
        &title,
        &response.text,
        &operation,
        &event_sink,
        draft_profile_name,
    )
    .await?;
    let mut markdown = validation_repair.markdown;
    used_prompts.extend(validation_repair.prompts);
    let mermaid_repair = repair_mermaid_once(
        prompt_catalog.as_ref(),
        provider.as_ref(),
        &config,
        &theme,
        &request.instruction,
        &project_instructions_prompt,
        &title,
        &markdown,
        diagram_renderer.as_ref(),
        workspace.as_ref(),
        &operation,
        &event_sink,
        draft_profile_name,
    )
    .await?;
    markdown = mermaid_repair.markdown;
    used_prompts.extend(mermaid_repair.prompts);
    let mut reviewer_provider: Option<Box<dyn TextGenerationProvider>> = None;
    if let Some((reviewer_name, reviewer_profile)) = reviewer_selection {
        match provider_factory.text(&config, reviewer_profile) {
            Ok(reviewer) => {
                emit_stage(
                    &event_sink,
                    GenerationStage::SemanticReview,
                    Some(reviewer_name),
                );
                let mut compaction_status = ReviewStatus::NotNeeded;
                let review_result = async {
                    let document = SlideDeckDocument::from_marp(&markdown, &title)?;
                    let snapshot = document.snapshot()?;
                    let mut retry = None;
                    let mut compacted = false;
                    let mut validation_retried = false;
                    loop {
                        let mut review_request = if compacted {
                            build_compact_review_request(ReviewPromptRequestContext {
                                catalog: prompt_catalog.as_ref(),
                                config: &config,
                                theme: &theme,
                                instruction: &request.instruction,
                                source_bundle: &compact_source_bundle,
                                snapshot: &snapshot,
                                retry: retry.as_ref(),
                                project_instructions: &project_instructions_prompt,
                                max_tool_rounds: model_tool_rounds(reviewer_profile),
                            })?
                        } else {
                            build_review_request(ReviewPromptRequestContext {
                                catalog: prompt_catalog.as_ref(),
                                config: &config,
                                theme: &theme,
                                instruction: &request.instruction,
                                source_bundle: &source_bundle,
                                snapshot: &snapshot,
                                retry: retry.as_ref(),
                                project_instructions: &project_instructions_prompt,
                                max_tool_rounds: model_tool_rounds(reviewer_profile),
                            })?
                        };
                        if !compacted {
                            review_request.tools = review_tool_definitions.clone();
                            review_request.tool_executor = Some(tool_set.executor.clone());
                            review_request.max_tool_rounds = model_tool_rounds(reviewer_profile);
                        }
                        review_request.event_sink = event_sink.clone();
                        used_prompts.extend(review_request.prompt_provenance.clone());
                        let original_chars = request_chars(&review_request);
                        operation.checkpoint(OperationStage::Review)?;
                        let response = match reviewer
                            .generate_text(review_request, &operation, OperationStage::Review)
                            .await
                        {
                            Ok(response) => response,
                            Err(error)
                                if !compacted && generation_limit(&error).is_some() =>
                            {
                                compacted = true;
                                compaction_status = ReviewStatus::Pending;
                                let compact_request = build_compact_review_request(
                                    ReviewPromptRequestContext {
                                        catalog: prompt_catalog.as_ref(),
                                        config: &config,
                                        theme: &theme,
                                        instruction: &request.instruction,
                                        source_bundle: &compact_source_bundle,
                                        snapshot: &snapshot,
                                        retry: retry.as_ref(),
                                        project_instructions: &project_instructions_prompt,
                                        max_tool_rounds: model_tool_rounds(reviewer_profile),
                                    },
                                )?;
                                emit_context_compaction(
                                    &event_sink,
                                    GenerationStage::SemanticReview,
                                    original_chars,
                                    request_chars(&compact_request),
                                );
                                continue;
                            }
                            Err(error) => {
                                if compacted && generation_limit(&error).is_some() {
                                    compaction_status = ReviewStatus::Failed;
                                }
                                return Err(error).context(if compacted {
                                    "Semantic review still exceeded the model limit after compacting context"
                                } else {
                                    "Semantic review request failed"
                                });
                            }
                        };
                        let candidate: Result<String> = (|| {
                            let patch = parse_json_patch(&response.text)?;
                            let mut candidate = document.clone();
                            candidate.apply_patch(&patch)?;
                            let reviewed = candidate.render()?;
                            let reviewed = normalize_marp_markdown(&reviewed, &config, &title)?;
                            extract_mermaid_blocks(&reviewed)?;
                            Ok(reviewed)
                        })();
                        let candidate = match candidate {
                            Ok(reviewed) => validate_mermaid_candidate(
                                &reviewed,
                                &theme,
                                diagram_renderer.as_ref(),
                                workspace.as_ref(),
                                &operation,
                            )
                                .await
                                .map(|()| reviewed),
                            Err(error) => Err(error),
                        };
                        match candidate {
                            Ok(reviewed) => {
                                if compacted {
                                    compaction_status = ReviewStatus::Completed;
                                }
                                return Ok(reviewed);
                            }
                            Err(error)
                                if !validation_retried && should_retry_model_output(&error) =>
                            {
                                let error = format!("{error:#}");
                                validation_retried = true;
                                emit_review_retry(&event_sink, 2, &error);
                                retry = Some(ReviewRetryContext {
                                    invalid_response: response.text,
                                    error,
                                });
                            }
                            Err(error) if !validation_retried => {
                                return Err(error)
                                    .context("Reviewer output could not be validated");
                            }
                            Err(error) => {
                                return Err(error).context(
                                    "Reviewer returned an invalid patch after one corrective retry",
                                );
                            }
                        }
                    }
                }
                .await;
                operation.checkpoint(OperationStage::Review)?;
                review_summary.context_compaction = compaction_status;
                match review_result {
                    Ok(reviewed) => {
                        markdown = reviewed;
                        review_summary.semantic_review = ReviewStatus::Completed;
                    }
                    Err(error) => {
                        if review_summary.context_compaction == ReviewStatus::Pending {
                            review_summary.context_compaction = ReviewStatus::Failed;
                        }
                        review_summary.semantic_review = ReviewStatus::Failed;
                        warnings.push(format!(
                            "Slide review failed; using the normalized draft: {error:#}"
                        ));
                    }
                }
                reviewer_provider = Some(reviewer);
            }
            Err(error) => {
                review_summary.semantic_review = ReviewStatus::Failed;
                warnings.push(format!(
                    "Slide reviewer could not start; using the normalized draft: {error:#}"
                ));
            }
        }

        emit_stage(&event_sink, GenerationStage::LayoutCheck, None);
        operation.emit(
            OperationStage::InspectLayout,
            OperationEventKind::Started,
            BTreeMap::new(),
        );
        let layout_context = LayoutInspectionContext {
            browser_path: config.marp.browser_path.as_deref(),
            diagram_renderer: diagram_renderer.as_ref(),
            slide_renderer: slide_renderer.as_ref(),
            workspace: workspace.as_ref(),
            project_assets: Some(&prepared_assets),
            generated_assets: &generated_tool_assets,
            operation: &operation,
        };
        let layout_result = inspect_candidate_layout(&markdown, &theme, &layout_context).await;
        operation.checkpoint(OperationStage::InspectLayout)?;
        match layout_result {
            Ok(issues) => {
                review_summary.layout_check = ReviewStatus::Completed;
                emit_layout_result(&event_sink, issues.len());
                if issues.is_empty() {
                    review_summary.repair = ReviewStatus::NotNeeded;
                } else if let Some(reviewer) = reviewer_provider.as_ref() {
                    emit_stage(
                        &event_sink,
                        GenerationStage::LayoutRepair,
                        Some(reviewer_name),
                    );
                    let mut repair_document = match SlideDeckDocument::from_marp(&markdown, &title)
                    {
                        Ok(document) => Some(document),
                        Err(error) => {
                            warnings.push(format!(
                                "Could not parse the deck for focused layout repair: {error:#}"
                            ));
                            None
                        }
                    };
                    let mut repaired_markdown = markdown.clone();
                    let mut assessment = LayoutAssessment::new(issues.clone());
                    let mut accepted_repairs = 0usize;
                    let mut valid_but_rejected = 0usize;
                    for (position, original_issue) in issues.iter().enumerate() {
                        let Some(document) = repair_document.as_ref() else {
                            break;
                        };
                        let Some(issue) = assessment.issue_for_slide(original_issue.slide) else {
                            continue;
                        };
                        let Some((_, slide)) = document.slide_at(issue.slide) else {
                            warnings.push(format!(
                                "Could not locate slide {} for focused layout repair.",
                                issue.slide
                            ));
                            continue;
                        };
                        emit_slide_repair(
                            &event_sink,
                            issue.slide,
                            position + 1,
                            issues.len(),
                            reviewer_name,
                        );
                        let original_slide = slide.markdown.clone();
                        let mut retry = None;
                        let mut compacted_context = false;
                        for attempt in 1..=2 {
                            let repair_request = build_layout_repair_request(
                                prompt_catalog.as_ref(),
                                LayoutRepairRequestContext {
                                    config: &config,
                                    theme: &theme,
                                    instruction: &request.instruction,
                                    title: &title,
                                    slide_markdown: &original_slide,
                                    issue: &issue,
                                    project_instructions: &project_instructions_prompt,
                                },
                                retry.as_ref(),
                                event_sink.clone(),
                            )?;
                            let compact_repair_request = build_compact_layout_repair_request(
                                prompt_catalog.as_ref(),
                                LayoutRepairRequestContext {
                                    config: &config,
                                    theme: &theme,
                                    instruction: &request.instruction,
                                    title: &title,
                                    slide_markdown: &original_slide,
                                    issue: &issue,
                                    project_instructions: &project_instructions_prompt,
                                },
                                retry.as_ref(),
                                event_sink.clone(),
                            )?;
                            let response = if compacted_context {
                                match reviewer
                                    .generate_text(
                                        compact_repair_request,
                                        &operation,
                                        OperationStage::Repair,
                                    )
                                    .await
                                {
                                    Ok(response) => response,
                                    Err(error) => {
                                        review_summary.context_compaction = ReviewStatus::Failed;
                                        warnings.push(format!(
                                            "Focused compact layout repair failed for slide {}: {error:#}",
                                            issue.slide
                                        ));
                                        break;
                                    }
                                }
                            } else {
                                match generate_with_compact_retry(
                                    reviewer.as_ref(),
                                    repair_request,
                                    compact_repair_request,
                                    GenerationStage::LayoutRepair,
                                    &operation,
                                    OperationStage::Repair,
                                    &event_sink,
                                )
                                .await
                                {
                                    Ok(outcome) => {
                                        if outcome.limit_error.is_some() {
                                            compacted_context = true;
                                            if review_summary.context_compaction
                                                != ReviewStatus::Failed
                                            {
                                                review_summary.context_compaction =
                                                    ReviewStatus::Completed;
                                            }
                                        }
                                        outcome.response
                                    }
                                    Err(error) => {
                                        if compact_retry_failed(&error) {
                                            review_summary.context_compaction =
                                                ReviewStatus::Failed;
                                        }
                                        warnings.push(format!(
                                            "Focused layout repair failed for slide {}: {error:#}",
                                            issue.slide
                                        ));
                                        break;
                                    }
                                }
                            };
                            let candidate = async {
                                let replacement = normalize_slide_replacement(&response.text)?;
                                let mut candidate_document = repair_document
                                    .as_ref()
                                    .context("Focused repair document is unavailable")?
                                    .clone();
                                candidate_document
                                    .replace_slide_fragment_at(issue.slide, replacement)?;
                                let candidate_markdown = candidate_document.render()?;
                                validate_mermaid_candidate(
                                    &candidate_markdown,
                                    &theme,
                                    diagram_renderer.as_ref(),
                                    workspace.as_ref(),
                                    &operation,
                                )
                                .await?;
                                let candidate_issues = inspect_candidate_layout(
                                    &candidate_markdown,
                                    &theme,
                                    &layout_context,
                                )
                                .await?;
                                Ok::<_, SfumatoError>((
                                    candidate_document,
                                    candidate_markdown,
                                    candidate_issues,
                                ))
                            }
                            .await;
                            match candidate {
                                Ok((candidate_document, candidate_markdown, candidate_issues)) => {
                                    if assessment.accept_if_improved(candidate_issues) {
                                        repair_document = Some(candidate_document);
                                        repaired_markdown = candidate_markdown;
                                        accepted_repairs += 1;
                                    } else {
                                        valid_but_rejected += 1;
                                        warnings.push(format!(
                                            "Focused layout repair for slide {} did not improve measured overflow; keeping the previous slide.",
                                            issue.slide
                                        ));
                                    }
                                    break;
                                }
                                Err(error) if attempt == 1 && should_retry_model_output(&error) => {
                                    let error = format!("{error:#}");
                                    emit_layout_repair_retry(
                                        &event_sink,
                                        issue.slide,
                                        attempt + 1,
                                        &error,
                                    );
                                    retry = Some(LayoutRepairRetryContext {
                                        invalid_response: response.text,
                                        error,
                                    });
                                }
                                Err(error) if attempt == 1 => {
                                    warnings.push(format!(
                                        "Focused layout repair could not be validated for slide {}: {error:#}",
                                        issue.slide
                                    ));
                                    break;
                                }
                                Err(error) => {
                                    warnings.push(format!(
                                        "Focused layout repair failed for slide {} after one corrective retry: {error:#}",
                                        issue.slide
                                    ));
                                    break;
                                }
                            }
                        }
                    }

                    if accepted_repairs > 0 {
                        markdown = repaired_markdown;
                        review_summary.repair = ReviewStatus::Accepted;
                        review_summary.remaining_issues = assessment.into_issues();
                    } else if valid_but_rejected > 0 {
                        review_summary.repair = ReviewStatus::Rejected;
                        review_summary.remaining_issues = issues;
                    } else {
                        review_summary.repair = ReviewStatus::Failed;
                        review_summary.remaining_issues = issues;
                    }
                } else {
                    review_summary.repair = ReviewStatus::Failed;
                    review_summary.remaining_issues = issues;
                    warnings.push(
                        "Layout issues were detected, but the reviewer provider was unavailable."
                            .to_string(),
                    );
                }
            }
            Err(error) => {
                review_summary.layout_check = ReviewStatus::Skipped;
                warnings.push(format!("Layout inspection skipped: {error:#}"));
            }
        }
        operation.emit(
            OperationStage::InspectLayout,
            OperationEventKind::Completed,
            BTreeMap::from([(
                "remaining_issues".to_string(),
                review_summary.remaining_issues.len().to_string(),
            )]),
        );
        if !review_summary.remaining_issues.is_empty() {
            let slides = review_summary
                .remaining_issues
                .iter()
                .map(|issue| issue.slide.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            warnings.push(format!(
                "Layout review completed with overflow remaining on slide(s): {slides}"
            ));
        }
    }

    operation.checkpoint(OperationStage::Render)?;
    operation.emit(
        OperationStage::Render,
        OperationEventKind::Started,
        BTreeMap::new(),
    );
    workspace.create_dir_all(&slides_dir)?;
    let (markdown, diagram_artifacts) = render_mermaid_diagrams(MermaidRenderRequest {
        markdown: &markdown,
        diagrams_dir: &diagrams_dir,
        theme: &theme,
        renderer: diagram_renderer.as_ref(),
        workspace: workspace.as_ref(),
        operation: &operation,
        stage: OperationStage::Render,
        image_markdown: mermaid_image_markdown,
    })
    .await?;
    let (used_project_asset_paths, used_project_assets) =
        prepared_assets.materialize_referenced(&markdown, workspace.as_ref())?;
    for path in reusable_asset_paths
        .iter()
        .filter(|path| !used_project_asset_paths.contains(path))
    {
        workspace.remove_file(path)?;
    }
    let used_generated_assets = retain_referenced_generated_assets(
        generated_tool_assets,
        &markdown,
        "images",
        workspace.as_ref(),
    )?;
    copy_theme_css(&theme, &theme_css_path, workspace.as_ref())?;
    workspace.write(&markdown_path, markdown.as_bytes())?;

    emit_stage(&event_sink, GenerationStage::Rendering, None);
    let rendered_pdf = if config.marp.pdf {
        let rendered_pdf_result = slide_renderer
            .render_pdf(
                &markdown_path,
                &theme_css_path,
                &pdf_path,
                config.marp.browser_path.as_deref(),
                &operation,
            )
            .await;
        operation.checkpoint(OperationStage::Render)?;
        match rendered_pdf_result {
            Ok(()) => Some(pdf_path.clone()),
            Err(error) => {
                warnings.push(format!("PDF export skipped: {error}"));
                None
            }
        }
    } else {
        None
    };
    operation.emit(
        OperationStage::Render,
        OperationEventKind::Completed,
        BTreeMap::from([("pdf".to_string(), rendered_pdf.is_some().to_string())]),
    );

    let mut staged_artifacts = vec![markdown_path.clone(), theme_css_path];
    staged_artifacts.extend(used_project_asset_paths);
    staged_artifacts.extend(used_generated_assets);
    used_prompts.extend(tool_set.generated_prompts()?);
    staged_artifacts.extend(diagram_artifacts);
    if let Some(pdf) = &rendered_pdf {
        staged_artifacts.push(pdf.clone());
    }
    staged_artifacts.sort();
    staged_artifacts.dedup();
    let mut unique_prompts = Vec::new();
    for prompt in used_prompts {
        if !unique_prompts.contains(&prompt) {
            unique_prompts.push(prompt);
        }
    }
    let used_prompts = unique_prompts;
    operation.checkpoint(OperationStage::CommitArtifacts)?;
    operation.emit(
        OperationStage::CommitArtifacts,
        OperationEventKind::Started,
        BTreeMap::new(),
    );
    let transaction = artifact_transaction
        .take()
        .context("Generation artifact transaction is unavailable")?;
    let files = staged_artifacts
        .iter()
        .map(|path| resource_artifact_file(&slides_dir, path))
        .collect::<Result<Vec<_>>>()?;
    let manifest = ResourceArtifactManifest {
        schema_version: 1,
        job_id: transaction.job_id().clone(),
        revision_id: transaction.revision_id().clone(),
        parent_revision: transaction.parent_revision().cloned(),
        project: config.project_name.clone(),
        resource_kind: ArtifactResourceKind::Slides,
        resource_id: slug.clone(),
        title: title.clone(),
        files,
        models: selected_models.clone(),
        prompts: used_prompts.clone(),
        plugins: Vec::new(),
        runtimes: Vec::new(),
        warnings: warnings.clone(),
    };
    let committed_revision = transaction.revision_id().to_string();
    let committed = transaction.commit(manifest)?;
    operation.emit(
        OperationStage::CommitArtifacts,
        OperationEventKind::Completed,
        BTreeMap::from([("revision".to_string(), committed_revision.clone())]),
    );
    let remap = |path: &Path| -> Result<PathBuf> {
        Ok(committed.root.join(
            path.strip_prefix(&slides_dir)
                .with_context(|| format!("Artifact {} escaped its transaction", path.display()))?,
        ))
    };
    let committed_markdown = remap(&markdown_path)?;
    let committed_pdf = rendered_pdf.as_deref().map(remap).transpose()?;
    let mut artifacts = staged_artifacts
        .iter()
        .map(|path| remap(path))
        .collect::<Result<Vec<_>>>()?;
    artifacts.push(committed.manifest_path.clone());
    operation.checkpoint(OperationStage::Publish)?;
    operation.emit(
        OperationStage::Publish,
        OperationEventKind::Started,
        BTreeMap::new(),
    );
    let publication = publish_slides(
        workspace.as_ref(),
        publish_root.as_deref(),
        committed_pdf.as_deref(),
        &pdf_path,
        &title,
        &config.project_name,
        &committed_revision,
    )?;
    let published_pdf_path = publication.pdf_path;
    if let Some(warning) = publication.warning {
        warnings.push(warning);
    }
    operation.emit(
        OperationStage::Publish,
        OperationEventKind::Completed,
        BTreeMap::from([("pdf".to_string(), published_pdf_path.is_some().to_string())]),
    );
    let published_artifacts = publication.artifacts;

    Ok(GenerateSlidesResult {
        markdown_path: committed_markdown,
        pdf_path: committed_pdf,
        published_pdf_path,
        output: GenerationOutput {
            project: config.project_name,
            project_instructions: project_instructions_path,
            models: selected_models,
            tools: tool_summaries.clone(),
            template: template.as_ref().map(|value| value.manifest.name.clone()),
            project_assets: used_project_assets,
            artifacts,
            published_artifacts,
            review: review_summary,
            prompts: used_prompts,
        },
        prompt_preview: None,
        tool_summaries,
        warnings,
    })
}

fn resource_artifact_file(root: &Path, path: &Path) -> Result<ResourceArtifactFile> {
    let relative = path
        .strip_prefix(root)
        .with_context(|| format!("Artifact {} escaped its transaction", path.display()))?
        .to_path_buf();
    let extension = path.extension().and_then(|value| value.to_str());
    let (kind, media_type) = match extension {
        Some("md") => (ArtifactKind::Markdown, Some("text/markdown")),
        Some("pdf") => (ArtifactKind::Pdf, Some("application/pdf")),
        Some("png") => (ArtifactKind::Image, Some("image/png")),
        Some("jpg" | "jpeg") => (ArtifactKind::Image, Some("image/jpeg")),
        Some("webp") => (ArtifactKind::Image, Some("image/webp")),
        Some("svg") => (ArtifactKind::Image, Some("image/svg+xml")),
        Some("css") => (ArtifactKind::Data, Some("text/css")),
        Some("mmd") => (ArtifactKind::Data, Some("text/plain")),
        _ => (ArtifactKind::Data, None),
    };
    Ok(ResourceArtifactFile {
        path: relative,
        kind,
        media_type: media_type.map(ToOwned::to_owned),
    })
}

fn emit_stage(
    sink: &Option<std::sync::Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
    stage: GenerationStage,
    profile: Option<&str>,
) {
    if let Some(sink) = sink {
        sink(TextGenerationEvent::StageStarted {
            stage,
            profile: profile.map(ToOwned::to_owned),
        });
    }
}

#[derive(Debug)]
struct CompactRetryOutcome {
    response: TextGenerationResponse,
    limit_error: Option<String>,
}

async fn generate_with_compact_retry(
    provider: &dyn TextGenerationProvider,
    request: TextGenerationRequest,
    compact_request: TextGenerationRequest,
    stage: GenerationStage,
    operation: &OperationContext,
    operation_stage: OperationStage,
    event_sink: &Option<std::sync::Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
) -> Result<CompactRetryOutcome> {
    let original_chars = request_chars(&request);
    match provider
        .generate_text(request, operation, operation_stage)
        .await
    {
        Ok(response) => Ok(CompactRetryOutcome {
            response,
            limit_error: None,
        }),
        Err(error) if generation_limit(&error).is_some() => {
            emit_context_compaction(
                event_sink,
                stage,
                original_chars,
                request_chars(&compact_request),
            );
            let limit_error = format!("{error:#}");
            let response = provider
                .generate_text(compact_request, operation, operation_stage)
                .await
                .map_err(|error| {
                    error
                        .context("Model request failed after compacting context")
                        .with_detail("compact_retry_failed", "true")
                })?;
            Ok(CompactRetryOutcome {
                response,
                limit_error: Some(limit_error),
            })
        }
        Err(error) => Err(error),
    }
}

fn emit_title_repair(
    sink: &Option<std::sync::Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
    error: &str,
) {
    if let Some(sink) = sink {
        sink(TextGenerationEvent::DraftTitleRepairStarted {
            error: error.to_string(),
        });
    }
}

fn emit_context_compaction(
    sink: &Option<std::sync::Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
    stage: GenerationStage,
    original_chars: usize,
    compacted_chars: usize,
) {
    if let Some(sink) = sink {
        sink(TextGenerationEvent::ContextCompactionStarted {
            stage,
            original_chars,
            compacted_chars,
        });
    }
}

fn emit_layout_result(
    sink: &Option<std::sync::Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
    issues: usize,
) {
    if let Some(sink) = sink {
        sink(TextGenerationEvent::LayoutCheckCompleted { issues });
    }
}

fn emit_slide_repair(
    sink: &Option<std::sync::Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
    slide: usize,
    position: usize,
    total: usize,
    profile: &str,
) {
    if let Some(sink) = sink {
        sink(TextGenerationEvent::LayoutSlideRepairStarted {
            slide,
            position,
            total,
            profile: profile.to_string(),
        });
    }
}

fn emit_review_retry(
    sink: &Option<std::sync::Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
    attempt: usize,
    error: &str,
) {
    if let Some(sink) = sink {
        sink(TextGenerationEvent::ReviewRetryStarted {
            attempt,
            error: error.to_string(),
        });
    }
}

fn emit_layout_repair_retry(
    sink: &Option<std::sync::Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
    slide: usize,
    attempt: usize,
    error: &str,
) {
    if let Some(sink) = sink {
        sink(TextGenerationEvent::LayoutSlideRepairRetryStarted {
            slide,
            attempt,
            error: error.to_string(),
        });
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

fn copy_theme_css(
    theme: &ThemePackage,
    destination: &Path,
    workspace: &dyn WorkspaceFileSystem,
) -> Result<()> {
    workspace.copy_file(&theme.marp_css_path(), destination)?;
    Ok(())
}

struct MermaidRepairOutcome {
    markdown: String,
    prompts: Vec<PromptProvenance>,
}

struct DraftValidationRepairOutcome {
    markdown: String,
    prompts: Vec<PromptProvenance>,
}

#[allow(clippy::too_many_arguments)]
async fn normalize_and_repair_draft_once(
    catalog: &dyn PromptCatalog,
    provider: &dyn TextGenerationProvider,
    config: &EffectiveConfig,
    theme: &ThemePackage,
    instruction: &str,
    project_instructions: &str,
    title: &str,
    draft: &str,
    operation: &OperationContext,
    event_sink: &Option<std::sync::Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
    profile: &str,
) -> Result<DraftValidationRepairOutcome> {
    let initial = normalize_marp_markdown(draft, config, title)
        .and_then(|candidate| validate_normalized_deck(&candidate, title).map(|_| candidate));
    let validation_error = match initial {
        Ok(markdown) => {
            return Ok(DraftValidationRepairOutcome {
                markdown,
                prompts: Vec::new(),
            });
        }
        Err(error) if should_retry_model_output(&error) => error,
        Err(error) => return Err(error.context("Generated slide deck is invalid")),
    };

    emit_stage(event_sink, GenerationStage::ValidationRepair, Some(profile));
    operation.emit(
        OperationStage::Repair,
        OperationEventKind::Started,
        BTreeMap::from([
            ("kind".to_string(), "validation".to_string()),
            ("model".to_string(), profile.to_string()),
        ]),
    );
    let mut request = build_validation_repair_request(
        catalog,
        config,
        theme,
        instruction,
        title,
        draft,
        &validation_error.to_string(),
        project_instructions,
    )?;
    request.event_sink = event_sink.clone();
    let prompts = request.prompt_provenance.clone();
    let response = provider
        .generate_text(request, operation, OperationStage::Repair)
        .await
        .map_err(|error| error.context("Draft validation repair request failed"))?;
    let markdown = normalize_marp_markdown(&response.text, config, title)
        .and_then(|candidate| validate_normalized_deck(&candidate, title).map(|_| candidate))
        .map_err(|error| {
            error.context("Draft remained invalid after one focused validation repair")
        })?;
    operation.emit(
        OperationStage::Repair,
        OperationEventKind::Completed,
        BTreeMap::from([("kind".to_string(), "validation".to_string())]),
    );
    Ok(DraftValidationRepairOutcome { markdown, prompts })
}

#[allow(clippy::too_many_arguments)]
async fn repair_mermaid_once(
    catalog: &dyn PromptCatalog,
    provider: &dyn TextGenerationProvider,
    config: &EffectiveConfig,
    theme: &ThemePackage,
    instruction: &str,
    project_instructions: &str,
    title: &str,
    markdown: &str,
    diagram_renderer: &dyn DiagramRenderer,
    workspace: &dyn WorkspaceFileSystem,
    operation: &OperationContext,
    event_sink: &Option<std::sync::Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
    profile: &str,
) -> Result<MermaidRepairOutcome> {
    let validation_error = match extract_mermaid_blocks(markdown) {
        Ok(blocks) if blocks.is_empty() => {
            return Ok(MermaidRepairOutcome {
                markdown: markdown.to_string(),
                prompts: Vec::new(),
            });
        }
        Ok(_) => {
            validate_mermaid_candidate(markdown, theme, diagram_renderer, workspace, operation)
                .await
                .err()
        }
        Err(error) => Some(error),
    };
    let Some(validation_error) = validation_error else {
        return Ok(MermaidRepairOutcome {
            markdown: markdown.to_string(),
            prompts: Vec::new(),
        });
    };
    if !should_retry_model_output(&validation_error) {
        return Err(validation_error).context("Could not validate generated Mermaid diagrams");
    }

    emit_stage(event_sink, GenerationStage::DiagramRepair, Some(profile));
    operation.emit(
        OperationStage::Repair,
        OperationEventKind::Started,
        BTreeMap::from([
            ("kind".to_string(), "mermaid".to_string()),
            ("model".to_string(), profile.to_string()),
        ]),
    );
    let document = SlideDeckDocument::from_marp(markdown, title)
        .context("Could not parse the deck for Mermaid repair")?;
    let snapshot = document.snapshot()?;
    let mut request = build_mermaid_repair_request(
        catalog,
        config,
        theme,
        instruction,
        project_instructions,
        &snapshot,
        &format!("{validation_error:#}"),
    )?;
    request.event_sink = event_sink.clone();
    let prompts = request.prompt_provenance.clone();
    let response = provider
        .generate_text(request, operation, OperationStage::Repair)
        .await
        .context("Mermaid repair request failed")?;
    let candidate = apply_mermaid_repair_response(markdown, title, &response.text, config)?;
    validate_normalized_deck(&candidate, title)?;
    validate_mermaid_candidate(&candidate, theme, diagram_renderer, workspace, operation)
        .await
        .context("Mermaid repair remained invalid after one focused attempt")?;
    operation.emit(
        OperationStage::Repair,
        OperationEventKind::Completed,
        BTreeMap::from([("kind".to_string(), "mermaid".to_string())]),
    );
    Ok(MermaidRepairOutcome {
        markdown: candidate,
        prompts,
    })
}

fn apply_mermaid_repair_response(
    markdown: &str,
    title: &str,
    response: &str,
    config: &EffectiveConfig,
) -> Result<String> {
    let patch = parse_json_patch(response)
        .context("Mermaid repair did not return a valid RFC 6902 patch")?;
    let mut document = SlideDeckDocument::from_marp(markdown, title)
        .context("Could not parse the deck returned for Mermaid repair")?;
    document.apply_patch(&patch)?;
    normalize_marp_markdown(&document.render()?, config, title)
}

async fn validate_mermaid_candidate(
    markdown: &str,
    theme: &ThemePackage,
    diagram_renderer: &dyn DiagramRenderer,
    workspace: &dyn WorkspaceFileSystem,
    operation: &OperationContext,
) -> Result<()> {
    let temp = workspace.temporary_directory("sfumato-mermaid-review-")?;
    let diagrams_dir = temp.path().join("diagrams");
    render_mermaid_diagrams(MermaidRenderRequest {
        markdown,
        diagrams_dir: &diagrams_dir,
        theme,
        renderer: diagram_renderer,
        workspace,
        operation,
        stage: OperationStage::Repair,
        image_markdown: mermaid_image_markdown,
    })
    .await
    .map(|_| ())
}

fn should_retry_model_output(error: &SfumatoError) -> bool {
    error.class == ErrorClass::InvalidOutput
        || error.code == ErrorCode::Validation && !error.message.contains("validation workspace")
}

pub(super) struct LayoutInspectionContext<'a> {
    pub browser_path: Option<&'a Path>,
    pub diagram_renderer: &'a dyn DiagramRenderer,
    pub slide_renderer: &'a dyn SlideRenderer,
    pub workspace: &'a dyn WorkspaceFileSystem,
    pub project_assets: Option<&'a crate::resources::project_assets::PreparedProjectAssets>,
    pub generated_assets: &'a [PathBuf],
    pub operation: &'a OperationContext,
}

async fn inspect_candidate_layout(
    markdown: &str,
    theme: &ThemePackage,
    context: &LayoutInspectionContext<'_>,
) -> Result<Vec<SlideLayoutIssue>> {
    context
        .operation
        .checkpoint(OperationStage::InspectLayout)?;
    let temp = context
        .workspace
        .temporary_directory("sfumato-layout-review-")?;
    let markdown_path = temp.path().join("review.md");
    let theme_path = temp.path().join("theme.css");
    let diagrams_dir = temp.path().join("diagrams");
    let html_path = temp.path().join("review.html");
    if let Some(project_assets) = context.project_assets {
        project_assets.stage_referenced(markdown, temp.path(), context.workspace)?;
    }
    stage_referenced_generated_assets(
        context.generated_assets,
        markdown,
        "images",
        temp.path(),
        context.workspace,
    )?;
    let (rendered, _) = render_mermaid_diagrams(MermaidRenderRequest {
        markdown,
        diagrams_dir: &diagrams_dir,
        theme,
        renderer: context.diagram_renderer,
        workspace: context.workspace,
        operation: context.operation,
        stage: OperationStage::InspectLayout,
        image_markdown: mermaid_image_markdown,
    })
    .await?;
    copy_theme_css(theme, &theme_path, context.workspace)?;
    context
        .workspace
        .write(&markdown_path, rendered.as_bytes())?;
    let issues = context
        .slide_renderer
        .inspect_layout(
            &markdown_path,
            &theme_path,
            &html_path,
            context.browser_path,
            context.operation,
        )
        .await?;
    Ok(issues)
}

fn model_tool_rounds(profile: &crate::config::ModelProfile) -> usize {
    profile.options.tool_rounds()
}

fn format_tokens(tokens: &std::collections::BTreeMap<String, String>) -> String {
    tokens
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../../tests/unit/resources_slides.rs"]
mod tests;
