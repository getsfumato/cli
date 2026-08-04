//! Markdown-to-printable-HTML assembly for paginated documents.
//!
//! The page furniture a document needs — sheet size, margins, running headers,
//! page numbers, a table of contents that knows page numbers — is CSS Paged
//! Media, which no browser implements directly. Paged.js paginates in the DOM
//! instead, drawing real page boxes the renderer can then print and measure.
//! Everything here is deterministic: the same document and theme always produce
//! the same HTML, byte for byte.

use anyhow::{Context, Result, bail};
use comrak::{Options, markdown_to_html};
use sfumato_core::{
    errors::{ErrorClass, OperationStage, SfumatoError, SfumatoResult},
    generation::DocumentPageSetup,
    renderers::{AssembledDocument, DocumentAssembler, DocumentAssemblyRequest, SectionedDocument},
    themes::ThemePackage,
};

use crate::pages::{
    MATHJAX_CONFIG, MATHJAX_CSS, MATHJAX_RUNTIME, escape_attribute, escape_script, escape_text,
    mathjax_runtime,
};

const DEFAULT_PRINT_CSS: &str = include_str!("../assets/document-runtimes/default-print.css");

/// Compiles validated document Markdown into paginatable HTML.
#[derive(Clone, Copy, Debug, Default)]
pub struct PagedDocumentAssembler;

impl DocumentAssembler for PagedDocumentAssembler {
    fn assemble(&self, request: DocumentAssemblyRequest<'_>) -> SfumatoResult<AssembledDocument> {
        assemble_document(request).map_err(|error| {
            SfumatoError::render(ErrorClass::InvalidOutput, format!("{error:#}"))
                .at_stage(OperationStage::Render)
        })
    }
}

fn assemble_document(request: DocumentAssemblyRequest<'_>) -> Result<AssembledDocument> {
    let markdown = request
        .document
        .render()
        .map_err(|error| anyhow::anyhow!("{error}"))
        .context("Could not render the document Markdown")?;
    let body_markdown = strip_title_heading(&markdown);
    let rendered = markdown_to_html(&body_markdown, &document_markdown_options());
    let uses_math = rendered.contains("data-math-style");
    let body = restore_math_delimiters(&rendered);
    validate_local_references(&body, request.allowed_assets)?;

    let mut runtimes = Vec::new();

    let mut head = String::new();
    head.push_str(&format!(
        "<style data-sfumato-page-setup>{}</style>\n",
        page_setup_css(request.setup)
    ));
    head.push_str(&format!(
        "<style data-sfumato-theme>{}</style>\n",
        theme_print_css(request.theme)?
    ));
    if uses_math {
        head.push_str(&format!("<style data-sfumato-math>{MATHJAX_CSS}</style>\n"));
    }

    let mut content = String::new();
    if request.setup.cover {
        content.push_str(&cover_html(&request));
    }
    if request.setup.table_of_contents {
        content.push_str(&table_of_contents_html(request.document, &body));
    }
    content.push_str(&format!(
        "<main class=\"sfumato-document\">\n{body}</main>\n"
    ));

    let mut scripts = String::new();
    if uses_math {
        // Math has to be typeset before pagination measures anything, or the
        // page breaks are computed against placeholder-sized formulas.
        let runtime = mathjax_runtime()?;
        scripts.push_str(MATHJAX_CONFIG);
        scripts.push_str(&format!(
            "<script data-sfumato-runtime=\"mathjax\" data-version=\"{}\">{}</script>\n",
            escape_attribute(&runtime.version),
            escape_script(MATHJAX_RUNTIME),
        ));
        runtimes.push(runtime);
    }

    let html = format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<title>{}</title>\n{head}</head>\n<body>\n{content}{scripts}</body>\n</html>\n",
        escape_text(request.document.title())
    );
    Ok(AssembledDocument { html, runtimes })
}

