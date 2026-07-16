use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, anyhow, bail};
use json_patch::Patch;
use serde::Serialize;
use serde_json::Value;
use sfumato_domain::RevisionId;
use slug::slugify;

use super::{
    SlidePromptContext, compact_review_snapshot, constrain_generated_images, copy_theme_css,
    emit_context_compaction, emit_layout_result, emit_review_retry, emit_stage, ensure_inside,
    extract_generated_title, format_tokens, generation_limit, inspect_candidate_layout,
    markdown_fences, render_mermaid_diagrams, render_prompt_request, request_chars,
    resource_artifact_file, validate_normalized_deck,
};
use crate::{
    artifacts::{ArtifactResourceKind, ArtifactStore, ResourceArtifactManifest},
    config::{Capability, EffectiveConfig},
    generation::SlideLayoutIssue,
    instructions::ProjectInstructions,
    prompts::{PromptCatalog, PromptId, PromptPair, PromptProvenance},
    providers::{
        GenerationStage, ProviderFactory, TextGenerationEvent, TextGenerationProvider,
        TextGenerationRequest,
    },
    renderers::marp,
    review::{
        PatchReport, ReviewConstraint, ReviewSnapshot, decks::SlideDeckDocument, parse_json_patch,
    },
    themes::{ThemePackage, ThemeService},
};

pub struct EditSlidesRequest {
    pub markdown_path: PathBuf,
    pub instruction: String,
}

pub struct EditSlidesOptions {
    pub event_sink: Option<Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
    pub prompt_catalog: Arc<dyn PromptCatalog>,
    pub artifact_store: Arc<dyn ArtifactStore>,
    pub provider_factory: Arc<dyn ProviderFactory>,
}

#[derive(Debug, Serialize)]
pub struct EditSlidesResult {
    pub project: String,
    pub model: String,
    pub markdown_path: PathBuf,
    pub pdf_path: PathBuf,
    pub project_instructions: Option<PathBuf>,
    pub operations: usize,
    pub changed_slides: Vec<String>,
    pub context_compacted: bool,
    pub layout_issues: Vec<SlideLayoutIssue>,
    pub artifacts: Vec<PathBuf>,
    pub warnings: Vec<String>,
    pub prompts: Vec<PromptProvenance>,
}

