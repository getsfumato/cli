//! Sectioned document drafting, review, format repair, and PDF rendering.
//!
//! The same shape as the slide workflow, with one deliberate difference. A slide
//! is a fixed box, so its repair stage chases overflow. Prose reflows across
//! pages on its own, so this workflow's repair stage chases the defects that
//! survive pagination instead: content wider than the text column, a block that
//! cannot fit any page, a heading stranded at the foot of one.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use serde::Serialize;
use sfumato_domain::ArtifactKind;
use slug::slugify;

use crate::resources::excerpt;
use crate::sfumato_bail as bail;
use crate::{
    artifacts::{
        ArtifactResourceKind, ArtifactStore, ResourceArtifactFile, ResourceArtifactManifest,
    },
    config::{Capability, EffectiveConfig, GenerationToolKind, ModelRole},
    errors::{
        ErrorClass, ErrorCode, OperationStage, ResultContext as Context, SfumatoError,
        SfumatoResult as Result,
    },
    filesystem::WorkspaceFileSystem,
    generation::{
        DocumentFormatIssue, DocumentGenerationOutput, DocumentPageSetup, DocumentPageSize,
        DocumentReviewSummary, GenerationRequest, GenerationToolSummary, ReviewStatus,
    },
    operation::{OperationContext, OperationEventKind},
    project_assets::{ProjectAssetCatalog, ProjectAssetReference},
    prompts::{PromptCatalog, PromptId, PromptProvenance, PromptRenderRequest, PromptVariables},
    providers::{
        GenerationStage, ImageGenerationProvider, ProviderFactory, TextGenerationEvent,
        TextGenerationProvider, TextGenerationRequest,
    },
    python::PythonRuntime,
    renderers::{
        DiagramRenderer, DocumentAssembler, DocumentAssemblyRequest, DocumentRenderRequest,
        DocumentRenderer,
    },
    repositories::ThemeRepository,
    review::parse_json_patch,
    sources::SourceReader,
    templates::GenerationTemplate,
    themes::ThemePackage,
    tools::{
        ChartToolConfig, GenerationToolFactory, GenerationToolsRequest, ImageToolConfig,
        chart_tool_gate_warning,
    },
};

use super::{
    DryRunImageProvider,
    project_assets::{
        PrepareProjectAssetsRequest, prepare_project_assets, retain_referenced_generated_assets,
    },
    slides::mermaid::{MermaidRenderRequest, extract_mermaid_blocks, render_mermaid_diagrams},
};

mod format;
mod prompting;

use format::FormatAssessment;
use prompting::*;

/// How many focused format repairs one document may spend.
///
/// Each repair costs a model call plus a re-inspection, and defects cluster:
/// after a handful the remaining ones are usually the same wide table reported
/// on consecutive pages.
const MAX_FORMAT_REPAIRS: usize = 6;
const MAX_SOURCE_BUNDLE_CHARS: usize = 48_000;

pub(crate) struct GenerateDocumentOptions {
    pub operation: OperationContext,
    pub title: Option<String>,
    pub template: Option<GenerationTemplate>,
    pub dry_run: bool,
    pub review: bool,
    pub page_size: Option<DocumentPageSize>,
    pub table_of_contents: Option<bool>,
    pub cover: Option<bool>,
    pub event_sink: Option<Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
    pub prompt_catalog: Arc<dyn PromptCatalog>,
    pub artifact_store: Arc<dyn ArtifactStore>,
    pub provider_factory: Arc<dyn ProviderFactory>,
    pub diagram_renderer: Arc<dyn DiagramRenderer>,
    pub document_assembler: Arc<dyn DocumentAssembler>,
    pub document_renderer: Arc<dyn DocumentRenderer>,
    pub source_reader: Arc<dyn SourceReader>,
    pub tool_factory: Arc<dyn GenerationToolFactory>,
    /// Managed Python environments backing the local charting tool.
    pub python_runtime: Arc<dyn PythonRuntime>,
    pub theme_repository: Arc<dyn ThemeRepository>,
    pub workspace: Arc<dyn WorkspaceFileSystem>,
    pub project_asset_catalog: Arc<dyn ProjectAssetCatalog>,
}