/// GitHub-flavoured options with the extensions a study document actually uses.
fn document_markdown_options() -> Options<'static> {
    let mut options = Options::default();
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.footnotes = true;
    options.extension.tasklist = true;
    options.extension.superscript = true;
    options.extension.description_lists = true;
    // Math is parsed rather than passed through: CommonMark reads `\(` as an
    // escaped parenthesis and would eat the delimiter, and emphasis markers
    // inside a formula would be styled as prose. Parsing protects the content;
    // `restore_math_delimiters` then hands MathJax the delimiters it expects.
    options.extension.math_dollars = true;
    options.extension.math_latex = true;
    // Stable anchors let the generated contents link into the body, and the
    // renderer resolve each entry's page number from the target.
    options.extension.header_id_prefix = Some(String::new());
    options.render.r#unsafe = false;
    options
}

/// Turns comrak's math spans back into the delimiters MathJax scans for.
fn restore_math_delimiters(html: &str) -> String {
    let mut output = String::with_capacity(html.len());
    let mut remainder = html;
    while let Some(start) = remainder.find("<span data-math-style=\"") {
        output.push_str(&remainder[..start]);
        let tail = &remainder[start..];
        let Some(style_end) = tail[23..].find('"').map(|offset| offset + 23) else {
            break;
        };
        let display = &tail[23..style_end] == "display";
        let Some(open_end) = tail.find('>') else {
            break;
        };
        let Some(close) = tail.find("</span>") else {
            break;
        };
        let content = &tail[open_end + 1..close];
        let (open, shut) = if display {
            ("\\[", "\\]")
        } else {
            ("\\(", "\\)")
        };
        output.push_str(open);
        output.push_str(content);
        output.push_str(shut);
        remainder = &tail[close + "</span>".len()..];
    }
    output.push_str(remainder);
    output
}

/// Removes the level-1 title, which the cover and running header already carry.
///
/// The frontmatter is dropped by position rather than by pattern. It used to be
/// removed by deleting any line that trimmed to `---` or started with `subtitle:`
/// while a `removed` flag was still false — with that flag doubling as "am I still
/// in the frontmatter?". It answers that wrongly the moment the title match fails,
/// and the match compared a trimmed line against an untrimmed pattern, so one
/// trailing space in the title was enough: every thematic break in the document was
/// deleted, every `subtitle:` line was deleted including inside fenced code, and
/// the H1 survived into the body to duplicate the cover.
///
/// Parsing the leading block instead makes that impossible rather than unlikely: a
/// `---` or a `subtitle:` further down is never a candidate, whatever the title
/// looks like.
fn strip_title_heading(markdown: &str) -> String {
    let body = strip_leading_frontmatter(markdown);
    let mut output = String::with_capacity(body.len());
    let mut removed = false;
    for line in body.lines() {
        // The document's first level-1 heading is its title, whatever the text
        // says. Reconstructing `# {title}` and matching it was the fragility
        // itself: it required the title and the heading to agree character for
        // character, and the caller's title has been through the domain's heading
        // cleaning while the line in the file has not.
        if !removed && is_level_one_heading(line) {
            removed = true;
            continue;
        }
        output.push_str(line);
        output.push('\n');
    }
    output
}

/// Reports whether a line is an ATX level-1 heading.
///
/// `#` followed by whitespace, so `##` and a bare `#hashtag` are not headings.
/// Indentation of four or more spaces is a code block rather than a heading.
fn is_level_one_heading(line: &str) -> bool {
    let indent = line.len() - line.trim_start().len();
    if indent >= 4 {
        return false;
    }
    line.trim_start()
        .strip_prefix('#')
        .is_some_and(|rest| rest.starts_with(char::is_whitespace))
}

/// Returns the document without its leading `---` delimited frontmatter block.
///
/// An unterminated opening block is left alone: deleting to the end of the file is
/// never the right reading of a malformed document.
fn strip_leading_frontmatter(markdown: &str) -> &str {
    let trimmed = markdown.trim_start_matches(['\n', '\r', ' ', '\t']);
    // The opening delimiter must be the first thing in the document.
    let mut rest = match trimmed.split_once('\n') {
        Some((first, rest)) if first.trim() == "---" => rest,
        _ => return markdown,
    };
    loop {
        match rest.split_once('\n') {
            Some((line, after)) if line.trim() == "---" => return after,
            Some((_, after)) => rest = after,
            None => return markdown,
        }
    }
}

