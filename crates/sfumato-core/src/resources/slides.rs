use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use sfumato_domain::ArtifactKind;
use sha2::{Digest, Sha256};
use slug::slugify;
use walkdir::WalkDir;

use crate::{
    artifacts::{
        ArtifactResourceKind, ArtifactStore, ResourceArtifactFile, ResourceArtifactManifest,
    },
    config::{Capability, EffectiveConfig, ModelRole},
    generation::{
        GenerationOutput, GenerationRequest, GenerationToolSummary, ReviewStatus, SlideLayoutIssue,
        SlideReviewSummary,
    },
    instructions::ProjectInstructions,
    prompts::{PromptCatalog, PromptId, PromptPair, PromptRenderRequest, PromptVariables},
    providers::{
        GenerationStage, ProviderFactory, TextGenerationEvent, TextGenerationLimitError,
        TextGenerationProvider, TextGenerationRequest, TextGenerationResponse, ToolDefinition,
    },
    renderers::{
        diagrams::{MermaidDiagramRenderer, MermaidThemeConfig},
        marp,
    },
    review::{ReviewSnapshot, decks::SlideDeckDocument, parse_json_patch},
    themes::{ThemePackage, ThemeService, ThemeTokens},
    tools::{ImageToolConfig, generation_tools},
};

mod edit;

pub use edit::{EditSlidesOptions, EditSlidesRequest, EditSlidesResult, edit_slides};

const SUPPORTED_EXTENSIONS: &[&str] = &[
    "md", "txt", "rs", "py", "js", "ts", "html", "css", "json", "toml", "yaml", "yml",
];
const GENERATED_IMAGE_MARP_HEIGHT: &str = "420px";
const MAX_SOURCE_FILES: usize = 256;
const MAX_SOURCE_BYTES_PER_FILE: u64 = 1_048_576;
const MAX_SOURCE_TOTAL_BYTES: u64 = 16_777_216;
const MAX_SOURCE_BUNDLE_CHARS: usize = 48_000;