/// Complete document-generation result returned to presentation frontends.
#[derive(Debug)]
pub struct GenerateDocumentResult {
    /// Committed or planned Markdown source path.
    pub markdown_path: PathBuf,
    /// Committed PDF path, absent during a dry run.
    pub pdf_path: Option<PathBuf>,
    /// Published PDF path when `--out` was supplied.
    pub published_pdf_path: Option<PathBuf>,
    /// Machine-readable operation output.
    pub output: DocumentGenerationOutput,
    /// Rendered drafting prompt during a dry run.
    pub prompt_preview: Option<String>,
    /// Tools offered to the drafter.
    pub tool_summaries: Vec<GenerationToolSummary>,
    /// Non-fatal workflow warnings.
    pub warnings: Vec<String>,
}

/// Context shared by every document prompt in one run.
#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct DocumentPromptContext {
    pub learning_style: String,
    pub project: String,
    pub project_root: String,
    pub theme_name: String,
    pub theme_colors: String,
    pub theme_fonts: String,
    pub instruction: String,
    pub project_instructions: String,
    pub source_bundle: String,
    pub title: String,
    pub title_provided: bool,
    pub page_size: String,
    pub table_of_contents: bool,
    pub reusable_assets: Vec<ProjectAssetReference>,
    pub image_generation_available: bool,
    pub chart_generation_available: bool,
    pub template_enabled: bool,
    pub template_name: String,
    pub template_source: String,
    pub max_tool_rounds: usize,
    pub document_snapshot: String,
    pub draft_markdown: String,
    pub validation_error: String,
    pub diagram_error: String,
    pub section_markdown: String,
    pub issue: Option<DocumentFormatIssue>,
    pub retry_present: bool,
    pub retry_error: Option<String>,
    pub retry_invalid_response: Option<String>,
}