/// Emits the `@page` rules that give the sheet its real geometry.
///
/// The margin is zero on purpose: Paged.js draws the real margins inside the
/// page box, so a Chrome margin on top of that would shrink every page twice.
fn page_setup_css(setup: DocumentPageSetup) -> String {
    format!(
        "@page {{ size: {}; margin: 0; }}\n:root {{ --sfumato-page-size: {}; }}\n",
        setup.page_size.css_size(),
        setup.page_size.as_str()
    )
}

/// Returns the theme's print stylesheet, or the bundled fallback.
fn theme_print_css(theme: &ThemePackage) -> Result<String> {
    match theme.document_css_path() {
        Some(path) => std::fs::read_to_string(&path)
            .with_context(|| format!("Could not read theme print CSS {}", path.display())),
        // A theme installed before documents existed still has to render, so the
        // fallback is derived from the tokens every theme already declares.
        None => Ok(fallback_print_css(theme)),
    }
}

fn fallback_print_css(theme: &ThemePackage) -> String {
    let token = |group: &std::collections::BTreeMap<String, String>, key: &str, default: &str| {
        group
            .get(key)
            .cloned()
            .unwrap_or_else(|| default.to_owned())
    };
    let colors = &theme.manifest.tokens.colors;
    let fonts = &theme.manifest.tokens.fonts;
    format!(
        ":root {{\n  --sfumato-background: {};\n  --sfumato-surface: {};\n  --sfumato-text: {};\n  --sfumato-muted: {};\n  --sfumato-primary: {};\n  --sfumato-accent: {};\n  --sfumato-body-font: {};\n  --sfumato-heading-font: {};\n  --sfumato-mono-font: {};\n}}\n{DEFAULT_PRINT_CSS}",
        token(colors, "background", "#ffffff"),
        token(colors, "surface", "#f5f5f5"),
        token(colors, "text", "#1a1a1a"),
        token(colors, "muted", "#5f6368"),
        token(colors, "primary", "#315c8c"),
        token(colors, "accent", "#b35c24"),
        token(fonts, "body", "Georgia, serif"),
        token(fonts, "heading", "Helvetica, Arial, sans-serif"),
        token(fonts, "mono", "monospace"),
    )
}

/// Builds the cover page from data Sfumato owns.
///
/// Deliberately not authored by the model: one structure for every document, and
/// nothing a review patch can dismantle.
fn cover_html(request: &DocumentAssemblyRequest<'_>) -> String {
    let mut cover = String::from("<section class=\"sfumato-cover\">\n");
    if let Some(image) = request.theme.document_cover_image_path()
        && let Some(name) = image.file_name().and_then(|value| value.to_str())
    {
        cover.push_str(&format!(
            "<img class=\"sfumato-cover-image\" src=\"{}\" alt=\"\">\n",
            escape_attribute(name)
        ));
    }
    cover.push_str(&format!(
        "<h1 class=\"sfumato-cover-title\">{}</h1>\n",
        escape_text(request.document.title())
    ));
    if let Some(subtitle) = request.document.subtitle() {
        cover.push_str(&format!(
            "<p class=\"sfumato-cover-subtitle\">{}</p>\n",
            escape_text(subtitle)
        ));
    }
    cover.push_str(&format!(
        "<p class=\"sfumato-cover-meta\"><span class=\"sfumato-cover-project\">{}</span><span class=\"sfumato-cover-date\">{}</span></p>\n</section>\n",
        escape_text(request.project),
        escape_text(request.revision_date)
    ));
    cover
}