pub async fn edit_slides(
    config: EffectiveConfig,
    request: EditSlidesRequest,
    options: EditSlidesOptions,
) -> Result<EditSlidesResult> {
    let instruction = request.instruction.trim();
    if instruction.is_empty() {
        bail!("Instruction cannot be empty");
    }

    let source_markdown_path = fs::canonicalize(&request.markdown_path).with_context(|| {
        format!(
            "Could not resolve slide deck {}",
            request.markdown_path.display()
        )
    })?;
    if source_markdown_path
        .extension()
        .and_then(|value| value.to_str())
        != Some("md")
    {
        bail!("Slide edits require a generated `.md` deck");
    }
    let artifact_root = fs::canonicalize(
        options.artifact_store.project_root(&config.project_name)?,
    )
    .with_context(|| {
        format!(
            "Could not resolve the artifact workspace for project '{}'",
            config.project_name
        )
    })?;
    ensure_inside(&artifact_root, &source_markdown_path).with_context(|| {
        format!(
            "{} is not a generated artifact for project '{}'; select the correct project with --project",
            source_markdown_path.display(),
            config.project_name
        )
    })?;

    let original = fs::read_to_string(&source_markdown_path)
        .with_context(|| format!("Could not read {}", source_markdown_path.display()))?;
    let title = extract_generated_title(&original)
        .context("Could not find the title slide in the generated Marp deck")?;
    let document = SlideDeckDocument::from_marp(&original, &title)
        .context("Could not parse the generated Marp deck for focused editing")?;
    let snapshot = document.snapshot()?;
    let theme_name = deck_theme_name(&original)?;
    let theme = ThemeService::load()?.resolve(&theme_name)?;
    let project_instructions = ProjectInstructions::load(&config.project_root)?;
    let project_instructions_prompt = project_instructions
        .as_ref()
        .map(ProjectInstructions::prompt_section)
        .unwrap_or_else(|| "Project instructions: no SFUMATO.md was found.".to_string());
    let (model_name, model_profile) = config.resolve_model(Capability::Text)?;
    let provider = options.provider_factory.text(&config, model_profile)?;
    let model_name = model_name.to_string();

    let transaction = options
        .artifact_store
        .begin(&config.project_name, ArtifactResourceKind::Slides)?;
    let source_revision_root = source_markdown_path
        .parent()
        .context("Generated slide deck must live in a revision directory")?;
    copy_revision_tree(source_revision_root, transaction.staging_root())?;
    let markdown_path = transaction.staging_root().join(
        source_markdown_path
            .file_name()
            .context("Generated slide deck must have a filename")?,
    );

    emit_stage(
        &options.event_sink,
        GenerationStage::Edit,
        Some(&model_name),
    );
    let edit = request_edit_patch(
        provider.as_ref(),
        &config,
        &theme,
        instruction,
        &snapshot,
        &document,
        &project_instructions_prompt,
        &options.event_sink,
        options.prompt_catalog.as_ref(),
    )
    .await?;
    let candidate = constrain_generated_images(&edit.markdown);
    validate_normalized_deck(&candidate, &title)?;

    let parent = markdown_path
        .parent()
        .context("Slide deck path must have a parent directory")?;
    let stem = markdown_path
        .file_stem()
        .and_then(|value| value.to_str())
        .context("Slide deck filename must be valid UTF-8")?;
    let slug = slugify(stem);
    if slug.is_empty() {
        bail!("Slide deck filename cannot be used for generated artifacts");
    }
    let pdf_path = markdown_path.with_extension("pdf");
    let themes_dir = parent.join("themes");
    let theme_css_path = themes_dir.join(format!("{theme_name}.css"));
    let diagrams_dir = parent.join("diagrams");
    ensure_inside(&artifact_root, &pdf_path)?;
    ensure_inside(&artifact_root, &theme_css_path)?;
    ensure_inside(&artifact_root, &diagrams_dir)?;

    emit_stage(&options.event_sink, GenerationStage::LayoutCheck, None);
    let mut warnings = Vec::new();
    let layout_issues = match inspect_candidate_layout(
        &candidate,
        &theme,
        &slug,
        config.marp.browser_path.as_deref(),
    )
    .await
    {
        Ok(issues) => {
            emit_layout_result(&options.event_sink, issues.len());
            if !issues.is_empty() {
                let slides = issues
                    .iter()
                    .map(|issue| issue.slide.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                warnings.push(format!(
                    "The edited deck still overflows on slide(s): {slides}"
                ));
            }
            issues
        }
        Err(error) => {
            warnings.push(format!("Layout inspection skipped: {error:#}"));
            Vec::new()
        }
    };

    let (rendered_markdown, diagram_artifacts) =
        render_mermaid_diagrams(&candidate, &diagrams_dir, &slug, &theme).await?;
    copy_theme_css(&theme, &theme_css_path)?;
    emit_stage(&options.event_sink, GenerationStage::Rendering, None);
    replace_deck_and_pdf(
        &markdown_path,
        &pdf_path,
        &theme_css_path,
        &rendered_markdown,
        config.marp.browser_path.as_deref(),
    )
    .await?;

    let mut artifacts = vec![markdown_path.clone(), pdf_path.clone(), theme_css_path];
    artifacts.extend(diagram_artifacts);
    let files = collect_revision_files(transaction.staging_root())?
        .iter()
        .map(|path| resource_artifact_file(transaction.staging_root(), path))
        .collect::<Result<Vec<_>>>()?;
    let parent_revision = source_revision_root
        .file_name()
        .and_then(|value| value.to_str())
        .and_then(|value| RevisionId::new(value).ok());
    let manifest = ResourceArtifactManifest {
        schema_version: 1,
        job_id: transaction.job_id().clone(),
        revision_id: transaction.revision_id().clone(),
        parent_revision,
        project: config.project_name.clone(),
        resource_kind: ArtifactResourceKind::Slides,
        resource_id: slug.clone(),
        title: title.clone(),
        files,
        models: std::collections::BTreeMap::from([("text".to_string(), model_name.clone())]),
        prompts: edit.prompts.clone(),
        warnings: warnings.clone(),
    };
    let staging_root = transaction.staging_root().to_path_buf();
    let committed = transaction.commit(manifest)?;
    let remap = |path: &Path| -> Result<PathBuf> {
        Ok(committed
            .root
            .join(path.strip_prefix(&staging_root).with_context(|| {
                format!("Edited artifact {} escaped its transaction", path.display())
            })?))
    };
    let markdown_path = remap(&markdown_path)?;
    let pdf_path = remap(&pdf_path)?;
    artifacts = artifacts
        .iter()
        .map(|path| remap(path))
        .collect::<Result<Vec<_>>>()?;
    artifacts.push(committed.manifest_path);
    Ok(EditSlidesResult {
        project: config.project_name,
        model: model_name,
        markdown_path,
        pdf_path,
        project_instructions: project_instructions.map(|instructions| instructions.path),
        operations: edit.report.operations,
        changed_slides: edit.report.changed_nodes,
        context_compacted: edit.context_compacted,
        layout_issues,
        artifacts,
        warnings,
        prompts: edit.prompts,
    })
}

struct AppliedEdit {
    markdown: String,
    report: PatchReport,
    context_compacted: bool,
    prompts: Vec<PromptProvenance>,
}

struct EditRetryContext {
    invalid_response: String,
    error: String,
}

#[allow(clippy::too_many_arguments)]
async fn request_edit_patch(
    provider: &dyn TextGenerationProvider,
    config: &EffectiveConfig,
    theme: &ThemePackage,
    instruction: &str,
    snapshot: &ReviewSnapshot,
    document: &SlideDeckDocument,
    project_instructions: &str,
    event_sink: &Option<Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
    prompt_catalog: &dyn PromptCatalog,
) -> Result<AppliedEdit> {
    let mut retry = None;
    let mut compacted = false;
    let mut prompts = Vec::new();
    for attempt in 1..=2 {
        let mut full_request = build_edit_request(
            prompt_catalog,
            config,
            theme,
            instruction,
            snapshot,
            retry.as_ref(),
            project_instructions,
        )?;
        let mut compact_request = build_compact_edit_request(
            prompt_catalog,
            config,
            theme,
            instruction,
            snapshot,
            retry.as_ref(),
            project_instructions,
        )?;
        full_request.event_sink = event_sink.clone();
        compact_request.event_sink = event_sink.clone();
        prompts.extend(full_request.prompt_provenance.clone());
        prompts.extend(compact_request.prompt_provenance.clone());

        let response = if compacted {
            provider
                .generate_text(compact_request)
                .await
                .context("Compact slide edit request failed")?
        } else {
            let original_chars = request_chars(&full_request);
            match provider.generate_text(full_request).await {
                Ok(response) => response,
                Err(error) if generation_limit(&error).is_some() => {
                    compacted = true;
                    emit_context_compaction(
                        event_sink,
                        GenerationStage::Edit,
                        original_chars,
                        request_chars(&compact_request),
                    );
                    provider
                        .generate_text(compact_request)
                        .await
                        .context("Compact slide edit request failed after a model limit")?
                }
                Err(error) => return Err(error).context("Slide edit request failed"),
            }
        };

        match apply_edit_response(document, &response.text) {
            Ok((markdown, report)) => {
                return Ok(AppliedEdit {
                    markdown,
                    report,
                    context_compacted: compacted,
                    prompts,
                });
            }
            Err(error) if attempt == 1 => {
                let error = format!("{error:#}");
                emit_review_retry(event_sink, attempt + 1, &error);
                retry = Some(EditRetryContext {
                    invalid_response: response.text,
                    error,
                });
            }
            Err(error) => {
                return Err(error).context(
                    "The slide editor returned an invalid patch after one corrective retry",
                );
            }
        }
    }
    unreachable!("the edit retry loop always returns")
}

fn apply_edit_response(
    document: &SlideDeckDocument,
    response: &str,
) -> Result<(String, PatchReport)> {
    let patch = parse_json_patch(response)?;
    validate_content_only_patch(&patch)?;
    let mut candidate = document.clone();
    let report = candidate.apply_patch(&patch)?;
    Ok((candidate.render()?, report))
}

fn validate_content_only_patch(patch: &Patch) -> Result<()> {
    let operations = serde_json::to_value(patch)?;
    for operation in operations
        .as_array()
        .context("JSON Patch must be an array")?
    {
        let kind = operation
            .get("op")
            .and_then(Value::as_str)
            .context("JSON Patch operation is missing `op`")?;
        if kind == "test" {
            continue;
        }
        let path = operation
            .get("path")
            .and_then(Value::as_str)
            .context("JSON Patch operation is missing `path`")?;
        let is_slide_markdown = path.starts_with("/slides/slide-")
            && path.ends_with("/markdown")
            && path.split('/').count() == 4;
        if kind != "replace" || !is_slide_markdown {
            bail!("Slide edit patches may only replace existing `/slides/<id>/markdown` content");
        }
    }
    Ok(())
}

fn build_edit_request(
    catalog: &dyn PromptCatalog,
    config: &EffectiveConfig,
    theme: &ThemePackage,
    instruction: &str,
    snapshot: &ReviewSnapshot,
    retry: Option<&EditRetryContext>,
    project_instructions: &str,
) -> Result<TextGenerationRequest> {
    let mut snapshot = snapshot.clone();
    snapshot.constraints = vec![
        ReviewConstraint::Rfc6902Only,
        ReviewConstraint::TestDeckRevision,
        ReviewConstraint::TestSlideRevision,
        ReviewConstraint::ReplaceSlideMarkdownOnly,
        ReviewConstraint::PreserveTitleSlide,
        ReviewConstraint::PreserveMetadata,
    ];
    let snapshot = serde_json::to_string_pretty(&snapshot)
        .context("Could not serialize the slide deck for editing")?;
    let context = edit_prompt_context(
        config,
        theme,
        instruction,
        snapshot,
        retry,
        project_instructions.to_string(),
        false,
    );
    render_prompt_request(catalog, EDIT_PROMPTS, &context)
}

fn build_compact_edit_request(
    catalog: &dyn PromptCatalog,
    config: &EffectiveConfig,
    theme: &ThemePackage,
    instruction: &str,
    snapshot: &ReviewSnapshot,
    retry: Option<&EditRetryContext>,
    project_instructions: &str,
) -> Result<TextGenerationRequest> {
    let snapshot = compact_review_snapshot(snapshot)?;
    let context = edit_prompt_context(
        config,
        theme,
        instruction,
        snapshot,
        retry,
        super::excerpt(project_instructions, 4_000),
        true,
    );
    render_prompt_request(catalog, COMPACT_EDIT_PROMPTS, &context)
}

fn edit_prompt_context(
    config: &EffectiveConfig,
    theme: &ThemePackage,
    instruction: &str,
    deck_snapshot: String,
    retry: Option<&EditRetryContext>,
    project_instructions: String,
    compact: bool,
) -> SlidePromptContext {
    SlidePromptContext {
        project: config.project_name.clone(),
        project_root: config.project_root.display().to_string(),
        theme_name: theme.manifest.name.clone(),
        theme_colors: format_tokens(&theme.manifest.tokens.colors),
        theme_fonts: format_tokens(&theme.manifest.tokens.fonts),
        instruction: instruction.to_string(),
        project_instructions,
        deck_snapshot,
        retry_present: retry.is_some(),
        retry_error: retry
            .map(|retry| super::excerpt(&retry.error, 1_500))
            .unwrap_or_default(),
        retry_invalid_response: retry
            .map(|retry| super::excerpt(&retry.invalid_response, 5_000))
            .unwrap_or_default(),
        compact,
        max_tool_rounds: 0,
        ..Default::default()
    }
}

const EDIT_PROMPTS: PromptPair = PromptPair {
    system: PromptId::SlidesEditSystem,
    user: PromptId::SlidesEditUser,
    tool_exhausted: PromptId::SlidesEditToolExhaustedUser,
};

const COMPACT_EDIT_PROMPTS: PromptPair = PromptPair {
    system: PromptId::SlidesCompactEditSystem,
    user: PromptId::SlidesCompactEditUser,
    tool_exhausted: PromptId::SlidesEditToolExhaustedUser,
};

fn deck_theme_name(markdown: &str) -> Result<String> {
    let fences = markdown_fences(markdown);
    if fences.len() < 2 || fences[0].start != 0 {
        bail!("Generated slide deck is missing canonical Marp frontmatter");
    }
    let frontmatter = &markdown[fences[0].end..fences[1].start];
    frontmatter
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("theme:")
                .map(str::trim)
                .map(|value| value.trim_matches(['\'', '"']).to_string())
        })
        .filter(|value| !value.is_empty())
        .context("Generated slide deck frontmatter does not select a theme")
}