pub(crate) async fn generate_document(
    config: EffectiveConfig,
    request: GenerationRequest,
    options: GenerateDocumentOptions,
) -> Result<GenerateDocumentResult> {
    let GenerateDocumentOptions {
        operation,
        title: title_override,
        template,
        dry_run,
        review,
        page_size,
        table_of_contents,
        cover,
        event_sink,
        prompt_catalog,
        artifact_store,
        provider_factory,
        diagram_renderer,
        document_assembler,
        document_renderer,
        source_reader,
        tool_factory,
        python_runtime,
        theme_repository,
        workspace,
        project_asset_catalog,
    } = options;
    operation.checkpoint(OperationStage::Resolve)?;
    let publish_root = config.publish_root()?;
    let theme = theme_repository.load(&config.theme)?;
    let setup = resolve_page_setup(&theme, page_size, table_of_contents, cover)?;

    let mut transaction = if dry_run {
        None
    } else {
        Some(artifact_store.begin(&config.project_name, ArtifactResourceKind::Documents)?)
    };
    let documents_dir = transaction
        .as_ref()
        .map(|value| value.staging_root().to_path_buf())
        .unwrap_or_else(|| {
            artifact_store
                .project_root(&config.project_name)
                .unwrap_or_else(|_| PathBuf::from(".sfumato"))
                .join("resources/documents/dry-run")
        });
    // The cover date comes from the revision, never the clock: the same revision
    // has to reproduce the same PDF.
    let revision_date = transaction
        .as_ref()
        .map(|value| revision_date(value.revision_id().as_str()))
        .unwrap_or_else(|| "dry-run".to_owned());
    let diagrams_dir = documents_dir.join("diagrams");
    let images_dir = documents_dir.join("images");

    operation.emit(
        OperationStage::ReadSources,
        OperationEventKind::Started,
        BTreeMap::new(),
    );
    operation.checkpoint(OperationStage::ReadSources)?;
    let project_instructions = source_reader.project_instructions(&config.project_root)?;
    let project_instructions_prompt = project_instructions
        .as_ref()
        .map(|value| value.content.clone())
        .unwrap_or_default();
    let project_instructions_path = project_instructions
        .as_ref()
        .map(|value| value.path.clone());
    let documents = source_reader.collect(&request.sources)?;
    operation.emit(
        OperationStage::ReadSources,
        OperationEventKind::Completed,
        BTreeMap::from([("documents".to_string(), documents.len().to_string())]),
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
        project_instructions: &project_instructions_prompt,
        output_dir: &images_dir,
        reference_prefix: "images",
        dry_run,
        operation: &operation,
    })
    .await?;
    let reusable_assets = prepared_assets.references();
    let image_tool = image_selection.map(|(profile_name, _)| ImageToolConfig {
        provider: image_provider
            .as_ref()
            .expect("image provider resolved above")
            .clone(),
        profile_name: profile_name.to_string(),
        output_dir: images_dir.clone(),
        reference_prefix: "images".into(),
        theme: theme.clone(),
        project_instructions: project_instructions.as_ref().map(|v| v.content.clone()),
    });
    // Resolved before the prompt context, because whether the model is told it can plot
    // depends on whether it actually can. The tool was reaching the factory and never
    // the prose, which left it announced only in the tool schema.
    let chart_tool = ChartToolConfig::enable(
        &config,
        python_runtime.clone(),
        images_dir.clone(),
        "images",
        &theme,
        project_instructions
            .as_ref()
            .map(|value| value.content.clone()),
        false,
    );
    let chart_generation_available = chart_tool.is_some();
    let tool_set = tool_factory.create(GenerationToolsRequest {
        project_root: config.project_root.clone(),
        sources: request.sources.clone(),
        image: image_tool,
        video: None,
        // Neither a deck nor a printable document has a timeline to hang audio
        // on, so speech is not offered here.
        audio: None,
        chart: chart_tool,
        prompt_catalog: prompt_catalog.clone(),
    })?;
    let tool_summaries = summarize_tools(&tool_set.definitions);
    let review_tool_definitions = tool_set
        .definitions
        .iter()
        .filter(|tool| tool.function.name != "sfumato_image_gen")
        .cloned()
        .collect::<Vec<_>>();

    let (draft_profile_name, draft_profile) = config.resolve_model(Capability::Text)?;
    let reviewer_selection = review
        .then(|| config.resolve_model_role(ModelRole::Reviewer))
        .transpose()?;
    let mut models = BTreeMap::from([("text".to_string(), draft_profile_name.to_string())]);
    if let Some((name, _)) = reviewer_selection {
        models.insert("reviewer".to_string(), name.to_string());
    }
    if let Some((name, _)) = image_selection {
        models.insert("image".to_string(), name.to_string());
    }

    let title_override = title_override.map(validate_title).transpose()?;
    let mut context = DocumentPromptContext {
        learning_style: config.user.learning_style.join(", "),
        project: config.project_name.clone(),
        project_root: config.project_root.display().to_string(),
        theme_name: theme.manifest.name.clone(),
        theme_colors: format_tokens(&theme.manifest.tokens.colors),
        theme_fonts: format_tokens(&theme.manifest.tokens.fonts),
        instruction: request.instruction.clone(),
        project_instructions: project_instructions_prompt.clone(),
        source_bundle: build_source_bundle(&documents, MAX_SOURCE_BUNDLE_CHARS),
        title: title_override.clone().unwrap_or_default(),
        title_provided: title_override.is_some(),
        page_size: setup.page_size.as_str().to_string(),
        table_of_contents: setup.table_of_contents,
        reusable_assets: reusable_assets.clone(),
        image_generation_available: image_selection.is_some(),
        chart_generation_available,
        template_enabled: template.is_some(),
        template_name: template
            .as_ref()
            .map(|value| value.manifest.name.clone())
            .unwrap_or_default(),
        template_source: template
            .as_ref()
            .map(|value| value.source.clone())
            .unwrap_or_default(),
        max_tool_rounds: draft_profile.options.tool_rounds(),
        ..Default::default()
    };

    let mut review_summary = if review {
        DocumentReviewSummary::enabled()
    } else {
        DocumentReviewSummary::disabled()
    };
    let mut warnings = prepared_assets.warnings.clone();
    // A charting tool the project enabled but the Python gate withheld leaves no
    // other trace: the resource simply comes back without charts.
    warnings.extend(chart_tool_gate_warning(&config, false));
    let mut prompts = prepared_assets.prompts.clone();

    let mut draft_request = render_pair(
        prompt_catalog.as_ref(),
        PromptId::DocumentDraftSystem,
        PromptId::DocumentDraftUser,
        &context,
    )?;
    draft_request.tools = tool_set.definitions.clone();
    draft_request.tool_executor = Some(tool_set.executor.clone());
    draft_request.event_sink = event_sink.clone();
    prompts.extend(draft_request.prompt_provenance.clone());

    if dry_run {
        let planned_title = title_override
            .clone()
            .unwrap_or_else(|| "model-generated-title".into());
        return Ok(GenerateDocumentResult {
            markdown_path: documents_dir.join(format!("{}.md", slugify(&planned_title))),
            pdf_path: None,
            published_pdf_path: None,
            output: DocumentGenerationOutput {
                project: config.project_name,
                project_instructions: project_instructions_path,
                models,
                tools: tool_summaries.clone(),
                template: template.as_ref().map(|v| v.manifest.name.clone()),
                project_assets: reusable_assets,
                artifacts: Vec::new(),
                published_artifacts: Vec::new(),
                review: review_summary,
                page_setup: setup,
                runtimes: Vec::new(),
                prompts,
            },
            prompt_preview: Some(draft_request.user_prompt),
            tool_summaries,
            warnings,
        });
    }

    emit_stage(
        &event_sink,
        GenerationStage::DocumentDraft,
        Some(draft_profile_name),
    );
    operation.emit(
        OperationStage::Draft,
        OperationEventKind::Started,
        BTreeMap::from([("model".to_string(), draft_profile_name.to_string())]),
    );
    let provider = provider_factory.text(&config, draft_profile)?;
    let compact_request = render_pair(
        prompt_catalog.as_ref(),
        PromptId::DocumentCompactDraftSystem,
        PromptId::DocumentCompactDraftUser,
        &context,
    )?;
    let compact_provenance = compact_request.prompt_provenance.clone();
    let outcome = generate_with_compact_retry(
        provider.as_ref(),
        draft_request,
        compact_request,
        &operation,
        OperationStage::Draft,
    )
    .await
    .context("Document draft generation failed")?;
    if let Some(error) = outcome.limit_error {
        prompts.extend(compact_provenance);
        warnings.push(format!(
            "Draft generation exceeded the model limit and was retried with compacted context: {error}"
        ));
    }
    let mut markdown = strip_markdown_fence(&outcome.response.text);
    if let Some(template) = &template {
        markdown = template.compose(&markdown)?;
    }
    let generated_tool_assets = tool_set.generated_artifacts()?;
    operation.emit(
        OperationStage::Draft,
        OperationEventKind::Completed,
        BTreeMap::new(),
    );

    // Parse into the domain model, repairing the structure once if it is invalid.
    // Everything downstream — review, repair, assembly — needs a valid document,
    // so this is the boundary where a malformed draft has to be fixed or fail.
    let mut document = match parse_document(&markdown, title_override.as_deref()) {
        Ok(document) => document,
        Err(error) => {
            let message = format!("{error:#}");
            emit_stage(
                &event_sink,
                GenerationStage::DocumentValidationRepair,
                Some(draft_profile_name),
            );
            context.validation_error = message.clone();
            context.draft_markdown = excerpt(&markdown, 24_000);
            context.title = title_override.clone().unwrap_or_default();
            let mut repair = render_pair(
                prompt_catalog.as_ref(),
                PromptId::DocumentValidationRepairSystem,
                PromptId::DocumentValidationRepairUser,
                &context,
            )?;
            repair.event_sink = event_sink.clone();
            prompts.extend(repair.prompt_provenance.clone());
            let repaired = provider
                .generate_text(repair, &operation, OperationStage::Repair)
                .await
                .context("The drafter could not repair the invalid document")?;
            warnings.push(format!(
                "The document draft was invalid and was repaired once: {message}"
            ));
            parse_document(
                &strip_markdown_fence(&repaired.text),
                title_override.as_deref(),
            )
            .context("The drafter returned an invalid document after one focused repair")?
        }
    };

    let title = document.title().to_string();
    let slug = slugify(&title);
    if slug.is_empty() {
        bail!("Document title cannot produce an artifact name");
    }
    context.title = title.clone();
    let markdown_path = documents_dir.join(format!("{slug}.md"));
    let html_name = format!("{slug}.html");
    let pdf_name = format!("{slug}.pdf");
    let html_path = documents_dir.join(&html_name);
    let pdf_path = documents_dir.join(&pdf_name);
    ensure_inside(&documents_dir, &markdown_path)?;
    ensure_inside(&documents_dir, &pdf_path)?;

    // Mermaid is validated before review so the reviewer never patches around a
    // diagram that was never going to render.
    if let Err(error) = validate_mermaid(
        &document,
        &theme,
        diagram_renderer.as_ref(),
        config.marp.browser_path.as_deref(),
        workspace.as_ref(),
        &operation,
    )
    .await
    {
        let message = format!("{error:#}");
        emit_stage(
            &event_sink,
            GenerationStage::DocumentDiagramRepair,
            Some(draft_profile_name),
        );
        context.diagram_error = message.clone();
        context.document_snapshot = snapshot_json(&document)?;
        let mut repair = render_pair(
            prompt_catalog.as_ref(),
            PromptId::DocumentMermaidRepairSystem,
            PromptId::DocumentMermaidRepairUser,
            &context,
        )?;
        repair.event_sink = event_sink.clone();
        prompts.extend(repair.prompt_provenance.clone());
        match provider
            .generate_text(repair, &operation, OperationStage::Repair)
            .await
            .and_then(|response| {
                let patch = parse_json_patch(&response.text).map_err(domain_error)?;
                let mut candidate = document.clone();
                candidate.apply_patch(&patch).map_err(domain_error)?;
                Ok(candidate)
            }) {
            Ok(candidate) => {
                validate_mermaid(
                    &candidate,
                    &theme,
                    diagram_renderer.as_ref(),
                    config.marp.browser_path.as_deref(),
                    workspace.as_ref(),
                    &operation,
                )
                .await
                .context("The repaired Mermaid diagram is still invalid")?;
                document = candidate;
                warnings.push(format!("A Mermaid diagram was repaired once: {message}"));
            }
            Err(error) => {
                return Err(error).context("The drafter could not repair the Mermaid diagram");
            }
        }
    }

    if let Some((reviewer_name, reviewer_profile)) = reviewer_selection {
        match provider_factory.text(&config, reviewer_profile) {
            Ok(reviewer) => {
                emit_stage(
                    &event_sink,
                    GenerationStage::DocumentReview,
                    Some(reviewer_name),
                );
                context.document_snapshot = snapshot_json(&document)?;
                context.max_tool_rounds = reviewer_profile.options.tool_rounds();
                let mut review_request = render_pair(
                    prompt_catalog.as_ref(),
                    PromptId::DocumentReviewSystem,
                    PromptId::DocumentReviewUser,
                    &context,
                )?;
                review_request.tools = review_tool_definitions.clone();
                review_request.tool_executor = Some(tool_set.executor.clone());
                review_request.event_sink = event_sink.clone();
                prompts.extend(review_request.prompt_provenance.clone());
                let compact_review = render_pair(
                    prompt_catalog.as_ref(),
                    PromptId::DocumentCompactReviewSystem,
                    PromptId::DocumentCompactReviewUser,
                    &context,
                )?;
                let compact_review_provenance = compact_review.prompt_provenance.clone();
                operation.checkpoint(OperationStage::Review)?;
                match generate_with_compact_retry(
                    reviewer.as_ref(),
                    review_request,
                    compact_review,
                    &operation,
                    OperationStage::Review,
                )
                .await
                {
                    Ok(outcome) => {
                        if outcome.limit_error.is_some() {
                            prompts.extend(compact_review_provenance);
                            review_summary.context_compaction = ReviewStatus::Completed;
                        }
                        match apply_review(&document, &outcome.response.text) {
                            Ok(candidate) => {
                                match validate_mermaid(
                                    &candidate,
                                    &theme,
                                    diagram_renderer.as_ref(),
                                    config.marp.browser_path.as_deref(),
                                    workspace.as_ref(),
                                    &operation,
                                )
                                .await
                                {
                                    Ok(()) => {
                                        document = candidate;
                                        review_summary.semantic_review = ReviewStatus::Completed;
                                    }
                                    Err(error) => {
                                        review_summary.semantic_review = ReviewStatus::Failed;
                                        warnings.push(format!(
                                            "Document review broke a Mermaid diagram; using the validated draft: {error:#}"
                                        ));
                                    }
                                }
                            }
                            Err(error) => {
                                review_summary.semantic_review = ReviewStatus::Failed;
                                warnings.push(format!(
                                    "Document review failed; using the validated draft: {error:#}"
                                ));
                            }
                        }
                    }
                    Err(error) => {
                        review_summary.semantic_review = ReviewStatus::Failed;
                        if review_summary.context_compaction == ReviewStatus::NotNeeded {
                            review_summary.context_compaction = ReviewStatus::Failed;
                        }
                        warnings.push(format!(
                            "Document review request failed; using the validated draft: {error:#}"
                        ));
                    }
                }

                // Format inspection needs a rendered document, so it runs after
                // review and drives repair against what the page really looks
                // like rather than against the Markdown.
                emit_stage(&event_sink, GenerationStage::DocumentFormatCheck, None);
                operation.emit(
                    OperationStage::InspectLayout,
                    OperationEventKind::Started,
                    BTreeMap::new(),
                );
                let inspection = inspect_document(
                    &document,
                    InspectionContext {
                        theme: &theme,
                        setup,
                        project: &config.project_name,
                        revision_date: &revision_date,
                        assembler: document_assembler.as_ref(),
                        renderer: document_renderer.as_ref(),
                        diagram_renderer: diagram_renderer.as_ref(),
                        browser_path: config.marp.browser_path.as_deref(),
                        workspace: workspace.as_ref(),
                        prepared_assets: &prepared_assets,
                        generated_assets: &generated_tool_assets,
                        operation: &operation,
                    },
                )
                .await;
                operation.checkpoint(OperationStage::InspectLayout)?;
                match inspection {
                    Ok(issues) => {
                        review_summary.format_check = ReviewStatus::Completed;
                        if issues.is_empty() {
                            review_summary.repair = ReviewStatus::NotNeeded;
                        } else {
                            let mut assessment = FormatAssessment::new(issues);
                            let mut repaired = 0usize;
                            while let Some(issue) = assessment.next_issue() {
                                if repaired >= MAX_FORMAT_REPAIRS {
                                    break;
                                }
                                let Some((_, section)) = document.section_at(issue.section) else {
                                    assessment.give_up_on(issue.section);
                                    continue;
                                };
                                emit_stage(
                                    &event_sink,
                                    GenerationStage::DocumentFormatRepair,
                                    Some(reviewer_name),
                                );
                                context.section_markdown = section.markdown.clone();
                                context.issue = Some(issue.clone());
                                let mut repair = render_pair(
                                    prompt_catalog.as_ref(),
                                    PromptId::DocumentFormatRepairSystem,
                                    PromptId::DocumentFormatRepairUser,
                                    &context,
                                )?;
                                repair.event_sink = event_sink.clone();
                                prompts.extend(repair.prompt_provenance.clone());
                                let response = reviewer
                                    .generate_text(repair, &operation, OperationStage::Repair)
                                    .await;
                                repaired += 1;
                                let candidate = response.and_then(|response| {
                                    let mut candidate = document.clone();
                                    candidate
                                        .replace_section_markdown_at(
                                            issue.section,
                                            strip_markdown_fence(&response.text),
                                        )
                                        .map_err(domain_error)?;
                                    Ok(candidate)
                                });
                                let Ok(candidate) = candidate else {
                                    assessment.give_up_on(issue.section);
                                    warnings.push(format!(
                                        "Focused format repair for section {} was rejected",
                                        issue.section
                                    ));
                                    continue;
                                };
                                let measured = inspect_document(
                                    &candidate,
                                    InspectionContext {
                                        theme: &theme,
                                        setup,
                                        project: &config.project_name,
                                        revision_date: &revision_date,
                                        assembler: document_assembler.as_ref(),
                                        renderer: document_renderer.as_ref(),
                                        diagram_renderer: diagram_renderer.as_ref(),
                                        browser_path: config.marp.browser_path.as_deref(),
                                        workspace: workspace.as_ref(),
                                        prepared_assets: &prepared_assets,
                                        generated_assets: &generated_tool_assets,
                                        operation: &operation,
                                    },
                                )
                                .await;
                                match measured {
                                    Ok(issues) => {
                                        // Only keep a repair that actually helped:
                                        // a rewrite that trades one defect for
                                        // another is churn, not progress.
                                        if assessment.accept_if_improved(issues) {
                                            document = candidate;
                                            review_summary.repair = ReviewStatus::Completed;
                                        } else {
                                            assessment.give_up_on(issue.section);
                                        }
                                    }
                                    Err(error) => {
                                        assessment.give_up_on(issue.section);
                                        warnings.push(format!(
                                            "Could not re-inspect after repairing section {}: {error:#}",
                                            issue.section
                                        ));
                                    }
                                }
                            }
                            let remaining = assessment.into_issues();
                            if !remaining.is_empty() {
                                warnings.push(format!(
                                    "Format review completed with {} defect(s) remaining",
                                    remaining.len()
                                ));
                                if review_summary.repair == ReviewStatus::NotNeeded {
                                    review_summary.repair = ReviewStatus::Failed;
                                }
                            }
                            review_summary.remaining_issues = remaining;
                        }
                    }
                    Err(error) => {
                        review_summary.format_check = ReviewStatus::Failed;
                        warnings.push(format!(
                            "Document format inspection failed; rendering the reviewed document anyway: {error:#}"
                        ));
                    }
                }
            }
            Err(error) => {
                review_summary.semantic_review = ReviewStatus::Failed;
                warnings.push(format!(
                    "Document reviewer could not start; using the validated draft: {error:#}"
                ));
            }
        }
    }

    operation.checkpoint(OperationStage::Render)?;
    operation.emit(
        OperationStage::Render,
        OperationEventKind::Started,
        BTreeMap::new(),
    );
    workspace.create_dir_all(&documents_dir)?;
    let source_markdown = document.render().map_err(domain_error)?;
    let (rendered_markdown, diagram_artifacts) = render_mermaid_diagrams(MermaidRenderRequest {
        markdown: &source_markdown,
        diagrams_dir: &diagrams_dir,
        theme: &theme,
        renderer: diagram_renderer.as_ref(),
        browser_path: config.marp.browser_path.as_deref(),
        workspace: workspace.as_ref(),
        operation: &operation,
        stage: OperationStage::Render,
        image_markdown: document_image_markdown,
    })
    .await?;
    let rendered_document = parse_document(&rendered_markdown, Some(&title))
        .context("Rendering Mermaid diagrams produced an invalid document")?;
    let (used_project_asset_paths, used_project_assets) =
        prepared_assets.materialize_referenced(&rendered_markdown, workspace.as_ref())?;
    let used_generated_assets = retain_referenced_generated_assets(
        generated_tool_assets,
        &rendered_markdown,
        "images",
        workspace.as_ref(),
    )?;
    let mut allowed_assets = diagram_artifacts.clone();
    allowed_assets.extend(used_project_asset_paths.clone());
    allowed_assets.extend(used_generated_assets.clone());
    let assembled = document_assembler.assemble(DocumentAssemblyRequest {
        document: &rendered_document,
        theme: &theme,
        setup,
        project: &config.project_name,
        revision_date: &revision_date,
        allowed_assets: &allowed_assets,
    })?;
    workspace.write(&markdown_path, source_markdown.as_bytes())?;
    workspace.write(&html_path, assembled.html.as_bytes())?;

    emit_stage(&event_sink, GenerationStage::DocumentRendering, None);
    let rendered = document_renderer
        .render_pdf(
            DocumentRenderRequest {
                workspace_root: &documents_dir,
                document: Path::new(&html_name),
                output: Path::new(&pdf_name),
                setup,
            },
            &operation,
        )
        .await?;
    let pages = rendered.pages;
    operation.checkpoint(OperationStage::Render)?;
    operation.emit(
        OperationStage::Render,
        OperationEventKind::Completed,
        BTreeMap::from([("pages".to_string(), pages.to_string())]),
    );

    let mut staged = vec![markdown_path.clone(), html_path.clone(), pdf_path.clone()];
    staged.extend(diagram_artifacts);
    staged.extend(used_project_asset_paths);
    staged.extend(used_generated_assets);
    staged.sort();
    staged.dedup();
    prompts.extend(tool_set.generated_prompts()?);
    deduplicate_prompts(&mut prompts);

    operation.checkpoint(OperationStage::CommitArtifacts)?;
    let transaction = transaction
        .take()
        .context("Document artifact transaction is unavailable")?;
    let files = staged
        .iter()
        .map(|path| document_artifact_file(&documents_dir, path))
        .collect::<Result<Vec<_>>>()?;
    let runtimes = assembled.runtimes.clone();
    let manifest = ResourceArtifactManifest {
        schema_version: 1,
        job_id: transaction.job_id().clone(),
        revision_id: transaction.revision_id().clone(),
        parent_revision: transaction.parent_revision().cloned(),
        project: config.project_name.clone(),
        resource_kind: ArtifactResourceKind::Documents,
        resource_id: slug.clone(),
        title: title.clone(),
        files,
        models: models.clone(),
        prompts: prompts.clone(),
        plugins: Vec::new(),
        runtimes: runtimes.clone(),
        warnings: warnings.clone(),
    };
    let committed = transaction.commit(manifest)?;
    let remap = |path: &Path| -> Result<PathBuf> {
        Ok(committed.root.join(
            path.strip_prefix(&documents_dir)
                .with_context(|| format!("Artifact {} escaped its transaction", path.display()))?,
        ))
    };
    let committed_markdown = remap(&markdown_path)?;
    let committed_pdf = remap(&pdf_path)?;
    let published_pdf_path = publish_document(
        workspace.as_ref(),
        publish_root.as_deref(),
        &committed_pdf,
        &slug,
        &mut warnings,
    )?;
    let mut artifacts = workspace.list_files(&committed.root, &[])?;
    artifacts.sort();

    Ok(GenerateDocumentResult {
        markdown_path: committed_markdown,
        pdf_path: Some(committed_pdf),
        published_pdf_path: published_pdf_path.clone(),
        output: DocumentGenerationOutput {
            project: config.project_name,
            project_instructions: project_instructions_path,
            models,
            tools: tool_summaries.clone(),
            template: template.as_ref().map(|value| value.manifest.name.clone()),
            project_assets: used_project_assets,
            artifacts,
            published_artifacts: published_pdf_path.into_iter().collect(),
            review: review_summary,
            page_setup: setup,
            runtimes,
            prompts,
        },
        prompt_preview: None,
        tool_summaries,
        warnings,
    })
}