pub struct GenerateSlidesOptions {
    pub title: Option<String>,
    pub dry_run: bool,
    pub review: bool,
    pub event_sink: Option<std::sync::Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
    pub prompt_catalog: Arc<dyn PromptCatalog>,
    pub artifact_store: Arc<dyn ArtifactStore>,
    pub provider_factory: Arc<dyn ProviderFactory>,
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

#[derive(Clone, Debug)]
struct SourceDocument {
    path: PathBuf,
    content: String,
}

pub async fn generate_slides(
    config: EffectiveConfig,
    request: GenerationRequest,
    options: GenerateSlidesOptions,
) -> Result<GenerateSlidesResult> {
    let GenerateSlidesOptions {
        title: title_override,
        dry_run,
        review,
        event_sink,
        prompt_catalog,
        artifact_store,
        provider_factory,
    } = options;
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

    let project_instructions = ProjectInstructions::load(&config.project_root)?;
    let project_instructions_prompt = project_instructions
        .as_ref()
        .map(ProjectInstructions::prompt_section)
        .unwrap_or_else(|| "Project instructions: no SFUMATO.md was found.".to_string());
    let project_instructions_path = project_instructions
        .as_ref()
        .map(|instructions| instructions.path.clone());
    let theme = ThemeService::load()?.resolve(&config.theme)?;
    let documents = collect_sources(&request.sources)?;
    let image_selection = config
        .model_defaults
        .contains_key(&Capability::Image)
        .then(|| config.resolve_model(Capability::Image))
        .transpose()?;
    let image_tool = image_selection
        .map(|(profile_name, profile)| {
            let provider: Arc<dyn crate::providers::ImageGenerationProvider> =
                Arc::from(provider_factory.image(&config, profile)?);
            Ok::<ImageToolConfig, anyhow::Error>(ImageToolConfig {
                provider,
                profile_name: profile_name.to_string(),
                output_dir: images_dir.clone(),
                theme: theme.clone(),
                project_instructions: project_instructions
                    .as_ref()
                    .map(|instructions| instructions.content.clone()),
            })
        })
        .transpose()?;
    let tool_set = generation_tools(
        &config.project_root,
        &request.sources,
        image_tool,
        prompt_catalog.clone(),
    )?;
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
    })?;
    provider_request.tools = tool_set.definitions.clone();
    provider_request.tool_executor = Some(tool_set.executor.clone());
    provider_request.event_sink = event_sink.clone();
    provider_request.max_tool_rounds = draft_tool_rounds;
    let mut used_prompts = provider_request.prompt_provenance.clone();
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
    let mut warnings = Vec::new();

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
                artifacts: Vec::new(),
                published_artifacts: Vec::new(),
                review: review_summary,
                prompts: used_prompts,
            },
            prompt_preview: Some(provider_request.user_prompt),
            tool_summaries,
            warnings: Vec::new(),
            published_pdf_path: None,
        });
    }

    emit_stage(
        &event_sink,
        GenerationStage::Draft,
        Some(draft_profile_name),
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
    })?;
    let compact_prompt_provenance = compact_request.prompt_provenance.clone();
    let draft_outcome = generate_with_compact_retry(
        provider.as_ref(),
        provider_request,
        compact_request,
        GenerationStage::Draft,
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
    let response = draft_outcome.response;
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
                    .generate_text(title_request)
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
    let mut markdown = normalize_marp_markdown(&response.text, &config, &title)?;
    validate_normalized_deck(&markdown, &title)?;
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
                        let response = match reviewer.generate_text(review_request).await {
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
                            Ok(reviewed) => validate_mermaid_candidate(&reviewed, &theme, &slug)
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
        match inspect_candidate_layout(
            &markdown,
            &theme,
            &slug,
            config.marp.browser_path.as_deref(),
        )
        .await
        {
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
                    let ranges = match slide_ranges(&markdown) {
                        Ok(ranges) => ranges,
                        Err(error) => {
                            warnings.push(format!(
                                "Could not map slides for focused layout repair: {error:#}"
                            ));
                            Vec::new()
                        }
                    };
                    let mut replacements = Vec::new();
                    for (position, issue) in
                        issues.iter().enumerate().filter(|_| !ranges.is_empty())
                    {
                        let Some(range) = ranges
                            .iter()
                            .find(|range| range.number == issue.slide)
                            .cloned()
                        else {
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
                        let original_slide = markdown[range.start..range.end].trim().to_string();
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
                                    issue,
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
                                    issue,
                                    project_instructions: &project_instructions_prompt,
                                },
                                retry.as_ref(),
                                event_sink.clone(),
                            )?;
                            let response = if compacted_context {
                                match reviewer.generate_text(compact_repair_request).await {
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
                            let candidate = match normalize_slide_replacement(&response.text) {
                                Ok(replacement) => {
                                    let candidate = apply_slide_replacements(
                                        &markdown,
                                        vec![SlideReplacement {
                                            range: range.clone(),
                                            markdown: replacement.clone(),
                                        }],
                                    );
                                    validate_mermaid_candidate(&candidate, &theme, &slug)
                                        .await
                                        .map(|()| replacement)
                                }
                                Err(error) => Err(error),
                            };
                            match candidate {
                                Ok(replacement) => {
                                    replacements.push(SlideReplacement {
                                        range: range.clone(),
                                        markdown: replacement,
                                    });
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

                    if replacements.is_empty() {
                        review_summary.repair = ReviewStatus::Failed;
                        review_summary.remaining_issues = issues;
                    } else {
                        let repaired = apply_slide_replacements(&markdown, replacements);
                        match inspect_candidate_layout(
                            &repaired,
                            &theme,
                            &slug,
                            config.marp.browser_path.as_deref(),
                        )
                        .await
                        {
                            Ok(repaired_issues)
                                if layout_score(&repaired_issues) < layout_score(&issues) =>
                            {
                                markdown = repaired;
                                review_summary.repair = ReviewStatus::Accepted;
                                review_summary.remaining_issues = repaired_issues;
                            }
                            Ok(_) => {
                                review_summary.repair = ReviewStatus::Rejected;
                                review_summary.remaining_issues = issues;
                                warnings.push("Focused layout repairs did not improve the deck; keeping the reviewed version.".to_string());
                            }
                            Err(error) => {
                                review_summary.repair = ReviewStatus::Failed;
                                review_summary.remaining_issues = issues;
                                warnings.push(format!(
                                    "Could not validate the focused layout repairs: {error:#}"
                                ));
                            }
                        }
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

    fs::create_dir_all(&slides_dir)
        .with_context(|| format!("Could not create {}", slides_dir.display()))?;
    let (markdown, diagram_artifacts) =
        render_mermaid_diagrams(&markdown, &diagrams_dir, &slug, &theme).await?;
    copy_theme_css(&theme, &theme_css_path)?;
    fs::write(&markdown_path, markdown)
        .with_context(|| format!("Could not write {}", markdown_path.display()))?;

    emit_stage(&event_sink, GenerationStage::Rendering, None);
    let rendered_pdf = match marp::render_pdf(
        &markdown_path,
        &theme_css_path,
        &pdf_path,
        config.marp.browser_path.as_deref(),
    )
    .await
    {
        Ok(()) => Some(pdf_path),
        Err(error) => {
            warnings.push(format!("PDF export skipped: {error}"));
            None
        }
    };

    let mut staged_artifacts = vec![markdown_path.clone(), theme_css_path];
    staged_artifacts.extend(tool_set.generated_artifacts()?);
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
        warnings: warnings.clone(),
    };
    let committed = transaction.commit(manifest)?;
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
    let published_pdf_path = match (&committed_pdf, publish_root) {
        (Some(pdf), Some(destination)) => Some(publish_artifact(pdf, &destination)?),
        _ => None,
    };
    let published_artifacts = published_pdf_path.iter().cloned().collect();

    Ok(GenerateSlidesResult {
        markdown_path: committed_markdown,
        pdf_path: committed_pdf,
        published_pdf_path,
        output: GenerationOutput {
            project: config.project_name,
            project_instructions: project_instructions_path,
            models: selected_models,
            tools: tool_summaries.clone(),
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

fn publish_artifact(artifact: &Path, destination_dir: &Path) -> Result<PathBuf> {
    let filename = artifact
        .file_name()
        .context("Published artifact must have a filename")?;
    fs::create_dir_all(destination_dir)
        .with_context(|| format!("Could not create {}", destination_dir.display()))?;
    let destination = destination_dir.join(filename);
    if artifact != destination {
        let mut source = fs::File::open(artifact)
            .with_context(|| format!("Could not open {} for publishing", artifact.display()))?;
        let mut temporary =
            tempfile::NamedTempFile::new_in(destination_dir).with_context(|| {
                format!(
                    "Could not create a temporary published artifact in {}",
                    destination_dir.display()
                )
            })?;
        io::copy(&mut source, &mut temporary).with_context(|| {
            format!(
                "Could not stage {} for publishing to {}",
                artifact.display(),
                destination.display()
            )
        })?;
        temporary.as_file().sync_all()?;
        temporary
            .persist(&destination)
            .map_err(|error| error.error)
            .with_context(|| format!("Could not atomically publish {}", destination.display()))?;
    }
    Ok(destination)
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

#[derive(Debug)]
struct CompactRetryError(anyhow::Error);

impl std::fmt::Display for CompactRetryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "Model request failed after compacting context: {}",
            self.0
        )
    }
}

impl std::error::Error for CompactRetryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.as_ref())
    }
}

async fn generate_with_compact_retry(
    provider: &dyn TextGenerationProvider,
    request: TextGenerationRequest,
    compact_request: TextGenerationRequest,
    stage: GenerationStage,
    event_sink: &Option<std::sync::Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
) -> Result<CompactRetryOutcome> {
    let original_chars = request_chars(&request);
    match provider.generate_text(request).await {
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
                .generate_text(compact_request)
                .await
                .map_err(CompactRetryError)?;
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

fn copy_theme_css(theme: &ThemePackage, destination: &Path) -> Result<()> {
    fs::create_dir_all(
        destination
            .parent()
            .context("Theme CSS output path must have a parent")?,
    )?;
    fs::copy(theme.marp_css_path(), destination)
        .with_context(|| format!("Could not copy theme CSS to {}", destination.display()))?;
    Ok(())
}

async fn render_mermaid_diagrams(
    markdown: &str,
    diagrams_dir: &Path,
    _slug: &str,
    theme: &ThemePackage,
) -> Result<(String, Vec<PathBuf>)> {
    let blocks = extract_mermaid_blocks(markdown)?;
    if blocks.is_empty() {
        return Ok((markdown.to_string(), Vec::new()));
    }

    fs::create_dir_all(diagrams_dir)
        .with_context(|| format!("Could not create {}", diagrams_dir.display()))?;
    let renderer = MermaidDiagramRenderer;
    let mermaid_theme = mermaid_theme_config(&theme.manifest.tokens);
    let mut rendered = String::new();
    let mut cursor = 0;
    let mut artifacts = Vec::new();

    for (index, block) in blocks.iter().enumerate() {
        rendered.push_str(&markdown[cursor..block.start]);
        let diagram_index = index + 1;
        let source = normalize_mermaid_source(&block.source);
        let content_hash = format!("{:x}", Sha256::digest(source.as_bytes()));
        let name = format!("diagram-{}", &content_hash[..16]);
        let source_path = diagrams_dir.join(format!("{name}.mmd"));
        let artifact_path = diagrams_dir.join(format!("{name}.svg"));
        fs::write(&source_path, &source)
            .with_context(|| format!("Could not write {}", source_path.display()))?;
        let _svg = renderer
            .render_svg(&source_path, &artifact_path, &mermaid_theme)
            .await?;
        if !artifact_path.exists() {
            bail!(
                "Mermaid CLI did not write the expected SVG artifact {}",
                artifact_path.display()
            );
        }
        rendered.push_str(&format!(
            "![Mermaid diagram {diagram_index}](diagrams/{name}.svg)"
        ));
        artifacts.push(source_path);
        artifacts.push(artifact_path);
        cursor = block.end;
    }

    rendered.push_str(&markdown[cursor..]);
    prune_unreferenced_diagrams(&rendered, diagrams_dir)?;
    Ok((rendered, artifacts))
}

fn prune_unreferenced_diagrams(markdown: &str, diagrams_dir: &Path) -> Result<()> {
    for entry in fs::read_dir(diagrams_dir)? {
        let entry = entry?;
        if entry.file_type()?.is_symlink() {
            bail!(
                "Diagram directory contains an unsafe symlink: {}",
                entry.path().display()
            );
        }
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("svg") {
            continue;
        }
        let filename = entry.file_name();
        let filename = filename.to_string_lossy();
        if markdown.contains(&format!("diagrams/{filename}")) {
            continue;
        }
        fs::remove_file(&path)?;
        let source = path.with_extension("mmd");
        if source.exists() {
            fs::remove_file(source)?;
        }
    }
    Ok(())
}

fn mermaid_theme_config(tokens: &ThemeTokens) -> MermaidThemeConfig {
    let colors = &tokens.colors;
    let fonts = &tokens.fonts;
    let background = theme_token(colors, "background", "#ffffff");
    let surface = theme_token(colors, "surface", &background);
    let surface_alt = theme_token(colors, "surface-alt", &surface);
    let text = theme_token(colors, "text", "#222222");
    let primary = theme_token(colors, "primary", "#315c8c");
    let accent = theme_token(colors, "accent", &primary);
    let muted = theme_token(colors, "muted", &text);
    let body_font = theme_token(fonts, "body", "system-ui, sans-serif");

    MermaidThemeConfig::new(BTreeMap::from([
        ("background".to_string(), background.clone()),
        ("mainBkg".to_string(), surface.clone()),
        ("primaryColor".to_string(), surface.clone()),
        ("primaryTextColor".to_string(), text.clone()),
        ("primaryBorderColor".to_string(), primary.clone()),
        ("secondaryColor".to_string(), surface_alt.clone()),
        ("secondaryTextColor".to_string(), text.clone()),
        ("secondaryBorderColor".to_string(), accent.clone()),
        ("tertiaryColor".to_string(), background.clone()),
        ("tertiaryTextColor".to_string(), text.clone()),
        ("tertiaryBorderColor".to_string(), muted.clone()),
        ("lineColor".to_string(), accent.clone()),
        ("textColor".to_string(), text.clone()),
        ("fontFamily".to_string(), body_font),
        ("nodeBorder".to_string(), primary.clone()),
        ("nodeTextColor".to_string(), text.clone()),
        ("clusterBkg".to_string(), background),
        ("clusterBorder".to_string(), accent.clone()),
        ("defaultLinkColor".to_string(), accent.clone()),
        ("edgeLabelBackground".to_string(), surface_alt.clone()),
        ("noteBkgColor".to_string(), surface_alt),
        ("noteTextColor".to_string(), text),
        ("noteBorderColor".to_string(), accent),
    ]))
}

fn normalize_mermaid_source(source: &str) -> String {
    let mut normalized = String::new();
    let mut rest = source;

    while let Some(start) = rest.find("[\"") {
        let label_start = start + 2;
        normalized.push_str(&rest[..label_start]);
        let Some(end) = rest[label_start..].find("\"]") else {
            normalized.push_str(&rest[label_start..]);
            return normalized;
        };
        let label_end = label_start + end;
        normalized.push_str(&normalize_mermaid_label(&rest[label_start..label_end]));
        rest = &rest[label_end..];
    }

    normalized.push_str(rest);
    normalized
}

fn normalize_mermaid_label(label: &str) -> String {
    let label = label.replace("\\n", "<br/>");
    let spaced = insert_missing_label_spaces(&label);
    wrap_mermaid_label(&spaced, 28)
}

fn insert_missing_label_spaces(label: &str) -> String {
    let mut output = String::new();
    let mut previous = None;

    for current in label.chars() {
        if let Some(previous) = previous
            && should_insert_label_space(previous, current)
        {
            output.push(' ');
        }
        output.push(current);
        previous = Some(current);
    }

    output
}

fn should_insert_label_space(previous: char, current: char) -> bool {
    (previous.is_ascii_digit() && current.is_alphabetic())
        || (previous == ')' && current.is_alphabetic())
        || (previous.is_lowercase() && current.is_uppercase())
}

fn wrap_mermaid_label(label: &str, max_len: usize) -> String {
    label
        .split("<br/>")
        .flat_map(|segment| wrap_mermaid_label_segment(segment, max_len))
        .collect::<Vec<_>>()
        .join("<br/>")
}

fn wrap_mermaid_label_segment(segment: &str, max_len: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();

    for word in segment.split_whitespace() {
        let next_len =
            current.chars().count() + usize::from(!current.is_empty()) + word.chars().count();
        if !current.is_empty() && next_len > max_len {
            lines.push(current);
            current = String::new();
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }

    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(segment.to_string());
    }
    lines
}

fn theme_token(tokens: &BTreeMap<String, String>, name: &str, fallback: &str) -> String {
    tokens
        .get(name)
        .filter(|value| is_mermaid_theme_value(value))
        .cloned()
        .unwrap_or_else(|| fallback.to_string())
}

fn is_mermaid_theme_value(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.starts_with('#') || trimmed.contains(',') || trimmed.contains("sans")
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MermaidBlock {
    start: usize,
    end: usize,
    source: String,
}

fn extract_mermaid_blocks(markdown: &str) -> Result<Vec<MermaidBlock>> {
    let mut blocks = Vec::new();
    let mut cursor = 0;

    while let Some(relative_start) = markdown[cursor..].find("```") {
        let fence_start = cursor + relative_start;
        let after_ticks = fence_start + 3;
        let line_end = markdown[after_ticks..]
            .find('\n')
            .map(|offset| after_ticks + offset)
            .unwrap_or(markdown.len());
        let language = markdown[after_ticks..line_end].trim();

        if !language.eq_ignore_ascii_case("mermaid") {
            cursor = line_end;
            continue;
        }

        let content_start = if line_end < markdown.len() {
            line_end + 1
        } else {
            line_end
        };
        let Some(relative_end) = markdown[content_start..].find("\n```") else {
            bail!("Generated Mermaid diagram fence is not closed");
        };
        let content_end = content_start + relative_end;
        let fence_end = content_end + "\n```".len();

        blocks.push(MermaidBlock {
            start: fence_start,
            end: fence_end,
            source: markdown[content_start..content_end].trim().to_string(),
        });
        cursor = fence_end;
    }

    Ok(blocks)
}

const DRAFT_PROMPTS: PromptPair = PromptPair {
    system: PromptId::SlidesDraftSystem,
    user: PromptId::SlidesDraftUser,
    tool_exhausted: PromptId::SlidesDraftToolExhaustedUser,
};
const COMPACT_DRAFT_PROMPTS: PromptPair = PromptPair {
    system: PromptId::SlidesCompactDraftSystem,
    user: PromptId::SlidesCompactDraftUser,
    tool_exhausted: PromptId::SlidesDraftToolExhaustedUser,
};
const TITLE_REPAIR_PROMPTS: PromptPair = PromptPair {
    system: PromptId::SlidesTitleRepairSystem,
    user: PromptId::SlidesTitleRepairUser,
    tool_exhausted: PromptId::SlidesDraftToolExhaustedUser,
};
const REVIEW_PROMPTS: PromptPair = PromptPair {
    system: PromptId::SlidesReviewSystem,
    user: PromptId::SlidesReviewUser,
    tool_exhausted: PromptId::SlidesReviewToolExhaustedUser,
};
const COMPACT_REVIEW_PROMPTS: PromptPair = PromptPair {
    system: PromptId::SlidesCompactReviewSystem,
    user: PromptId::SlidesCompactReviewUser,
    tool_exhausted: PromptId::SlidesReviewToolExhaustedUser,
};
const LAYOUT_REPAIR_PROMPTS: PromptPair = PromptPair {
    system: PromptId::SlidesLayoutRepairSystem,
    user: PromptId::SlidesLayoutRepairUser,
    tool_exhausted: PromptId::SlidesLayoutRepairToolExhaustedUser,
};
const COMPACT_LAYOUT_REPAIR_PROMPTS: PromptPair = PromptPair {
    system: PromptId::SlidesCompactLayoutRepairSystem,
    user: PromptId::SlidesCompactLayoutRepairUser,
    tool_exhausted: PromptId::SlidesLayoutRepairToolExhaustedUser,
};

#[derive(Clone, Debug, Default, Serialize)]
struct SlidePromptContext {
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
    source_bundle: String,
    tools: Vec<GenerationToolSummary>,
    deck_snapshot: String,
    validation_error: String,
    headings: String,
    retry_present: bool,
    retry_error: String,
    retry_invalid_response: String,
    issue_report: String,
    slide_markdown: String,
    compact: bool,
    max_tool_rounds: usize,
}

fn render_prompt_request(
    catalog: &dyn PromptCatalog,
    pair: PromptPair,
    context: &SlidePromptContext,
) -> Result<TextGenerationRequest> {
    let variables = PromptVariables::from_serializable(context)?;
    let system = catalog.render(PromptRenderRequest {
        id: pair.system,
        variables: variables.clone(),
    })?;
    let user = catalog.render(PromptRenderRequest {
        id: pair.user,
        variables: variables.clone(),
    })?;
    let exhausted = catalog.render(PromptRenderRequest {
        id: pair.tool_exhausted,
        variables,
    })?;
    let mut request = TextGenerationRequest::new(system.text, user.text);
    request.tool_exhausted_prompt = Some(exhausted.text);
    request.prompt_provenance = vec![system.provenance, user.provenance, exhausted.provenance];
    request.max_tool_rounds = context.max_tool_rounds;
    Ok(request)
}

#[allow(clippy::too_many_arguments)]
fn review_prompt_context(
    config: &EffectiveConfig,
    theme: &ThemePackage,
    instruction: &str,
    project_instructions: String,
    source_bundle: String,
    deck_snapshot: String,
    retry: Option<&ReviewRetryContext>,
    compact: bool,
    max_tool_rounds: usize,
) -> SlidePromptContext {
    SlidePromptContext {
        project: config.project_name.clone(),
        project_root: config.project_root.display().to_string(),
        theme_name: theme.manifest.name.clone(),
        theme_colors: format_tokens(&theme.manifest.tokens.colors),
        theme_fonts: format_tokens(&theme.manifest.tokens.fonts),
        instruction: instruction.to_string(),
        project_instructions,
        source_bundle,
        deck_snapshot,
        retry_present: retry.is_some(),
        retry_error: retry
            .map(|retry| excerpt(&retry.error, if compact { 1_000 } else { 2_000 }))
            .unwrap_or_default(),
        retry_invalid_response: retry
            .map(|retry| {
                excerpt(
                    &retry.invalid_response,
                    if compact { 4_000 } else { 12_000 },
                )
            })
            .unwrap_or_default(),
        compact,
        max_tool_rounds,
        ..Default::default()
    }
}

struct DraftPromptRequestContext<'a> {
    catalog: &'a dyn PromptCatalog,
    config: &'a EffectiveConfig,
    theme: &'a ThemePackage,
    instruction: &'a str,
    title: Option<&'a str>,
    source_bundle: &'a str,
    image_generation_available: bool,
    project_instructions: &'a str,
    tools: &'a [GenerationToolSummary],
    max_tool_rounds: usize,
    event_sink: Option<std::sync::Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
}

struct ReviewPromptRequestContext<'a> {
    catalog: &'a dyn PromptCatalog,
    config: &'a EffectiveConfig,
    theme: &'a ThemePackage,
    instruction: &'a str,
    source_bundle: &'a str,
    snapshot: &'a ReviewSnapshot,
    retry: Option<&'a ReviewRetryContext>,
    project_instructions: &'a str,
    max_tool_rounds: usize,
}

fn build_generation_request(args: DraftPromptRequestContext<'_>) -> Result<TextGenerationRequest> {
    let learning_style = if args.config.user.learning_style.is_empty() {
        "not specified".to_string()
    } else {
        args.config.user.learning_style.join(", ")
    };
    let context = SlidePromptContext {
        learning_style,
        project: args.config.project_name.clone(),
        project_root: args.config.project_root.display().to_string(),
        theme_name: args.theme.manifest.name.clone(),
        theme_colors: format_tokens(&args.theme.manifest.tokens.colors),
        theme_fonts: format_tokens(&args.theme.manifest.tokens.fonts),
        instruction: args.instruction.to_string(),
        project_instructions: args.project_instructions.to_string(),
        title: args.title.unwrap_or_default().to_string(),
        title_provided: args.title.is_some(),
        image_generation_available: args.image_generation_available,
        source_bundle: args.source_bundle.to_string(),
        tools: args.tools.to_vec(),
        max_tool_rounds: args.max_tool_rounds,
        ..Default::default()
    };
    render_prompt_request(args.catalog, DRAFT_PROMPTS, &context)
}

fn build_compact_generation_request(
    args: DraftPromptRequestContext<'_>,
) -> Result<TextGenerationRequest> {
    let learning_style = if args.config.user.learning_style.is_empty() {
        "not specified".to_string()
    } else {
        args.config.user.learning_style.join(", ")
    };
    let context = SlidePromptContext {
        learning_style,
        project: args.config.project_name.clone(),
        project_root: args.config.project_root.display().to_string(),
        theme_name: args.theme.manifest.name.clone(),
        theme_colors: format_tokens(&args.theme.manifest.tokens.colors),
        theme_fonts: format_tokens(&args.theme.manifest.tokens.fonts),
        instruction: args.instruction.to_string(),
        project_instructions: excerpt(args.project_instructions, 4_000),
        title: args.title.unwrap_or_default().to_string(),
        title_provided: args.title.is_some(),
        source_bundle: args.source_bundle.to_string(),
        compact: true,
        max_tool_rounds: args.max_tool_rounds,
        ..Default::default()
    };
    let mut request = render_prompt_request(args.catalog, COMPACT_DRAFT_PROMPTS, &context)?;
    request.event_sink = args.event_sink;
    Ok(request)
}

fn build_title_repair_request(
    catalog: &dyn PromptCatalog,
    config: &EffectiveConfig,
    instruction: &str,
    draft: &str,
    validation_error: &str,
    project_instructions: &str,
) -> Result<TextGenerationRequest> {
    let project_instructions = excerpt(project_instructions, 1_500);
    let headings = markdown_headings(draft);
    let headings = if headings.is_empty() {
        "No usable headings were found in the draft.".to_string()
    } else {
        headings
            .into_iter()
            .take(20)
            .map(|heading| format!("- {heading}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let context = SlidePromptContext {
        project: config.project_name.clone(),
        instruction: instruction.to_string(),
        validation_error: validation_error.to_string(),
        project_instructions,
        headings,
        max_tool_rounds: 0,
        ..Default::default()
    };
    render_prompt_request(catalog, TITLE_REPAIR_PROMPTS, &context)
}

fn build_review_request(args: ReviewPromptRequestContext<'_>) -> Result<TextGenerationRequest> {
    let snapshot = serde_json::to_string_pretty(args.snapshot)
        .context("Could not serialize slide deck review snapshot")?;
    let context = review_prompt_context(
        args.config,
        args.theme,
        args.instruction,
        args.project_instructions.to_string(),
        args.source_bundle.to_string(),
        snapshot,
        args.retry,
        false,
        args.max_tool_rounds,
    );
    render_prompt_request(args.catalog, REVIEW_PROMPTS, &context)
}

fn build_compact_review_request(
    args: ReviewPromptRequestContext<'_>,
) -> Result<TextGenerationRequest> {
    let snapshot = compact_review_snapshot(args.snapshot)?;
    let context = review_prompt_context(
        args.config,
        args.theme,
        args.instruction,
        excerpt(args.project_instructions, 4_000),
        args.source_bundle.to_string(),
        snapshot,
        args.retry,
        true,
        args.max_tool_rounds,
    );
    render_prompt_request(args.catalog, COMPACT_REVIEW_PROMPTS, &context)
}

fn compact_review_snapshot(snapshot: &ReviewSnapshot) -> Result<String> {
    let document = snapshot
        .document
        .as_object()
        .context("Slide review snapshot document must be an object")?;
    let slides = document
        .get("slides")
        .and_then(serde_json::Value::as_object)
        .context("Slide review snapshot is missing slides")?;
    let order = document
        .get("order")
        .and_then(serde_json::Value::as_array)
        .context("Slide review snapshot is missing slide order")?;
    let per_slide_budget = (30_000 / slides.len().max(1)).clamp(500, 1_800);
    let compact_slides = order
        .iter()
        .filter_map(serde_json::Value::as_str)
        .filter_map(|id| {
            slides
                .get(id)
                .and_then(serde_json::Value::as_object)
                .map(|slide| (id, slide))
        })
        .map(|(id, slide)| {
            let markdown = slide
                .get("markdown")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            serde_json::json!({
                "id": id,
                "revision": slide.get("revision"),
                "kind": slide.get("kind"),
                "heading": slide.get("heading"),
                "elements": slide.get("elements"),
                "markdown": excerpt(markdown, per_slide_budget),
                "markdown_complete": markdown.chars().count() <= per_slide_budget,
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_string_pretty(&serde_json::json!({
        "schema_version": snapshot.schema_version,
        "format": snapshot.format,
        "revision": snapshot.revision,
        "title": document.get("title"),
        "order": order,
        "slides": compact_slides,
    }))
    .context("Could not serialize compact slide review snapshot")
}

struct ReviewRetryContext {
    invalid_response: String,
    error: String,
}

fn build_layout_repair_request(
    catalog: &dyn PromptCatalog,
    context: LayoutRepairRequestContext<'_>,
    retry: Option<&LayoutRepairRetryContext>,
    event_sink: Option<std::sync::Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
) -> Result<TextGenerationRequest> {
    let issue_report =
        serde_json::to_string_pretty(context.issue).unwrap_or_else(|_| "{}".to_string());
    let prompt_context = SlidePromptContext {
        project: context.config.project_name.clone(),
        instruction: context.instruction.to_string(),
        title: context.title.to_string(),
        title_provided: true,
        theme_name: context.theme.manifest.name.clone(),
        theme_colors: format_tokens(&context.theme.manifest.tokens.colors),
        theme_fonts: format_tokens(&context.theme.manifest.tokens.fonts),
        project_instructions: context.project_instructions.to_string(),
        issue_report,
        slide_markdown: context.slide_markdown.to_string(),
        retry_present: retry.is_some(),
        retry_error: retry
            .map(|retry| excerpt(&retry.error, 4_000))
            .unwrap_or_default(),
        retry_invalid_response: retry
            .map(|retry| excerpt(&retry.invalid_response, 12_000))
            .unwrap_or_default(),
        max_tool_rounds: 0,
        ..Default::default()
    };
    let mut request = render_prompt_request(catalog, LAYOUT_REPAIR_PROMPTS, &prompt_context)?;
    request.event_sink = event_sink;
    Ok(request)
}

fn build_compact_layout_repair_request(
    catalog: &dyn PromptCatalog,
    context: LayoutRepairRequestContext<'_>,
    retry: Option<&LayoutRepairRetryContext>,
    event_sink: Option<std::sync::Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
) -> Result<TextGenerationRequest> {
    let compact_retry = retry.map(|retry| LayoutRepairRetryContext {
        invalid_response: excerpt(&retry.invalid_response, 2_000),
        error: excerpt(&retry.error, 1_000),
    });
    let compact_instructions = excerpt(context.project_instructions, 1_500);
    let issue_report =
        serde_json::to_string_pretty(context.issue).unwrap_or_else(|_| "{}".to_string());
    let prompt_context = SlidePromptContext {
        project: context.config.project_name.clone(),
        instruction: context.instruction.to_string(),
        title: context.title.to_string(),
        title_provided: true,
        theme_name: context.theme.manifest.name.clone(),
        theme_colors: format_tokens(&context.theme.manifest.tokens.colors),
        theme_fonts: format_tokens(&context.theme.manifest.tokens.fonts),
        project_instructions: compact_instructions,
        issue_report,
        slide_markdown: context.slide_markdown.to_string(),
        retry_present: compact_retry.is_some(),
        retry_error: compact_retry
            .as_ref()
            .map(|retry| retry.error.clone())
            .unwrap_or_default(),
        retry_invalid_response: compact_retry
            .as_ref()
            .map(|retry| retry.invalid_response.clone())
            .unwrap_or_default(),
        compact: true,
        max_tool_rounds: 0,
        ..Default::default()
    };
    let mut request =
        render_prompt_request(catalog, COMPACT_LAYOUT_REPAIR_PROMPTS, &prompt_context)?;
    request.event_sink = event_sink;
    Ok(request)
}

struct LayoutRepairRequestContext<'a> {
    config: &'a EffectiveConfig,
    theme: &'a ThemePackage,
    instruction: &'a str,
    title: &'a str,
    slide_markdown: &'a str,
    issue: &'a SlideLayoutIssue,
    project_instructions: &'a str,
}

struct LayoutRepairRetryContext {
    invalid_response: String,
    error: String,
}

async fn validate_mermaid_candidate(
    markdown: &str,
    theme: &ThemePackage,
    slug: &str,
) -> Result<()> {
    let temp = tempfile::tempdir().context("Could not create Mermaid validation workspace")?;
    let diagrams_dir = temp.path().join("diagrams");
    render_mermaid_diagrams(markdown, &diagrams_dir, slug, theme)
        .await
        .map(|_| ())
}

fn should_retry_model_output(error: &anyhow::Error) -> bool {
    let error = format!("{error:#}");
    !error.contains("is not installed")
        && !error.contains("Could not create Mermaid validation workspace")
}

async fn inspect_candidate_layout(
    markdown: &str,
    theme: &ThemePackage,
    slug: &str,
    browser_path: Option<&Path>,
) -> Result<Vec<SlideLayoutIssue>> {
    let temp = tempfile::tempdir().context("Could not create slide review workspace")?;
    let markdown_path = temp.path().join("review.md");
    let theme_path = temp.path().join("theme.css");
    let diagrams_dir = temp.path().join("diagrams");
    let html_path = temp.path().join("review.html");
    let (rendered, _) = render_mermaid_diagrams(markdown, &diagrams_dir, slug, theme).await?;
    copy_theme_css(theme, &theme_path)?;
    fs::write(&markdown_path, rendered)
        .with_context(|| format!("Could not write {}", markdown_path.display()))?;
    marp::inspect_layout(&markdown_path, &theme_path, &html_path, browser_path).await
}

fn layout_score(issues: &[SlideLayoutIssue]) -> (usize, u64) {
    let overflow = issues
        .iter()
        .map(|issue| {
            u64::from(issue.vertical_overflow_px) + u64::from(issue.horizontal_overflow_px)
        })
        .sum();
    (issues.len(), overflow)
}

fn model_tool_rounds(profile: &crate::config::ModelProfile) -> usize {
    profile
        .options
        .get("max_tool_rounds")
        .and_then(toml::Value::as_integer)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0)
        .unwrap_or(8)
}

fn format_tokens(tokens: &std::collections::BTreeMap<String, String>) -> String {
    tokens
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn collect_sources(inputs: &[PathBuf]) -> Result<Vec<SourceDocument>> {
    let mut documents = Vec::new();
    let mut total_bytes = 0_u64;
    let mut seen = std::collections::BTreeSet::new();

    for input in inputs {
        if input.is_file() {
            push_source_file(input, &mut documents, &mut total_bytes, &mut seen)?;
        } else if input.is_dir() {
            for entry in WalkDir::new(input) {
                let entry = entry.with_context(|| {
                    format!("Could not traverse source directory {}", input.display())
                })?;
                if entry.file_type().is_file() {
                    push_source_file(entry.path(), &mut documents, &mut total_bytes, &mut seen)?;
                }
            }
        } else {
            bail!("Input path does not exist: {}", input.display());
        }
    }

    documents.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(documents)
}

fn validate_title(title: &str) -> Result<String> {
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.is_empty() {
        bail!("Slide title cannot be empty");
    }
    if slugify(&title).is_empty() {
        bail!("Slide title must contain characters that can be used in a filename");
    }
    Ok(title)
}

fn slide_artifact_paths(slides_dir: &Path, title: &str) -> Result<(PathBuf, PathBuf)> {
    let title = validate_title(title)?;
    let slug = slugify(title);
    Ok((
        slides_dir.join(format!("{slug}.md")),
        slides_dir.join(format!("{slug}.pdf")),
    ))
}

fn extract_generated_title(generated: &str) -> Option<String> {
    let markdown = strip_code_fence(generated.trim());
    let markdown = sanitize_marp_markdown(markdown);
    let markdown = promote_marp_frontmatter(markdown);
    let markdown = body_without_frontmatter(&markdown);
    let mut code_fence: Option<(char, usize)> = None;

    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if let Some((marker, length)) = markdown_code_fence(trimmed) {
            match code_fence {
                Some((open_marker, open_length))
                    if marker == open_marker && length >= open_length =>
                {
                    code_fence = None;
                }
                None => code_fence = Some((marker, length)),
                _ => {}
            }
            continue;
        }
        if code_fence.is_some() {
            continue;
        }

        if let Some(title) = trimmed.strip_prefix("# ") {
            let title = clean_generated_title(title);
            if let Ok(title) = validate_title(&title) {
                return Some(title);
            }
        }
    }

    None
}

fn validate_draft_title(generated: &str, instruction: &str) -> Result<String> {
    let title = extract_generated_title(generated).context(
        "The drafter did not provide a title. Return a concise title as the first `# H1` on the title slide",
    )?;
    if titles_are_equivalent(&title, instruction) {
        bail!(
            "The drafter reused the instruction as the title. Generate a concise subject title instead"
        );
    }
    Ok(title)
}

fn parse_repaired_title(response: &str, instruction: &str) -> Result<String> {
    let line = strip_code_fence(response.trim())
        .lines()
        .find(|line| !line.trim().is_empty())
        .context("Title repair response was empty")?;
    let title = line
        .trim()
        .trim_start_matches('#')
        .trim()
        .trim_matches(['\'', '"', '`'])
        .trim_end_matches(['.', ':', ';'])
        .trim();
    let title = validate_title(&clean_generated_title(title))?;
    if titles_are_equivalent(&title, instruction) {
        bail!("Title repair reused the generation instruction");
    }
    Ok(title)
}

fn markdown_headings(markdown: &str) -> Vec<String> {
    let markdown = strip_code_fence(markdown.trim());
    let mut code_fence: Option<(char, usize)> = None;
    let mut headings = Vec::new();
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if let Some((marker, length)) = markdown_code_fence(trimmed) {
            match code_fence {
                Some((open_marker, open_length))
                    if marker == open_marker && length >= open_length =>
                {
                    code_fence = None;
                }
                None => code_fence = Some((marker, length)),
                _ => {}
            }
            continue;
        }
        if code_fence.is_some() {
            continue;
        }
        let heading = trimmed.trim_start_matches('#').trim();
        if trimmed.starts_with('#') && !heading.is_empty() {
            headings.push(clean_generated_title(heading));
        }
    }
    headings
}

fn clean_generated_title(title: &str) -> String {
    let mut title = title.trim().trim_end_matches('#').trim();
    loop {
        let mut changed = false;
        for delimiter in ["**", "__", "`", "*", "_"] {
            if let Some(inner) = title
                .strip_prefix(delimiter)
                .and_then(|value| value.strip_suffix(delimiter))
            {
                title = inner.trim();
                changed = true;
                break;
            }
        }
        if !changed {
            break;
        }
    }
    title.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn titles_are_equivalent(title: &str, instruction: &str) -> bool {
    let normalize = |value: &str| {
        value
            .chars()
            .filter(|character| character.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    normalize(title) == normalize(instruction)
}

fn push_source_file(
    path: &Path,
    documents: &mut Vec<SourceDocument>,
    total_bytes: &mut u64,
    seen: &mut std::collections::BTreeSet<PathBuf>,
) -> Result<()> {
    if !is_supported(path) {
        return Ok(());
    }

    let canonical = fs::canonicalize(path)
        .with_context(|| format!("Could not resolve source {}", path.display()))?;
    if !seen.insert(canonical.clone()) {
        return Ok(());
    }
    if documents.len() >= MAX_SOURCE_FILES {
        bail!(
            "Source selection exceeds the maximum of {MAX_SOURCE_FILES} supported files; select a narrower directory or explicit files"
        );
    }
    let bytes = fs::metadata(&canonical)
        .with_context(|| format!("Could not inspect {}", canonical.display()))?
        .len();
    *total_bytes = total_bytes.saturating_add(bytes.min(MAX_SOURCE_BYTES_PER_FILE));
    if *total_bytes > MAX_SOURCE_TOTAL_BYTES {
        bail!(
            "Source selection exceeds Sfumato's {} MiB preflight budget",
            MAX_SOURCE_TOTAL_BYTES / 1_048_576
        );
    }
    let mut raw = Vec::new();
    fs::File::open(&canonical)
        .with_context(|| format!("Could not open {}", canonical.display()))?
        .take(MAX_SOURCE_BYTES_PER_FILE + 1)
        .read_to_end(&mut raw)
        .with_context(|| format!("Could not read {}", canonical.display()))?;
    let truncated = raw.len() as u64 > MAX_SOURCE_BYTES_PER_FILE;
    raw.truncate(MAX_SOURCE_BYTES_PER_FILE as usize);
    while std::str::from_utf8(&raw).is_err() && !raw.is_empty() {
        raw.pop();
    }
    let mut content = String::from_utf8(raw)
        .with_context(|| format!("Source {} is not valid UTF-8", canonical.display()))?;
    if truncated {
        content.push_str("\n[...source file truncated by sfumato preflight...]");
    }
    documents.push(SourceDocument {
        path: canonical,
        content,
    });
    Ok(())
}

fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            SUPPORTED_EXTENSIONS
                .iter()
                .any(|supported| extension.eq_ignore_ascii_case(supported))
        })
        .unwrap_or(false)
}

fn build_source_bundle(documents: &[SourceDocument]) -> String {
    if documents.is_empty() {
        return "No explicit source files were supplied.".to_string();
    }
    let per_document = (MAX_SOURCE_BUNDLE_CHARS / documents.len().max(1)).clamp(500, 6_000);
    let bundle = documents
        .iter()
        .map(|document| {
            let excerpt = excerpt(&document.content, per_document);
            format!(
                "\n--- SOURCE: {} ---\n{}\n",
                document.path.display(),
                excerpt
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    excerpt(&bundle, MAX_SOURCE_BUNDLE_CHARS)
}

fn build_compact_source_bundle(documents: &[SourceDocument], max_chars: usize) -> String {
    if documents.is_empty() {
        return "No explicit source files were supplied.".to_string();
    }
    let index = documents
        .iter()
        .map(|document| format!("- {}", document.path.display()))
        .collect::<Vec<_>>()
        .join("\n");
    let index_chars = index.chars().count();
    let remaining = max_chars.saturating_sub(index_chars + 32);
    let per_document = (remaining / documents.len().max(1)).clamp(200, 1_200);
    let excerpts = documents
        .iter()
        .map(|document| {
            format!(
                "\n--- {} ---\n{}",
                document.path.display(),
                excerpt(&document.content, per_document)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    excerpt(
        &format!("Source index:\n{index}\n\nDistributed excerpts:{excerpts}"),
        max_chars,
    )
}

fn request_chars(request: &TextGenerationRequest) -> usize {
    request.system_prompt.chars().count() + request.user_prompt.chars().count()
}

fn generation_limit(error: &anyhow::Error) -> Option<&TextGenerationLimitError> {
    error.downcast_ref::<TextGenerationLimitError>()
}

fn compact_retry_failed(error: &anyhow::Error) -> bool {
    error.downcast_ref::<CompactRetryError>().is_some()
}

fn excerpt(content: &str, max_chars: usize) -> String {
    let mut excerpt = content.chars().take(max_chars).collect::<String>();
    if content.chars().count() > max_chars {
        excerpt.push_str("\n[...truncated by sfumato...]");
    }
    excerpt
}

fn normalize_marp_markdown(
    generated: &str,
    config: &EffectiveConfig,
    title: &str,
) -> Result<String> {
    let mut markdown = strip_code_fence(generated.trim()).to_string();
    markdown = sanitize_marp_markdown(&markdown);
    markdown = promote_marp_frontmatter(markdown);
    let body = body_without_frontmatter(&markdown);

    markdown = canonical_marp_document(body, &config.theme);
    markdown = remove_duplicate_leading_title_slides(markdown, title);
    markdown = constrain_generated_images(&markdown);

    if !markdown.contains("\n---") {
        bail!("Generated deck does not contain Marp slide separators.");
    }

    markdown = ensure_title_slide(markdown, title)?;

    Ok(markdown)
}

fn validate_normalized_deck(markdown: &str, title: &str) -> Result<()> {
    SlideDeckDocument::from_marp(markdown, title)
        .context("Generated slide deck is invalid after normalization")?;
    Ok(())
}

fn canonical_marp_document(body: &str, theme: &str) -> String {
    format!(
        "---\nmarp: true\ntheme: {theme}\npaginate: true\nmath: mathjax\n---\n\n{}",
        body.trim()
    )
}

fn insert_title_slide(markdown: String, title: &str) -> String {
    let fences = markdown_fences(&markdown);
    if fences.len() < 2 || fences[0].start != 0 {
        return format!("# {title}\n\n---\n\n{markdown}");
    }

    format!(
        "{}\n\n# {title}\n\n---\n\n{}",
        markdown[..fences[1].end].trim_end(),
        markdown[fences[1].end..].trim_start()
    )
}

fn ensure_title_slide(mut markdown: String, title: &str) -> Result<String> {
    let first_slide = slide_ranges(&markdown)?
        .into_iter()
        .next()
        .context("Generated deck does not contain a title slide")?;
    if let Some((start, end)) = first_h1_range(&markdown[first_slide.start..first_slide.end]) {
        markdown.replace_range(
            first_slide.start + start..first_slide.start + end,
            &format!("# {title}"),
        );
        Ok(markdown)
    } else {
        Ok(insert_title_slide(markdown, title))
    }
}

fn first_h1_range(markdown: &str) -> Option<(usize, usize)> {
    let mut cursor = 0;
    let mut code_fence: Option<(char, usize)> = None;
    for line in markdown.split_inclusive('\n') {
        let without_newline = line.trim_end_matches(['\n', '\r']);
        let trimmed = without_newline.trim_start();
        if let Some((marker, length)) = markdown_code_fence(trimmed) {
            match code_fence {
                Some((open_marker, open_length))
                    if marker == open_marker && length >= open_length =>
                {
                    code_fence = None;
                }
                None => code_fence = Some((marker, length)),
                _ => {}
            }
        } else if code_fence.is_none() && trimmed.starts_with("# ") {
            let indentation = without_newline.len() - trimmed.len();
            return Some((cursor + indentation, cursor + without_newline.len()));
        }
        cursor += line.len();
    }
    None
}

#[derive(Clone, Copy)]
struct Fence {
    start: usize,
    end: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SlideRange {
    number: usize,
    start: usize,
    end: usize,
}

#[derive(Clone, Debug)]
struct SlideReplacement {
    range: SlideRange,
    markdown: String,
}

fn promote_marp_frontmatter(markdown: String) -> String {
    let fences = markdown_fences(&markdown);
    let Some(frontmatter_index) = fences.windows(2).position(|window| {
        frontmatter_contains_key(&markdown[window[0].end..window[1].start], "marp")
    }) else {
        return markdown;
    };
    if frontmatter_index == 0 {
        return markdown;
    }

    let frontmatter = markdown[fences[frontmatter_index].start..fences[frontmatter_index + 1].end]
        .trim()
        .to_string();
    let prefix_body = if fences[0].start == 0 {
        markdown[fences[0].end..fences[frontmatter_index].start]
            .trim()
            .to_string()
    } else {
        markdown[..fences[frontmatter_index].start]
            .trim()
            .to_string()
    };
    let suffix_body = markdown[fences[frontmatter_index + 1].end..].trim_start();
    let suffix_body = if !prefix_body.is_empty()
        && !suffix_body.is_empty()
        && !suffix_body.trim_start().starts_with("---")
    {
        format!("---\n\n{suffix_body}")
    } else {
        suffix_body.to_string()
    };

    [frontmatter, prefix_body, suffix_body]
        .into_iter()
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn markdown_fences(markdown: &str) -> Vec<Fence> {
    let mut fences = Vec::new();
    let mut cursor = 0;
    let mut code_fence: Option<(char, usize)> = None;

    for line in markdown.split_inclusive('\n') {
        let line_without_newline = line.trim_end_matches('\n').trim_end_matches('\r');
        let trimmed = line_without_newline.trim_start();
        if let Some((marker, length)) = markdown_code_fence(trimmed) {
            match code_fence {
                Some((open_marker, open_length))
                    if marker == open_marker && length >= open_length =>
                {
                    code_fence = None;
                }
                None => code_fence = Some((marker, length)),
                _ => {}
            }
        } else if code_fence.is_none() && line_without_newline.trim() == "---" {
            fences.push(Fence {
                start: cursor,
                end: cursor + line.len(),
            });
        }
        cursor += line.len();
    }

    if cursor < markdown.len() && markdown[cursor..].trim() == "---" {
        fences.push(Fence {
            start: cursor,
            end: markdown.len(),
        });
    }

    fences
}

fn markdown_code_fence(line: &str) -> Option<(char, usize)> {
    let marker = line.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let length = line
        .chars()
        .take_while(|character| *character == marker)
        .count();
    (length >= 3).then_some((marker, length))
}

fn slide_ranges(markdown: &str) -> Result<Vec<SlideRange>> {
    let fences = markdown_fences(markdown);
    if fences.len() < 2
        || fences[0].start != 0
        || !frontmatter_contains_key(&markdown[fences[0].end..fences[1].start], "marp")
    {
        bail!("Cannot locate canonical Marp frontmatter for slide replacement");
    }

    let mut ranges = Vec::new();
    let mut start = fences[1].end;
    for (index, separator) in fences.iter().skip(2).enumerate() {
        ranges.push(SlideRange {
            number: index + 1,
            start,
            end: separator.start,
        });
        start = separator.end;
    }
    ranges.push(SlideRange {
        number: ranges.len() + 1,
        start,
        end: markdown.len(),
    });
    Ok(ranges)
}

fn normalize_slide_replacement(generated: &str) -> Result<String> {
    let mut fragment = strip_code_fence(generated.trim()).trim().to_string();
    fragment = sanitize_marp_markdown(&fragment).trim().to_string();

    let fences = markdown_fences(&fragment);
    if fences.len() >= 2
        && fences[0].start == 0
        && frontmatter_contains_key(&fragment[fences[0].end..fences[1].start], "marp")
    {
        fragment = fragment[fences[1].end..].trim_start().to_string();
    }

    fragment = trim_outer_slide_separators(&fragment).to_string();
    fragment = constrain_generated_images(&fragment);
    if fragment.trim().is_empty() {
        bail!("Reviewer returned an empty slide replacement");
    }
    Ok(fragment.trim().to_string())
}

fn trim_outer_slide_separators(fragment: &str) -> &str {
    let mut fragment = fragment.trim();
    loop {
        let fences = markdown_fences(fragment);
        if fences.first().is_some_and(|fence| fence.start == 0) {
            fragment = fragment[fences[0].end..].trim_start();
            continue;
        }
        if fences
            .last()
            .is_some_and(|fence| fence.end == fragment.len())
        {
            fragment = fragment[..fences.last().expect("checked above").start].trim_end();
            continue;
        }
        return fragment;
    }
}

fn apply_slide_replacements(markdown: &str, mut replacements: Vec<SlideReplacement>) -> String {
    replacements.sort_by_key(|replacement| std::cmp::Reverse(replacement.range.start));
    let mut repaired = markdown.to_string();
    for replacement in replacements {
        repaired.replace_range(
            replacement.range.start..replacement.range.end,
            &format!("\n\n{}\n\n", replacement.markdown.trim()),
        );
    }
    repaired
}

fn frontmatter_contains_key(frontmatter: &str, key: &str) -> bool {
    let prefix = format!("{key}:");
    frontmatter
        .lines()
        .any(|line| line.trim_start().starts_with(&prefix))
}

fn body_without_frontmatter(markdown: &str) -> &str {
    let fences = markdown_fences(markdown);
    if fences.len() >= 2 && fences[0].start == 0 {
        markdown[fences[1].end..].trim_start()
    } else {
        markdown.trim()
    }
}

fn remove_duplicate_leading_title_slides(markdown: String, title: &str) -> String {
    let mut markdown = markdown;

    loop {
        let fences = markdown_fences(&markdown);
        if fences.len() < 3 || fences[0].start != 0 {
            return markdown;
        }

        let first_slide = markdown[fences[1].end..fences[2].start].trim();
        if !is_only_title_slide(first_slide, title) {
            return markdown;
        }

        let remaining = markdown[fences[2].end..].trim_start();
        if !remaining_starts_with_title_slide(remaining, title) {
            return markdown;
        }

        markdown = format!("{}\n\n{}", markdown[..fences[1].end].trim_end(), remaining);
    }
}

fn is_only_title_slide(slide: &str, title: &str) -> bool {
    slide
        .strip_prefix("# ")
        .map(|heading| heading.trim().eq_ignore_ascii_case(title))
        .unwrap_or(false)
}

fn remaining_starts_with_title_slide(remaining: &str, title: &str) -> bool {
    let first_slide = remaining
        .split_once("\n---")
        .map(|(first, _)| first)
        .unwrap_or(remaining);
    first_slide.lines().any(|line| {
        line.trim_start_matches("# ")
            .trim()
            .eq_ignore_ascii_case(title)
    }) || first_slide.contains("<!-- _class: title -->")
}

fn sanitize_marp_markdown(markdown: &str) -> String {
    let without_svg = strip_html_blocks(markdown, "svg");
    let without_style = strip_html_blocks(&without_svg, "style");
    let without_css_fences =
        strip_code_blocks_by_language(&without_style, &["css", "scss", "sass"]);
    remove_html_tags_by_names(
        &without_css_fences,
        &[
            "article", "div", "section", "span", "p", "br", "svg", "style",
        ],
    )
}

fn constrain_generated_images(markdown: &str) -> String {
    let mut output = String::with_capacity(markdown.len());
    let mut cursor = 0;

    while let Some(relative_start) = markdown[cursor..].find("![") {
        let image_start = cursor + relative_start;
        output.push_str(&markdown[cursor..image_start]);

        let alt_start = image_start + 2;
        let Some(relative_alt_end) = markdown[alt_start..].find("](") else {
            output.push_str(&markdown[image_start..]);
            return output;
        };
        let alt_end = alt_start + relative_alt_end;
        let target_start = alt_end + 2;
        let Some(relative_target_end) = markdown[target_start..].find(')') else {
            output.push_str(&markdown[image_start..]);
            return output;
        };
        let target_end = target_start + relative_target_end;
        let alt = &markdown[alt_start..alt_end];
        let target = markdown[target_start..target_end].trim();

        if is_generated_image_target(target) && !has_marp_image_layout(alt) {
            output.push_str(&format!(
                "![height:{GENERATED_IMAGE_MARP_HEIGHT}]({target})"
            ));
        } else {
            output.push_str(&markdown[image_start..=target_end]);
        }
        cursor = target_end + 1;
    }

    output.push_str(&markdown[cursor..]);
    output
}

fn is_generated_image_target(target: &str) -> bool {
    target.starts_with("images/") || target.starts_with("./images/")
}

fn has_marp_image_layout(alt: &str) -> bool {
    alt.split_whitespace().any(|part| {
        let option = part.to_ascii_lowercase();
        option == "bg"
            || option.starts_with("bg:")
            || ["width:", "height:", "w:", "h:"]
                .iter()
                .any(|prefix| option.starts_with(prefix))
    })
}

fn strip_code_blocks_by_language(markdown: &str, languages: &[&str]) -> String {
    let mut output = String::new();
    let mut cursor = 0;

    while let Some(relative_start) = markdown[cursor..].find("```") {
        let fence_start = cursor + relative_start;
        output.push_str(&markdown[cursor..fence_start]);

        let after_ticks = fence_start + 3;
        let line_end = markdown[after_ticks..]
            .find('\n')
            .map(|offset| after_ticks + offset)
            .unwrap_or(markdown.len());
        let language = markdown[after_ticks..line_end].trim();

        if !languages
            .iter()
            .any(|candidate| language.eq_ignore_ascii_case(candidate))
        {
            output.push_str(&markdown[fence_start..line_end]);
            cursor = line_end;
            continue;
        }

        let content_start = if line_end < markdown.len() {
            line_end + 1
        } else {
            line_end
        };
        if let Some(relative_end) = markdown[content_start..].find("\n```") {
            cursor = content_start + relative_end + "\n```".len();
        } else {
            cursor = markdown.len();
        }
    }

    output.push_str(&markdown[cursor..]);
    output
}

fn strip_html_blocks(markdown: &str, tag_name: &str) -> String {
    let mut output = String::new();
    let mut cursor = 0;
    let lower = markdown.to_lowercase();
    let opening = format!("<{}", tag_name.to_lowercase());
    let closing = format!("</{}>", tag_name.to_lowercase());

    while let Some(relative_start) = lower[cursor..].find(&opening) {
        let start = cursor + relative_start;
        output.push_str(&markdown[cursor..start]);

        let after_start = start + opening.len();
        let is_tag_boundary = lower[after_start..]
            .chars()
            .next()
            .map(|next| next.is_whitespace() || next == '>' || next == '/')
            .unwrap_or(false);
        if !is_tag_boundary {
            output.push_str(&markdown[start..after_start]);
            cursor = after_start;
            continue;
        }

        if let Some(relative_end) = lower[after_start..].find(&closing) {
            cursor = after_start + relative_end + closing.len();
        } else if let Some(relative_tag_end) = lower[after_start..].find('>') {
            cursor = after_start + relative_tag_end + 1;
        } else {
            cursor = markdown.len();
        }
    }

    output.push_str(&markdown[cursor..]);
    output
}

fn remove_html_tags_by_names(markdown: &str, tag_names: &[&str]) -> String {
    let mut output = String::new();
    let mut cursor = 0;

    while let Some(relative_start) = markdown[cursor..].find('<') {
        let start = cursor + relative_start;
        output.push_str(&markdown[cursor..start]);

        let Some(relative_end) = markdown[start..].find('>') else {
            output.push_str(&markdown[start..]);
            return output;
        };
        let end = start + relative_end + 1;
        let tag = &markdown[start + 1..end - 1];

        if is_named_html_tag(tag, tag_names) {
            cursor = end;
        } else {
            output.push_str(&markdown[start..end]);
            cursor = end;
        }
    }

    output.push_str(&markdown[cursor..]);
    output
}

fn is_named_html_tag(tag: &str, tag_names: &[&str]) -> bool {
    let tag = tag.trim_start().trim_start_matches('/').trim_start();
    let name = tag
        .split(|character: char| character.is_whitespace() || character == '/' || character == '>')
        .next()
        .unwrap_or_default();
    tag_names
        .iter()
        .any(|candidate| name.eq_ignore_ascii_case(candidate))
}

fn strip_code_fence(text: &str) -> &str {
    let text = text.trim();
    for marker in ["```", "~~~"] {
        let Some(after_marker) = text.strip_prefix(marker) else {
            continue;
        };
        let Some(opening_line_end) = after_marker.find('\n') else {
            continue;
        };
        let language = after_marker[..opening_line_end].trim();
        let body = &after_marker[opening_line_end + 1..];
        if is_markdown_document_language(language) {
            return strip_optional_document_closing_fence(body, marker);
        }
        let Some(body) = body.trim_end().strip_suffix(marker) else {
            continue;
        };
        return body.trim();
    }
    text
}

fn is_markdown_document_language(language: &str) -> bool {
    matches!(
        language.to_ascii_lowercase().as_str(),
        "marp" | "markdown" | "md"
    )
}

fn strip_optional_document_closing_fence<'a>(body: &'a str, marker: &str) -> &'a str {
    let marker_character = marker.chars().next().unwrap_or('`');
    let marker_count = body
        .lines()
        .filter_map(|line| markdown_code_fence(line.trim_start()))
        .filter(|(character, length)| *character == marker_character && *length >= marker.len())
        .count();
    let body = body.trim();

    if marker_count % 2 == 1 {
        body.strip_suffix(marker).map(str::trim_end).unwrap_or(body)
    } else {
        body
    }
}

fn ensure_inside(root: &Path, path: &Path) -> Result<()> {
    if !path.starts_with(root) {
        bail!(
            "Refusing to write {} because it is outside {}",
            path.display(),
            root.display()
        );
    }

    Ok(())
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../../tests/unit/resources_slides.rs"]
mod tests;
