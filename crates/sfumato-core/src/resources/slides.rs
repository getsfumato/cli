use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use slug::slugify;
use walkdir::WalkDir;

use crate::{
    config::{Capability, EffectiveConfig},
    generation::{GenerationOutput, GenerationRequest, GenerationToolSummary},
    providers::{TextGenerationEvent, TextGenerationRequest, ToolDefinition, build_text_provider},
    renderers::{diagrams::MermaidDiagramRenderer, marp},
    themes::{ThemePackage, ThemeService},
    tools::default_filesystem_tools,
};

const SUPPORTED_EXTENSIONS: &[&str] = &[
    "md", "txt", "rs", "py", "js", "ts", "html", "css", "json", "toml", "yaml", "yml",
];

pub struct GenerateSlidesOptions {
    pub title: Option<String>,
    pub dry_run: bool,
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
    let output_root = config.output_root()?;
    let slides_dir = output_root.join("slides");
    let title = options
        .title
        .unwrap_or_else(|| instruction_title(&request.instruction));
    let slug = slugify(&title);
    let markdown_path = slides_dir.join(format!("{slug}.md"));
    let pdf_path = slides_dir.join(format!("{slug}.pdf"));
    let theme_css_path = slides_dir
        .join("themes")
        .join(format!("{}.css", config.theme));
    let diagrams_dir = slides_dir.join("diagrams");

    ensure_inside(&output_root, &markdown_path)?;
    ensure_inside(&output_root, &pdf_path)?;
    ensure_inside(&output_root, &theme_css_path)?;
    ensure_inside(&output_root, &diagrams_dir)?;

    let theme = ThemeService::load()?.resolve(&config.theme)?;
    let documents = collect_sources(&request.sources)?;
    let tool_set = default_filesystem_tools(&config.project_root, &request.sources)?;
    let tool_summaries = summarize_tools(&tool_set.definitions);
    let source_bundle = build_source_bundle(&documents);
    let mut provider_request = build_generation_request(
        &config,
        &theme,
        &request.instruction,
        &title,
        &source_bundle,
    );
    provider_request.tools = tool_set.definitions;
    provider_request.tool_executor = Some(tool_set.executor);
    provider_request.event_sink = options.event_sink;
    let (profile_name, profile) = config.resolve_model(Capability::Text)?;
    provider_request.max_tool_rounds = model_tool_rounds(profile);
    let selected_models =
        std::collections::BTreeMap::from([("text".to_string(), profile_name.to_string())]);

    if options.dry_run {
        return Ok(GenerateSlidesResult {
            markdown_path,
            pdf_path: None,
            output: GenerationOutput {
                project: config.project_name,
                models: selected_models,
                tools: tool_summaries.clone(),
                artifacts: Vec::new(),
            },
            prompt_preview: Some(provider_request.user_prompt),
            tool_summaries,
            warnings: Vec::new(),
        });
    }

    let provider = build_text_provider(&config, profile)?;
    let response = provider.generate_text(provider_request).await?;
    let markdown = normalize_marp_markdown(&response.text, &config, &title)?;

    fs::create_dir_all(&slides_dir)
        .with_context(|| format!("Could not create {}", slides_dir.display()))?;
    let (markdown, diagram_artifacts) =
        render_mermaid_diagrams(&markdown, &diagrams_dir, &slug).await?;
    copy_theme_css(&theme, &theme_css_path)?;
    fs::write(&markdown_path, markdown)
        .with_context(|| format!("Could not write {}", markdown_path.display()))?;