/// Resolves the page setup from the theme's defaults and the request overrides.
pub(crate) fn resolve_page_setup(
    theme: &ThemePackage,
    page_size: Option<DocumentPageSize>,
    table_of_contents: Option<bool>,
    cover: Option<bool>,
) -> Result<DocumentPageSetup> {
    let adapter = theme.manifest.adapters.document.as_ref();
    let theme_page_size = adapter
        .and_then(|adapter| adapter.page_size.as_deref())
        .map(str::parse::<DocumentPageSize>)
        .transpose()?;
    Ok(DocumentPageSetup {
        page_size: page_size.or(theme_page_size).unwrap_or_default(),
        table_of_contents: table_of_contents
            .or_else(|| adapter.and_then(|adapter| adapter.table_of_contents))
            .unwrap_or(true),
        cover: cover
            .or_else(|| adapter.and_then(|adapter| adapter.cover))
            .unwrap_or(true),
    })
}

/// Derives the cover date from a revision identifier.
///
/// Revisions are stamped `rev-<hex nanoseconds>`, so the date is a property of
/// the revision rather than of when the render happened to run.
pub(crate) fn revision_date(revision: &str) -> String {
    let Some(stamp) = revision
        .strip_prefix("rev-")
        .and_then(|hex| u128::from_str_radix(hex, 16).ok())
    else {
        return revision.to_owned();
    };
    civil_date(u64::try_from(stamp / 1_000_000_000).unwrap_or_default())
}

