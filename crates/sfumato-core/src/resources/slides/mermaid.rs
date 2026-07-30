//! Mermaid extraction, normalization, theming, and artifact rendering.

use super::*;

const MERMAID_IMAGE_HEIGHT_PX: u16 = 300;

/// Everything one Mermaid rendering pass needs.
pub(crate) struct MermaidRenderRequest<'a> {
    /// Markdown whose fences are replaced with rendered images.
    pub(crate) markdown: &'a str,
    /// Directory that receives the `.mmd` sources and `.svg` artifacts.
    pub(crate) diagrams_dir: &'a Path,
    /// Theme whose tokens colour the diagram.
    pub(crate) theme: &'a ThemePackage,
    /// Mermaid CLI adapter.
    pub(crate) renderer: &'a dyn DiagramRenderer,
    /// Filesystem the artifacts are written through.
    pub(crate) workspace: &'a dyn WorkspaceFileSystem,
    /// Cancellation and progress context.
    pub(crate) operation: &'a OperationContext,
    /// Stage the work is attributed to.
    pub(crate) stage: OperationStage,
    /// How a rendered diagram is written back into the Markdown.
    ///
    /// A slide constrains a diagram by height to fit its fixed box; a document
    /// constrains it by the width of its text column.
    pub(crate) image_markdown: fn(&str) -> String,
}

/// Renders every Mermaid fence to a themed SVG and rewrites it as an image.
pub(crate) async fn render_mermaid_diagrams(
    request: MermaidRenderRequest<'_>,
) -> Result<(String, Vec<PathBuf>)> {
    let MermaidRenderRequest {
        markdown,
        diagrams_dir,
        theme,
        renderer,
        workspace,
        operation,
        stage,
        image_markdown,
    } = request;
    operation.checkpoint(stage)?;
    let blocks = extract_mermaid_blocks(markdown)?;
    if blocks.is_empty() {
        return Ok((markdown.to_string(), Vec::new()));
    }

    workspace.create_dir_all(diagrams_dir)?;
    let mermaid_theme = mermaid_theme_config(&theme.manifest.tokens);
    let mut rendered = String::new();
    let mut cursor = 0;
    let mut artifacts = Vec::new();

    for block in &blocks {
        rendered.push_str(&markdown[cursor..block.start]);
        let source = normalize_mermaid_source(&block.source);
        let content_hash = format!("{:x}", Sha256::digest(source.as_bytes()));
        let name = format!("diagram-{}", &content_hash[..16]);
        let source_path = diagrams_dir.join(format!("{name}.mmd"));
        let artifact_path = diagrams_dir.join(format!("{name}.svg"));
        workspace.write(&source_path, source.as_bytes())?;
        renderer
            .render_svg(
                &source_path,
                &artifact_path,
                &mermaid_theme,
                operation,
                stage,
            )
            .await?;
        if !workspace.is_file(&artifact_path) {
            bail!(
                "Mermaid CLI did not write the expected SVG artifact {}",
                artifact_path.display()
            );
        }
        rendered.push_str(&image_markdown(&name));
        artifacts.push(source_path);
        artifacts.push(artifact_path);
        cursor = block.end;
    }

    rendered.push_str(&markdown[cursor..]);
    prune_unreferenced_diagrams(&rendered, diagrams_dir, workspace)?;
    Ok((rendered, artifacts))
}

pub(crate) fn mermaid_image_markdown(name: &str) -> String {
    format!("![height:{MERMAID_IMAGE_HEIGHT_PX}px](diagrams/{name}.svg)",)
}

fn prune_unreferenced_diagrams(
    markdown: &str,
    diagrams_dir: &Path,
    workspace: &dyn WorkspaceFileSystem,
) -> Result<()> {
    for entry in workspace.read_dir(diagrams_dir)? {
        let path = entry.path;
        if !entry.is_file {
            continue;
        }
        if path.extension().and_then(|value| value.to_str()) != Some("svg") {
            continue;
        }
        let filename = path
            .file_name()
            .context("Diagram artifact must have a filename")?
            .to_string_lossy();
        if markdown.contains(&format!("diagrams/{filename}")) {
            continue;
        }
        workspace.remove_file(&path)?;
        workspace.remove_file(&path.with_extension("mmd"))?;
    }
    Ok(())
}

pub(crate) fn mermaid_theme_config(tokens: &ThemeTokens) -> MermaidThemeConfig {
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

pub(crate) fn normalize_mermaid_source(source: &str) -> String {
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
pub(crate) struct MermaidBlock {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) source: String,
}

pub(crate) fn extract_mermaid_blocks(markdown: &str) -> Result<Vec<MermaidBlock>> {
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