/// Builds the contents list from the document's own outline.
///
/// Page numbers come from CSS `target-counter()`, which Paged.js resolves after
/// pagination, so the numbers cannot disagree with where the sections landed.
/// Builds the contents from the anchors the renderer actually emitted.
///
/// The anchors are read out of the rendered body rather than recomputed. The
/// previous version reimplemented comrak's rule and reproduced it faithfully
/// except for the duplicate counter: comrak disambiguates a repeated heading with
/// `-1`, `-2`, while the reimplementation emitted the same anchor every time. So
/// the second "Conclusión" linked to the first — and because page numbers come from
/// CSS `target-counter()` resolved against that anchor, printed the first one's
/// page number too. Repeated headings are the normal case in a study document, and
/// a printed contents page is not navigable, so the wrong number is undetectable
/// from the PDF.
///
/// Reading the emitted ids removes the invariant rather than maintaining it.
fn table_of_contents_html(document: &SectionedDocument, body: &str) -> String {
    let mut anchors = heading_anchors(body).into_iter();
    let mut contents = String::from(
        "<nav class=\"sfumato-contents\" role=\"doc-toc\">\n<h2 class=\"sfumato-contents-title\">Contents</h2>\n<ol class=\"sfumato-contents-list\">\n",
    );
    for (level, heading) in document.outline() {
        // Positional: the outline and the rendered headings are the same sequence,
        // because the body is rendered from this document.
        let Some(anchor) = anchors.next() else {
            continue;
        };
        contents.push_str(&format!(
            "<li class=\"sfumato-contents-entry\" data-level=\"{level}\"><a href=\"#{}\">{}</a></li>\n",
            escape_attribute(&anchor),
            escape_text(heading)
        ));
    }
    contents.push_str("</ol>\n</nav>\n");
    contents
}

/// Collects the `id` of every heading in the rendered body, in document order.
///
/// The title heading is stripped before rendering, so what remains is exactly the
/// document outline.
fn heading_anchors(body: &str) -> Vec<String> {
    let mut anchors = Vec::new();
    let mut remainder = body;
    while let Some(start) = remainder.find("<h") {
        let tail = &remainder[start..];
        // `<h2 ...>` and nothing else: `<hr`, `<header` and the like must not match.
        let is_heading = tail
            .as_bytes()
            .get(2)
            .is_some_and(|level| level.is_ascii_digit());
        let Some(open_end) = tail.find('>') else {
            break;
        };
        if is_heading && let Some(id) = attribute_value(&tail[..open_end], "id=\"") {
            anchors.push(id);
        }
        remainder = &tail[open_end + 1..];
    }
    anchors
}

/// Reads one double-quoted attribute value out of an opening tag.
fn attribute_value(tag: &str, attribute: &str) -> Option<String> {
    let start = tag.find(attribute)? + attribute.len();
    let end = tag[start..].find('"')? + start;
    Some(tag[start..end].to_owned())
}

/// Rejects any reference a deterministic offline render could not resolve.
fn validate_local_references(html: &str, allowed_assets: &[std::path::PathBuf]) -> Result<()> {
    for forbidden in ["http://", "https://", "//", "file:"] {
        if let Some(position) = html.find(&format!("src=\"{forbidden}")) {
            bail!(
                "Document markup references a remote resource: {}",
                html[position..].chars().take(80).collect::<String>()
            );
        }
    }
    let names = allowed_assets
        .iter()
        .filter_map(|path| path.file_name().and_then(|value| value.to_str()))
        .collect::<Vec<_>>();
    let mut remainder = html;
    while let Some(start) = remainder.find("src=\"") {
        remainder = &remainder[start + 5..];
        let Some(end) = remainder.find('"') else {
            break;
        };
        let source = &remainder[..end];
        if source.starts_with("data:") {
            remainder = &remainder[end..];
            continue;
        }
        if source.contains("..") {
            bail!("Document markup references `{source}`, which escapes the render workspace");
        }
        // An image the workflow never materialized renders as a broken box in a
        // PDF nobody can fix after the fact, so it fails here instead.
        if !names.iter().any(|name| source.ends_with(name)) {
            bail!("Document markup references `{source}`, which is not a generated asset");
        }
        remainder = &remainder[end..];
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/unit/documents.rs"]
mod tests;