/// Formats a Unix timestamp as an ISO calendar date.
fn civil_date(seconds: u64) -> String {
    // Howard Hinnant's civil-from-days, which needs no calendar dependency.
    let days = i64::try_from(seconds / 86_400).unwrap_or_default();
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    let year = if month <= 2 { year + 1 } else { year };
    format!("{year:04}-{month:02}-{day:02}")
}

/// How a rendered diagram is embedded in a document.
///
/// Constrained by the width of the text column, unlike a slide's diagram, which
/// is constrained by the height of its fixed box.
pub(crate) fn document_image_markdown(name: &str) -> String {
    format!("![Diagram](diagrams/{name}.svg)")
}

fn publish_document(
    workspace: &dyn WorkspaceFileSystem,
    root: Option<&Path>,
    pdf: &Path,
    slug: &str,
    warnings: &mut Vec<String>,
) -> Result<Option<PathBuf>> {
    let Some(root) = root else {
        return Ok(None);
    };
    let destination = root.join("_sfumato/documents").join(slug);
    match workspace.publish_atomic(pdf, &destination) {
        Ok(path) => Ok(Some(path)),
        Err(error) => {
            warnings.push(format!(
                "Document publication failed; the managed revision was preserved: {error}"
            ));
            Ok(None)
        }
    }
}

