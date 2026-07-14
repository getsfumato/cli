use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, bail};
use slug::slugify;
use walkdir::WalkDir;

use crate::{
    config::{Capability, EffectiveConfig, ModelRole},
    generation::{
        GenerationOutput, GenerationRequest, GenerationToolSummary, ReviewStatus, SlideLayoutIssue,
        SlideReviewSummary,
    },
    providers::{
        GenerationStage, TextGenerationEvent, TextGenerationProvider, TextGenerationRequest,
        ToolDefinition, build_image_provider, build_text_provider,
    },
    renderers::{
        diagrams::{MermaidDiagramRenderer, MermaidThemeConfig},
        marp,
    },
    review::{ReviewSnapshot, ReviewableDocument, decks::SlideDeckDocument, parse_json_patch},
    themes::{ThemePackage, ThemeService, ThemeTokens},
    tools::{ImageToolConfig, generation_tools},
};

const SUPPORTED_EXTENSIONS: &[&str] = &[
    "md", "txt", "rs", "py", "js", "ts", "html", "css", "json", "toml", "yaml", "yml",
];

pub struct GenerateSlidesOptions {
    pub title: Option<String>,
    pub dry_run: bool,
    pub review: bool,
    pub event_sink: Option<std::sync::Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
}

#[derive(Debug)]
pub struct GenerateSlidesResult {
    pub markdown_path: PathBuf,
    pub pdf_path: Option<PathBuf>,
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
    } = options;
    let output_root = config.output_root()?;
    let slides_dir = output_root.join("slides");
    let title_override = title_override
        .map(|title| validate_title(&title))
        .transpose()?;
    let theme_css_path = slides_dir
        .join("themes")
        .join(format!("{}.css", config.theme));
    let diagrams_dir = slides_dir.join("diagrams");
    let images_dir = slides_dir.join("images");

    ensure_inside(&output_root, &theme_css_path)?;
    ensure_inside(&output_root, &diagrams_dir)?;
    ensure_inside(&output_root, &images_dir)?;

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
                Arc::from(build_image_provider(&config, profile)?);
            Ok::<ImageToolConfig, anyhow::Error>(ImageToolConfig {
                provider,
                profile_name: profile_name.to_string(),
                output_dir: images_dir.clone(),
                theme: theme.clone(),
            })
        })
        .transpose()?;
    let tool_set = generation_tools(&config.project_root, &request.sources, image_tool)?;
    let review_tool_definitions = tool_set
        .definitions
        .iter()
        .filter(|tool| tool.function.name != "sfumato_image_gen")
        .cloned()
        .collect::<Vec<_>>();
    let tool_summaries = summarize_tools(&tool_set.definitions);
    let source_bundle = build_source_bundle(&documents);
    let mut provider_request = build_generation_request(
        &config,
        &theme,
        &request.instruction,
        title_override.as_deref(),
        &source_bundle,
        image_selection.is_some(),
    );
    provider_request.tools = tool_set.definitions.clone();
    provider_request.tool_executor = Some(tool_set.executor.clone());
    provider_request.event_sink = event_sink.clone();
    let (draft_profile_name, draft_profile) = config.resolve_model(Capability::Text)?;
    provider_request.max_tool_rounds = model_tool_rounds(draft_profile);
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

    if dry_run {
        let dry_run_title = title_override.as_deref().unwrap_or("model-generated-title");
        let (markdown_path, _) = slide_artifact_paths(&slides_dir, dry_run_title)?;
        ensure_inside(&output_root, &markdown_path)?;
        return Ok(GenerateSlidesResult {
            markdown_path,
            pdf_path: None,
            output: GenerationOutput {
                project: config.project_name,
                models: selected_models,
                tools: tool_summaries.clone(),
                artifacts: Vec::new(),
                review: review_summary,
            },
            prompt_preview: Some(provider_request.user_prompt),
            tool_summaries,
            warnings: Vec::new(),
        });
    }

    emit_stage(
        &event_sink,
        GenerationStage::Draft,
        Some(draft_profile_name),
    );
    let provider = build_text_provider(&config, draft_profile)?;
    let response = provider.generate_text(provider_request).await?;
    let title = match title_override {
        Some(title) => title,
        None => match validate_draft_title(&response.text, &request.instruction) {
            Ok(title) => title,
            Err(error) => {
                let error = format!("{error:#}");
                emit_title_repair(&event_sink, &error);
                let mut title_request = build_title_repair_request(
                    &config,
                    &request.instruction,
                    &response.text,
                    &error,
                );
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
    ensure_inside(&output_root, &markdown_path)?;
    ensure_inside(&output_root, &pdf_path)?;
    let mut markdown = normalize_marp_markdown(&response.text, &config, &title)?;
    let mut warnings = Vec::new();

    let mut reviewer_provider: Option<Box<dyn TextGenerationProvider>> = None;
    if let Some((reviewer_name, reviewer_profile)) = reviewer_selection {
        match build_text_provider(&config, reviewer_profile) {
            Ok(reviewer) => {
                emit_stage(
                    &event_sink,
                    GenerationStage::SemanticReview,
                    Some(reviewer_name),
                );
                let review_result = async {
                    let document = SlideDeckDocument::from_marp(&markdown, &title)?;
                    let snapshot = document.snapshot()?;
                    let mut retry = None;
                    for attempt in 1..=2 {
                        let mut review_request = build_review_request(
                            &config,
                            &theme,
                            &request.instruction,
                            &source_bundle,
                            &snapshot,
                            retry.as_ref(),
                        )?;
                        review_request.tools = review_tool_definitions.clone();
                        review_request.tool_executor = Some(tool_set.executor.clone());
                        review_request.event_sink = event_sink.clone();
                        review_request.max_tool_rounds = model_tool_rounds(reviewer_profile);
                        let response = reviewer.generate_text(review_request).await?;
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
                            Ok(reviewed) => return Ok(reviewed),
                            Err(error) if attempt == 1 && should_retry_model_output(&error) => {
                                let error = format!("{error:#}");
                                emit_review_retry(&event_sink, attempt + 1, &error);
                                retry = Some(ReviewRetryContext {
                                    invalid_response: response.text,
                                    error,
                                });
                            }
                            Err(error) if attempt == 1 => {
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
                    unreachable!("review loop either returns a candidate or an error")
                }
                .await;
                match review_result {
                    Ok(reviewed) => {
                        markdown = reviewed;
                        review_summary.semantic_review = ReviewStatus::Completed;
                    }
                    Err(error) => {
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
                        for attempt in 1..=2 {
                            let repair_request = build_layout_repair_request(
                                LayoutRepairRequestContext {
                                    config: &config,
                                    theme: &theme,
                                    instruction: &request.instruction,
                                    title: &title,
                                    slide_markdown: &original_slide,
                                    issue,
                                },
                                retry.as_ref(),
                                event_sink.clone(),
                            );
                            let response = match reviewer.generate_text(repair_request).await {
                                Ok(response) => response,
                                Err(error) => {
                                    warnings.push(format!(
                                        "Focused layout repair failed for slide {}: {error:#}",
                                        issue.slide
                                    ));
                                    break;
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

    let mut artifacts = vec![markdown_path.clone(), theme_css_path];
    artifacts.extend(tool_set.generated_artifacts()?);
    artifacts.extend(diagram_artifacts);
    if let Some(pdf) = &rendered_pdf {
        artifacts.push(pdf.clone());
    }

    Ok(GenerateSlidesResult {
        markdown_path,
        pdf_path: rendered_pdf,
        output: GenerationOutput {
            project: config.project_name,
            models: selected_models,
            tools: tool_summaries.clone(),
            artifacts,
            review: review_summary,
        },
        prompt_preview: None,
        tool_summaries,
        warnings,
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
    slug: &str,
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
        let source_path = diagrams_dir.join(format!("{slug}-diagram-{diagram_index}.mmd"));
        let artifact_path = diagrams_dir.join(format!("{slug}-diagram-{diagram_index}.svg"));
        let source = normalize_mermaid_source(&block.source);
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
        rendered.push_str(&embedded_svg_markdown(slug, diagram_index));
        artifacts.push(source_path);
        artifacts.push(artifact_path);
        cursor = block.end;
    }

    rendered.push_str(&markdown[cursor..]);
    Ok((rendered, artifacts))
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

fn embedded_svg_markdown(slug: &str, index: usize) -> String {
    format!("![Mermaid diagram {index}](diagrams/{slug}-diagram-{index}.svg)")
}

fn build_generation_request(
    config: &EffectiveConfig,
    theme: &ThemePackage,
    instruction: &str,
    title: Option<&str>,
    source_bundle: &str,
    image_generation_available: bool,
) -> TextGenerationRequest {
    let learning_style = if config.user.learning_style.is_empty() {
        "not specified".to_string()
    } else {
        config.user.learning_style.join(", ")
    };

    let system_prompt = format!(
        "You are Sfumato, a careful study-resource generator. Create clear, accurate Marp slide decks for Obsidian users. Use the user's learning preferences: {learning_style}. Prefer concise slides, useful examples, and presenter notes when they help."
    );
    let title_requirement = match title {
        Some(title) => format!(
            "Use exactly \"{title}\" as the deck title and as the first `# H1` on the title slide."
        ),
        None => "Create a concise, specific deck title that describes the subject being taught. Do not copy or lightly rephrase the instruction as the title. Put your chosen title in the first `# H1` on the title slide; Sfumato will use it as the artifact filename."
            .to_string(),
    };
    let image_requirement = if image_generation_available {
        "- You may call `sfumato_image_gen` when a purpose-built educational illustration would teach better than text or Mermaid. Give it a precise subject, composition, labels, and learning purpose, then embed the returned `markdown_path` with standard Markdown image syntax. Sfumato applies the project theme automatically."
    } else {
        "- No image generation model is configured. Use Markdown and Mermaid for visuals."
    };

    let user_prompt = format!(
        r#"Create a Marp Markdown slide deck.

Project: {project}
Allowed filesystem root: {project_root}
Theme: {theme_name}
Theme colors: {theme_colors}
Theme fonts: {theme_fonts}
Instruction: {instruction}

Requirements:
- {title_requirement}
- Return only Markdown.
- Start the document with Marp frontmatter as the first bytes. Do not put a slide, title, blank `---`, or any text before it.
- Do not include `style:`, inline CSS, or generated theme CSS in Marp frontmatter.
- Set Marp math rendering to MathJax with `math: mathjax`.
- Use slide separators with ---.
- Include a title slide.
- Use `<!-- _class: lead -->` for title and section-divider slides.
- Use `<!-- _class: compact -->` only for dense tables, formulas, or comparison slides.
- Prefer `##` headings for normal content slides after the title slide.
- You may use Mermaid diagrams in fenced ```mermaid blocks when a visual structure helps.
{image_requirement}
- Keep Mermaid diagrams simple: at most 6 nodes, short quoted labels, and no formulas inside labels.
- Put math formulas in normal Markdown outside Mermaid diagrams.
- Use `<br/>` inside Mermaid labels when a label needs two short lines.
- Do not use raw HTML, inline SVG, or HTML wrapper tags.
- Include a short learning objective slide.
- Explain the source material for a student.
- Use examples from the provided files.
- Add presenter notes with short teaching cues when useful.
- You may call Sfumato filesystem tools to list allowed directories or read allowed text files when more context is needed.
- When calling filesystem tools, prefer absolute paths under the allowed filesystem root.
- Explore selectively. After reading the most relevant files, stop calling tools and produce the final deck.

Source material:
{source_bundle}
"#,
        project = config.project_name,
        project_root = config.project_root.display(),
        theme_name = theme.manifest.name,
        theme_colors = format_tokens(&theme.manifest.tokens.colors),
        theme_fonts = format_tokens(&theme.manifest.tokens.fonts),
    );

    TextGenerationRequest::new(system_prompt, user_prompt)
}

fn build_title_repair_request(
    config: &EffectiveConfig,
    instruction: &str,
    draft: &str,
    validation_error: &str,
) -> TextGenerationRequest {
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
    let system_prompt = "You repair one missing metadata field in an existing study deck. Return only a concise subject title on one plain-text line. Do not return Markdown, JSON, explanations, or tool calls.".to_string();
    let user_prompt = format!(
        r#"Create the missing title for an existing Marp deck.

Project: {project}
Original instruction: {instruction}
Validation error: {validation_error}

Existing deck headings:
{headings}

Requirements:
- Return exactly one concise title on one line.
- Describe the subject being taught, not the generation instruction.
- Do not copy or lightly rephrase the original instruction.
- Do not include `#`, quotes, a filename extension, or ending punctuation.
- Do not regenerate or summarize the deck.
"#,
        project = config.project_name,
    );
    TextGenerationRequest::new(system_prompt, user_prompt)
}

fn build_review_request(
    config: &EffectiveConfig,
    theme: &ThemePackage,
    instruction: &str,
    source_bundle: &str,
    snapshot: &ReviewSnapshot,
    retry: Option<&ReviewRetryContext>,
) -> Result<TextGenerationRequest> {
    let snapshot = serde_json::to_string_pretty(snapshot)
        .context("Could not serialize slide deck review snapshot")?;
    let system_prompt = "You are Sfumato's slide reviewer. Propose precise, conservative changes as an RFC 6902 JSON Patch. Never regenerate or return the complete deck.".to_string();
    let retry_feedback = retry
        .map(|retry| {
            format!(
                r#"
Corrective retry:
Your previous response was rejected by Sfumato.
Validation error:
{error}

Previous invalid response:
{invalid_response}

Return a corrected JSON Patch against the original snapshot below. Do not patch or continue the rejected response.
"#,
                error = excerpt(&retry.error, 2_000),
                invalid_response = excerpt(&retry.invalid_response, 12_000),
            )
        })
        .unwrap_or_default();
    let user_prompt = format!(
        r#"Review this generated slide deck before it is published.

Original instruction: {instruction}
Project: {project}
Theme: {theme_name}
Theme colors: {theme_colors}
Theme fonts: {theme_fonts}

Review requirements:
- Return only an RFC 6902 JSON Patch array. Do not use a Markdown code fence.
- The patch target is the object inside `document`; paths therefore start at `/revision`, `/slides`, or `/order`, not `/document`.
- Start with a `test` operation for `/revision` before any mutation.
- Before replacing or removing a slide, test `/slides/<id>/revision` with the supplied value.
- Prefer replacing only `/slides/<id>/markdown` for slides that genuinely need correction.
- Do not return the complete deck or replace the document root.
- Do not modify the title slide, title, frontmatter, IDs, revisions, headings, kinds, or element summaries.
- Correct factual inconsistencies only when supported by the source material or filesystem tools.
- Remove duplication and improve the learning sequence.
- Keep one principal idea per slide and concise supporting content.
- Add, remove, or reorder slides only when a targeted Markdown replacement cannot solve the problem.
- Preserve useful explanations instead of deleting them merely to save space.
- Keep MathJax math and Mermaid fences valid. Do not add inline CSS, raw HTML, or inline SVG.
- Use the filesystem tools only when a claim cannot be checked from the supplied source excerpts.
- Return `[]` when no changes are needed.
{retry_feedback}

Source material:
{source_bundle}

Structured review snapshot:
{snapshot}
"#,
        project = config.project_name,
        theme_name = theme.manifest.name,
        theme_colors = format_tokens(&theme.manifest.tokens.colors),
        theme_fonts = format_tokens(&theme.manifest.tokens.fonts),
    );
    Ok(TextGenerationRequest::new(system_prompt, user_prompt))
}

struct ReviewRetryContext {
    invalid_response: String,
    error: String,
}

fn build_layout_repair_request(
    context: LayoutRepairRequestContext<'_>,
    retry: Option<&LayoutRepairRetryContext>,
    event_sink: Option<std::sync::Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
) -> TextGenerationRequest {
    let issue_report =
        serde_json::to_string_pretty(context.issue).unwrap_or_else(|_| "{}".to_string());
    let system_prompt = "You are Sfumato's focused Marp slide repairer. Improve only the supplied slide fragment and return only its replacement Markdown.".to_string();
    let retry_feedback = retry
        .map(|retry| {
            format!(
                r#"
Corrective retry:
Your previous replacement was rejected by Sfumato.
Validation or rendering error:
{error}

Previous invalid replacement:
{invalid_response}

Return a corrected replacement for the original slide fragment below. Do not continue or patch the rejected replacement.
"#,
                error = excerpt(&retry.error, 4_000),
                invalid_response = excerpt(&retry.invalid_response, 12_000),
            )
        })
        .unwrap_or_default();
    let user_prompt = format!(
        r#"Repair the measured layout problem in this single Marp slide.

Original instruction: {instruction}
Deck title: {title}
Project: {project}
Theme: {theme_name}

Measured layout issue:
{issue_report}

Requirements:
- Return only replacement Markdown for the supplied slide, without a code fence.
- Do not return Marp frontmatter or a leading/trailing `---` separator.
- Preserve the slide's factual meaning and useful explanations.
- Prefer concise wording when it resolves the overflow.
- If necessary, split this slide into two coherent slides using one `---` separator between them.
- Do not add inline CSS, raw HTML, or alter global theme settings.
- Keep MathJax formulas and Mermaid diagrams valid.
{retry_feedback}

Exact slide fragment to replace:
```markdown
{slide_markdown}
```
"#,
        instruction = context.instruction,
        title = context.title,
        project = context.config.project_name,
        theme_name = context.theme.manifest.name,
        slide_markdown = context.slide_markdown,
    );
    let mut request = TextGenerationRequest::new(system_prompt, user_prompt);
    request.event_sink = event_sink;
    request
}

struct LayoutRepairRequestContext<'a> {
    config: &'a EffectiveConfig,
    theme: &'a ThemePackage,
    instruction: &'a str,
    title: &'a str,
    slide_markdown: &'a str,
    issue: &'a SlideLayoutIssue,
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

    for input in inputs {
        if input.is_file() {
            push_source_file(input, &mut documents)?;
        } else if input.is_dir() {
            for entry in WalkDir::new(input).into_iter().filter_map(Result::ok) {
                if entry.file_type().is_file() {
                    push_source_file(entry.path(), &mut documents)?;
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

fn push_source_file(path: &Path, documents: &mut Vec<SourceDocument>) -> Result<()> {
    if !is_supported(path) {
        return Ok(());
    }

    let content =
        fs::read_to_string(path).with_context(|| format!("Could not read {}", path.display()))?;
    documents.push(SourceDocument {
        path: path.to_path_buf(),
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
    documents
        .iter()
        .map(|document| {
            let excerpt = excerpt(&document.content, 6_000);
            format!(
                "\n--- SOURCE: {} ---\n{}\n",
                document.path.display(),
                excerpt
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
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

    if !markdown.contains("\n---") {
        bail!("Generated deck does not contain Marp slide separators.");
    }

    markdown = ensure_title_slide(markdown, title)?;

    Ok(markdown)
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
        let body = &after_marker[opening_line_end + 1..];
        let Some(body) = body.trim_end().strip_suffix(marker) else {
            continue;
        };
        return body.trim();
    }
    text
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