fn copy_revision_tree(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)
        .with_context(|| format!("Could not create {}", destination.display()))?;
    for entry in fs::read_dir(source)
        .with_context(|| format!("Could not read revision directory {}", source.display()))?
    {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            bail!(
                "Generated revision contains an unsafe symlink: {}",
                entry.path().display()
            );
        }
        if entry.file_name() == "manifest.json" {
            continue;
        }
        let target = destination.join(entry.file_name());
        if file_type.is_dir() {
            copy_revision_tree(&entry.path(), &target)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), &target).with_context(|| {
                format!(
                    "Could not stage existing artifact {}",
                    entry.path().display()
                )
            })?;
        }
    }
    Ok(())
}

fn collect_revision_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(root).follow_links(false) {
        let entry = entry?;
        if entry.file_type().is_symlink() {
            bail!(
                "Revision contains an unsafe symlink: {}",
                entry.path().display()
            );
        }
        if entry.file_type().is_file() && entry.file_name() != "manifest.json" {
            files.push(entry.path().to_path_buf());
        }
    }
    files.sort();
    Ok(files)
}

async fn replace_deck_and_pdf(
    markdown_path: &Path,
    pdf_path: &Path,
    theme_css_path: &Path,
    markdown: &str,
    browser_path: Option<&Path>,
) -> Result<()> {
    let parent = markdown_path
        .parent()
        .context("Slide deck path must have a parent directory")?;
    let mut temporary_markdown = tempfile::Builder::new()
        .prefix(".sfumato-edit-")
        .suffix(".md")
        .tempfile_in(parent)
        .context("Could not create a temporary edited deck")?;
    temporary_markdown
        .write_all(markdown.as_bytes())
        .context("Could not write the temporary edited deck")?;
    temporary_markdown
        .flush()
        .context("Could not flush the temporary edited deck")?;
    let temporary_pdf = tempfile::Builder::new()
        .prefix(".sfumato-edit-")
        .suffix(".pdf")
        .tempfile_in(parent)
        .context("Could not create a temporary edited PDF")?;

    marp::render_pdf(
        temporary_markdown.path(),
        theme_css_path,
        temporary_pdf.path(),
        browser_path,
    )
    .await
    .context("Could not render the edited slide deck to PDF; the original deck was preserved")?;

    if let Ok(metadata) = fs::metadata(markdown_path) {
        fs::set_permissions(temporary_markdown.path(), metadata.permissions())?;
    }
    temporary_markdown
        .persist(markdown_path)
        .map_err(|error| anyhow!(error.error))
        .with_context(|| format!("Could not replace {}", markdown_path.display()))?;
    temporary_pdf
        .persist(pdf_path)
        .map_err(|error| anyhow!(error.error))
        .with_context(|| format!("Could not replace {}", pdf_path.display()))?;
    Ok(())
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private editing helpers.
#[path = "../../../tests/unit/resources_slides_edit.rs"]
mod tests;