    let mut warnings = Vec::new();
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
        },
        prompt_preview: None,
        tool_summaries,
        warnings,
    })
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
) -> Result<(String, Vec<PathBuf>)> {
    let blocks = extract_mermaid_blocks(markdown)?;
    if blocks.is_empty() {
        return Ok((markdown.to_string(), Vec::new()));
    }

    fs::create_dir_all(diagrams_dir)
        .with_context(|| format!("Could not create {}", diagrams_dir.display()))?;
    let renderer = MermaidDiagramRenderer;
    let mut rendered = String::new();
    let mut cursor = 0;
    let mut artifacts = Vec::new();

    for (index, block) in blocks.iter().enumerate() {
        rendered.push_str(&markdown[cursor..block.start]);
        let diagram_index = index + 1;
        let source_path = diagrams_dir.join(format!("{slug}-diagram-{diagram_index}.mmd"));
        let artifact_path = diagrams_dir.join(format!("{slug}-diagram-{diagram_index}.svg"));
        fs::write(&source_path, &block.source)
            .with_context(|| format!("Could not write {}", source_path.display()))?;
        let _svg = renderer.render_svg(&source_path, &artifact_path).await?;
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
    title: &str,
    source_bundle: &str,
) -> TextGenerationRequest {
    let learning_style = if config.user.learning_style.is_empty() {
        "not specified".to_string()
    } else {
        config.user.learning_style.join(", ")
    };

    let system_prompt = format!(
        "You are Sfumato, a careful study-resource generator. Create clear, accurate Marp slide decks for Obsidian users. Use the user's learning preferences: {learning_style}. Prefer concise slides, useful examples, and presenter notes when they help."
    );

    let user_prompt = format!(
        r#"Create a Marp Markdown slide deck titled "{title}".

Project: {project}
Allowed filesystem root: {project_root}
Theme: {theme_name}
Theme colors: {theme_colors}
Theme fonts: {theme_fonts}
Instruction: {instruction}

Requirements:
- Return only Markdown.
- Include Marp frontmatter.
- Set Marp math rendering to MathJax with `math: mathjax`.
- Use slide separators with ---.
- Include a title slide.
- You may use Mermaid diagrams in fenced ```mermaid blocks when a visual structure helps.
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

fn instruction_title(instruction: &str) -> String {
    let compact = instruction.split_whitespace().collect::<Vec<_>>().join(" ");
    compact.chars().take(60).collect()
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

    if !markdown.starts_with("---") {
        markdown = format!(
            "---\nmarp: true\ntheme: {}\npaginate: true\nmath: mathjax\n---\n\n{}",
            config.theme, markdown
        );
    }

    if !markdown.contains("marp: true") {
        markdown = markdown.replacen("---", "---\nmarp: true", 1);
    }
    markdown = set_frontmatter_value(markdown, "theme", &config.theme);
    markdown = set_frontmatter_value(markdown, "math", "mathjax");

    if !markdown.contains("\n---") {
        bail!("Generated deck does not contain Marp slide separators.");
    }

    if !markdown.to_lowercase().contains(&title.to_lowercase()) {
        markdown = markdown.replacen("---", &format!("---\n\n# {title}\n\n---"), 2);
    }

    Ok(markdown)
}

fn sanitize_marp_markdown(markdown: &str) -> String {
    let without_svg = strip_html_blocks(markdown, "svg");
    remove_html_tags_by_names(
        &without_svg,
        &["article", "div", "section", "span", "p", "br", "svg"],
    )
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

fn set_frontmatter_value(markdown: String, key: &str, value: &str) -> String {
    let closing = markdown[3..]
        .find("\n---")
        .map(|index| index + 3)
        .unwrap_or(markdown.len());
    let (frontmatter, body) = markdown.split_at(closing);
    let prefix = format!("{key}:");
    if frontmatter
        .lines()
        .any(|line| line.trim_start().starts_with(&prefix))
    {
        let mut replaced = false;
        let frontmatter = frontmatter
            .lines()
            .map(|line| {
                if !replaced && line.trim_start().starts_with(&prefix) {
                    replaced = true;
                    format!("{key}: {value}")
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!("{frontmatter}{body}")
    } else {
        markdown.replacen("---", &format!("---\n{key}: {value}"), 1)
    }
}

fn strip_code_fence(text: &str) -> &str {
    let without_opening = text
        .strip_prefix("```markdown")
        .or_else(|| text.strip_prefix("```md"))
        .or_else(|| text.strip_prefix("```"))
        .unwrap_or(text);

    without_opening
        .strip_suffix("```")
        .unwrap_or(without_opening)
        .trim()
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
