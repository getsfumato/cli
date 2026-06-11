use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use slug::slugify;
use walkdir::WalkDir;

use crate::{
    config::{Capability, EffectiveConfig},
    generation::{GenerationOutput, GenerationRequest},
    providers::{TextGenerationRequest, build_text_provider},
    renderers::marp,
};

const SUPPORTED_EXTENSIONS: &[&str] = &[
    "md", "txt", "rs", "py", "js", "ts", "html", "css", "json", "toml", "yaml", "yml",
];

#[derive(Debug)]
pub struct GenerateSlidesOptions {
    pub title: Option<String>,
    pub dry_run: bool,
}

#[derive(Debug)]
pub struct GenerateSlidesResult {
    pub markdown_path: PathBuf,
    pub pdf_path: Option<PathBuf>,
    pub output: GenerationOutput,
    pub prompt_preview: Option<String>,
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

    ensure_inside(&output_root, &markdown_path)?;
    ensure_inside(&output_root, &pdf_path)?;

    let documents = collect_sources(&request.sources)?;
    let source_bundle = build_source_bundle(&documents);
    let provider_request =
        build_generation_request(&config, &request.instruction, &title, &source_bundle);
    let (profile_name, profile) = config.resolve_model(Capability::Text)?;
    let selected_models =
        std::collections::BTreeMap::from([("text".to_string(), profile_name.to_string())]);

    if options.dry_run {
        return Ok(GenerateSlidesResult {
            markdown_path,
            pdf_path: None,
            output: GenerationOutput {
                project: config.project_name,
                models: selected_models,
                artifacts: Vec::new(),
            },
            prompt_preview: Some(provider_request.user_prompt),
        });
    }

    let provider = build_text_provider(&config, profile)?;
    let response = provider.generate_text(provider_request).await?;
    let markdown = normalize_marp_markdown(&response.text, &config, &title)?;

    fs::create_dir_all(&slides_dir)
        .with_context(|| format!("Could not create {}", slides_dir.display()))?;
    fs::write(&markdown_path, markdown)
        .with_context(|| format!("Could not write {}", markdown_path.display()))?;

    let rendered_pdf = if config.marp.pdf {
        match marp::render_pdf(&markdown_path, &pdf_path).await {
            Ok(()) => Some(pdf_path),
            Err(error) => {
                eprintln!("PDF export skipped: {error}");
                None
            }
        }
    } else {
        None
    };

    let mut artifacts = vec![markdown_path.clone()];
    if let Some(pdf) = &rendered_pdf {
        artifacts.push(pdf.clone());
    }

    Ok(GenerateSlidesResult {
        markdown_path,
        pdf_path: rendered_pdf,
        output: GenerationOutput {
            project: config.project_name,
            models: selected_models,
            artifacts,
        },
        prompt_preview: None,
    })
}

fn build_generation_request(
    config: &EffectiveConfig,
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
Theme: {theme}
Marp theme: {marp_theme}
Instruction: {instruction}

Requirements:
- Return only Markdown.
- Include Marp frontmatter.
- Use slide separators with ---.
- Include a title slide.
- Include a short learning objective slide.
- Explain the source material for a student.
- Use examples from the provided files.
- Add presenter notes with short teaching cues when useful.

Source material:
{source_bundle}
"#,
        project = config.project_name,
        theme = config.user.theme,
        marp_theme = config.marp.theme,
    );

    TextGenerationRequest {
        system_prompt,
        user_prompt,
    }
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

    if !markdown.starts_with("---") {
        markdown = format!(
            "---\nmarp: true\ntheme: {}\npaginate: true\n---\n\n{}",
            config.marp.theme, markdown
        );
    }

    if !markdown.contains("marp: true") {
        markdown = markdown.replacen("---", "---\nmarp: true", 1);
    }

    if !markdown.contains("\n---") {
        bail!("Generated deck does not contain Marp slide separators.");
    }

    if !markdown.to_lowercase().contains(&title.to_lowercase()) {
        markdown = markdown.replacen("---", &format!("---\n\n# {title}\n\n---"), 2);
    }

    Ok(markdown)
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