fn document_artifact_file(root: &Path, path: &Path) -> Result<ResourceArtifactFile> {
    let relative = path
        .strip_prefix(root)
        .context("Document artifact escaped staging")?
        .to_path_buf();
    let (kind, media_type) = match relative.extension().and_then(|value| value.to_str()) {
        Some("pdf") => (ArtifactKind::Pdf, Some("application/pdf".into())),
        Some("md") => (ArtifactKind::Markdown, Some("text/markdown".into())),
        Some("html") => (ArtifactKind::Source, Some("text/html".into())),
        Some("svg") => (ArtifactKind::Image, Some("image/svg+xml".into())),
        Some("png") => (ArtifactKind::Image, Some("image/png".into())),
        Some("mmd") => (ArtifactKind::Source, Some("text/plain".into())),
        _ => (ArtifactKind::Source, None),
    };
    Ok(ResourceArtifactFile {
        path: relative,
        kind,
        media_type,
    })
}

fn deduplicate_prompts(prompts: &mut Vec<PromptProvenance>) {
    let mut seen = std::collections::BTreeSet::new();
    prompts.retain(|prompt| seen.insert((prompt.id, prompt.content_hash.clone())));
}

fn domain_error(error: impl std::fmt::Display) -> SfumatoError {
    SfumatoError::provider(ErrorClass::InvalidOutput, error.to_string())
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../../tests/unit/resources_documents.rs"]
mod tests;
