//! Static page assembly and browser-backed inspection adapters.

use std::{collections::BTreeSet, path::Path};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use html5ever::{parse_document, tendril::TendrilSink};
use lightningcss::{stylesheet::ParserOptions, stylesheet::StyleSheet};
use markup5ever_rcdom::RcDom;
use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;
use serde::Deserialize;
use sfumato_core::{
    errors::{ErrorClass, OperationStage, SfumatoError, SfumatoResult},
    generation::{PageInspectionIssue, PageIssueKind, PageRuntimeSelection},
    operation::OperationContext,
    renderers::{AssembledPage, PageAssembler, PageAssemblyRequest, PageInspector},
};
use sha2::{Digest, Sha256};
use tokio::process::Command;

use crate::{renderers::resolved_browser_path, runtime::run_command};

const CONTENT_SLOT: &str = "<!-- SFUMATO_CONTENT -->";
const CSP: &str = "default-src 'none'; script-src 'unsafe-inline' data:; style-src 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:; media-src 'self' data:; connect-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'";
const MATHJAX_RUNTIME: &str = include_str!("../assets/page-runtimes/mathjax/runtime.js");
const MATHJAX_LICENSE: &str = include_str!("../assets/page-runtimes/mathjax/LICENSE");
const MATHJAX_VERSION: &str = "3.2.2";
const MATHJAX_RUNTIME_HASH: &str =
    "a4354ff94fd868aea0cc6eaaa79a57fda0588646fc46ee3700a349ee0a11cbe6";
const MATHJAX_CONFIG: &str = r#"<script data-sfumato-math-config>
window.MathJax = {
  tex: {
    inlineMath: [['\\(', '\\)']],
    displayMath: [['\\[', '\\]'], ['$$', '$$']],
    processEscapes: true
  },
  svg: { fontCache: 'local' },
  options: { skipHtmlTags: ['script', 'noscript', 'style', 'textarea', 'pre', 'code'] }
};
</script>
"#;
const MATHJAX_CSS: &str = r#"mjx-container[jax="SVG"] {
  color: inherit;
  max-width: 100%;
  overflow-x: auto;
  overflow-y: hidden;
}
mjx-container[display="true"] {
  margin: 1.25rem 0 !important;
}
"#;

/// Validates model fragments and compiles them into a theme-owned HTML shell.
#[derive(Clone, Copy, Debug, Default)]
pub struct StandalonePageAssembler;

impl PageAssembler for StandalonePageAssembler {
    fn assemble(&self, request: PageAssemblyRequest<'_>) -> SfumatoResult<AssembledPage> {
        assemble_page(request).map_err(|error| {
            SfumatoError::render(ErrorClass::InvalidOutput, format!("{error:#}"))
                .at_stage(OperationStage::Render)
        })
    }
}

/// Inspects standalone pages in a locally installed Chromium-family browser.
#[derive(Clone, Copy, Debug, Default)]
pub struct ChromiumPageInspector;

#[async_trait]
impl PageInspector for ChromiumPageInspector {
    async fn inspect(
        &self,
        html_path: &Path,
        browser_path: Option<&Path>,
        operation: &OperationContext,
    ) -> SfumatoResult<Vec<PageInspectionIssue>> {
        inspect_page(html_path, browser_path, operation)
            .await
            .map_err(page_inspection_error)
    }
}

fn assemble_page(request: PageAssemblyRequest<'_>) -> Result<AssembledPage> {
    let adapter = request
        .theme
        .manifest
        .adapters
        .html
        .as_ref()
        .context("Selected theme does not provide an HTML adapter")?;
    let shell_path = request.theme.root.join(&adapter.shell);
    let css_path = request.theme.root.join(&adapter.css);
    let mut shell = std::fs::read_to_string(&shell_path)
        .with_context(|| format!("Could not read HTML theme shell {}", shell_path.display()))?;
    if shell.matches(CONTENT_SLOT).count() != 1 {
        bail!("HTML theme shell must contain exactly one {CONTENT_SLOT}");
    }
    if !shell.contains("</head>") || !shell.contains("</body>") {
        bail!("HTML theme shell must contain closing head and body elements");
    }

    validate_html_fragment(request.document.body_html())?;
    validate_css(request.document.css(), "generated page CSS")?;
    validate_javascript(request.document.javascript(), "generated page JavaScript")?;
    validate_fragment_policy(request.document.body_html(), request.allowed_assets)?;
    validate_css_policy(request.document.css())?;
    validate_javascript_policy(request.document.javascript())?;

    let theme_css = std::fs::read_to_string(&css_path)
        .with_context(|| format!("Could not read HTML theme CSS {}", css_path.display()))?;
    validate_css(&theme_css, "theme CSS")?;
    let theme_javascript = adapter
        .script
        .as_ref()
        .map(|path| {
            let path = request.theme.root.join(path);
            std::fs::read_to_string(&path)
                .with_context(|| format!("Could not read HTML theme script {}", path.display()))
        })
        .transpose()?
        .unwrap_or_default();
    validate_javascript(&theme_javascript, "theme JavaScript")?;

    shell = remove_theme_asset_references(shell, &adapter.css, adapter.script.as_deref());
    shell = replace_title(shell, request.document.title());
    let uses_math = contains_tex_math(request.document.body_html());
    let inspection_bootstrap = if request.inspection {
        INSPECTION_BOOTSTRAP
    } else {
        ""
    };
    let math_css = if uses_math { MATHJAX_CSS } else { "" };
    let plugin_css = request
        .plugins
        .iter()
        .map(|plugin| plugin.stylesheet.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    validate_css(&plugin_css, "page plugin CSS")?;
    let head = format!(
        "<meta http-equiv=\"Content-Security-Policy\" content=\"{CSP}\">\n<style data-sfumato-theme>{}</style>\n<style data-sfumato-plugins>{}</style>\n<style data-sfumato-page>{}</style>\n<style data-sfumato-math>{math_css}</style>\n{inspection_bootstrap}",
        escape_style(&theme_css),
        escape_style(&plugin_css),
        escape_style(request.document.css()),
    );
    shell = shell.replacen("</head>", &format!("{head}\n</head>"), 1);

    let body = format!(
        "<div id=\"sfumato-page\">{}</div>",
        request.document.body_html()
    );
    shell = shell.replacen(CONTENT_SLOT, &body, 1);

    let mut scripts =
        String::from("<script>window.SfumatoPlugins = window.SfumatoPlugins || {};</script>\n");
    let mut runtimes = Vec::new();
    if uses_math {
        let runtime = mathjax_runtime()?;
        scripts.push_str(MATHJAX_CONFIG);
        scripts.push_str(&format!(
            "<script data-sfumato-runtime=\"mathjax\" data-version=\"{}\">{}</script>\n",
            runtime.version,
            escape_script(MATHJAX_RUNTIME),
        ));
        runtimes.push(runtime);
    }
    for plugin in request.plugins {
        scripts.push_str(&format!(
            "<script data-sfumato-plugin=\"{}\" data-version=\"{}\">{}</script>\n",
            escape_attribute(&plugin.summary.id),
            escape_attribute(&plugin.summary.version),
            escape_script(&plugin.runtime_javascript),
        ));
    }
    if !theme_javascript.trim().is_empty() {
        scripts.push_str(&format!(
            "<script data-sfumato-theme>{}</script>\n",
            escape_script(&theme_javascript)
        ));
    }
    if !request.document.javascript().trim().is_empty() {
        scripts.push_str(&format!(
            "<script data-sfumato-page>{}</script>\n",
            escape_script(request.document.javascript())
        ));
    }
    if request.inspection {
        scripts.push_str(INSPECTION_REPORTER);
    }
    Ok(AssembledPage {
        html: shell.replacen("</body>", &format!("{scripts}</body>"), 1),
        runtimes,
    })
}

fn contains_tex_math(fragment: &str) -> bool {
    fragment.contains("\\(") || fragment.contains("\\[") || fragment.contains("$$")
}

fn mathjax_runtime() -> Result<PageRuntimeSelection> {
    if MATHJAX_LICENSE.trim().is_empty() {
        bail!("Bundled MathJax license is missing");
    }
    let runtime_hash = format!("{:x}", Sha256::digest(MATHJAX_RUNTIME.as_bytes()));
    if runtime_hash != MATHJAX_RUNTIME_HASH {
        bail!("Bundled MathJax runtime failed its integrity check");
    }
    Ok(PageRuntimeSelection {
        id: "mathjax".into(),
        name: "MathJax TeX to SVG".into(),
        version: MATHJAX_VERSION.into(),
        runtime_hash,
    })
}

fn validate_html_fragment(fragment: &str) -> Result<()> {
    let wrapped = format!(
        "<!doctype html><html><head><title>fragment</title></head><body><main>{fragment}</main></body></html>"
    );
    let dom = parse_document(RcDom::default(), Default::default()).one(wrapped);
    let errors = dom.errors.borrow();
    if let Some(error) = errors.first() {
        bail!("Generated body HTML is invalid: {error}");
    }
    Ok(())
}

fn validate_css(css: &str, label: &str) -> Result<()> {
    StyleSheet::parse(
        css,
        ParserOptions {
            filename: label.to_string(),
            ..ParserOptions::default()
        },
    )
    .map(|_| ())
    .map_err(|error| anyhow::anyhow!("Invalid {label}: {error}"))
}

fn validate_javascript(javascript: &str, label: &str) -> Result<()> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, javascript, SourceType::cjs()).parse();
    if let Some(error) = parsed.errors.first() {
        bail!("Invalid {label}: {error}");
    }
    Ok(())
}

fn validate_fragment_policy(fragment: &str, allowed_assets: &[std::path::PathBuf]) -> Result<()> {
    for forbidden in [
        "script", "style", "html", "head", "body", "iframe", "object", "embed", "base",
    ] {
        if contains_html_tag(fragment, forbidden) {
            bail!("Generated body HTML contains forbidden element '<{forbidden}'");
        }
    }
    reject_remote_or_traversal(fragment, "body HTML")?;

    let allowed = allowed_assets
        .iter()
        .filter_map(|path| path.file_name().and_then(|name| name.to_str()))
        .map(|name| format!("assets/images/{name}"))
        .collect::<BTreeSet<_>>();
    for source in quoted_attribute_values(fragment, "src") {
        if source.starts_with("data:") || source.starts_with("blob:") {
            continue;
        }
        if !allowed.contains(source) {
            bail!("Generated body HTML references unregistered asset '{source}'");
        }
    }
    Ok(())
}

fn contains_html_tag(fragment: &str, tag: &str) -> bool {
    let lowercase = fragment.to_ascii_lowercase();
    [format!("<{tag}"), format!("</{tag}")]
        .into_iter()
        .any(|prefix| {
            lowercase.match_indices(&prefix).any(|(index, _)| {
                lowercase[index + prefix.len()..]
                    .chars()
                    .next()
                    .is_none_or(|character| {
                        character == '>' || character == '/' || character.is_ascii_whitespace()
                    })
            })
        })
}

fn validate_css_policy(css: &str) -> Result<()> {
    let lowercase = css.to_ascii_lowercase();
    if lowercase.contains("@import") {
        bail!("Generated CSS cannot use @import");
    }
    reject_remote_or_traversal(css, "CSS")
}

fn validate_javascript_policy(javascript: &str) -> Result<()> {
    let compact = javascript.to_ascii_lowercase();
    for forbidden in [
        "import ",
        "import(",
        "export ",
        "fetch(",
        "xmlhttprequest",
        "websocket(",
        "eventsource(",
        "navigator.sendbeacon",
        "document.write(",
    ] {
        if compact.contains(forbidden) {
            bail!("Generated JavaScript contains forbidden operation '{forbidden}'");
        }
    }
    reject_remote_or_traversal(javascript, "JavaScript")
}

fn reject_remote_or_traversal(value: &str, label: &str) -> Result<()> {
    let lowercase = value.to_ascii_lowercase();
    if lowercase.contains("http://")
        || lowercase.contains("https://")
        || lowercase.contains("src=\"//")
        || lowercase.contains("src='//")
        || lowercase.contains("url(//")
    {
        bail!("Generated {label} cannot reference remote URLs");
    }
    if value.contains("../") || value.contains("..\\") {
        bail!("Generated {label} cannot traverse outside the page artifact");
    }
    Ok(())
}

fn quoted_attribute_values<'a>(html: &'a str, name: &str) -> Vec<&'a str> {
    let mut values = Vec::new();
    let lowercase = html.to_ascii_lowercase();
    for quote in ['\"', '\''] {
        let marker = format!("{name}={quote}");
        let mut offset = 0;
        while let Some(relative) = lowercase[offset..].find(&marker) {
            let start = offset + relative + marker.len();
            if let Some(length) = html[start..].find(quote) {
                values.push(&html[start..start + length]);
                offset = start + length + 1;
            } else {
                break;
            }
        }
    }
    values
}

fn remove_theme_asset_references(
    mut shell: String,
    css_path: &Path,
    script_path: Option<&Path>,
) -> String {
    let css_name = css_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let script_name = script_path
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    shell = remove_tag_containing(shell, "link", css_name, false);
    if !script_name.is_empty() {
        shell = remove_tag_containing(shell, "script", script_name, true);
    }
    shell
}

fn remove_tag_containing(
    mut html: String,
    tag: &str,
    needle: &str,
    remove_closing_tag: bool,
) -> String {
    let opening = format!("<{tag}");
    loop {
        let lowercase = html.to_ascii_lowercase();
        let Some(start) = lowercase.find(&opening) else {
            break;
        };
        let Some(open_end_relative) = lowercase[start..].find('>') else {
            break;
        };
        let open_end = start + open_end_relative + 1;
        if !lowercase[start..open_end].contains(&needle.to_ascii_lowercase()) {
            let remainder = html.split_off(open_end);
            let prefix = html;
            let cleaned = remove_tag_containing(remainder, tag, needle, remove_closing_tag);
            return format!("{prefix}{cleaned}");
        }
        let end = if remove_closing_tag {
            let closing = format!("</{tag}>");
            lowercase[open_end..]
                .find(&closing)
                .map(|relative| open_end + relative + closing.len())
                .unwrap_or(open_end)
        } else {
            open_end
        };
        html.replace_range(start..end, "");
    }
    html
}

fn replace_title(mut shell: String, title: &str) -> String {
    if let (Some(start), Some(end)) = (shell.find("<title>"), shell.find("</title>"))
        && end >= start + "<title>".len()
    {
        shell.replace_range(start + "<title>".len()..end, &escape_text(title));
        return shell;
    }
    shell.replacen(
        "</head>",
        &format!("<title>{}</title></head>", escape_text(title)),
        1,
    )
}

fn escape_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_attribute(value: &str) -> String {
    escape_text(value).replace('"', "&quot;")
}

fn escape_script(value: &str) -> String {
    value
        .replace("</script", "<\\/script")
        .replace("</SCRIPT", "<\\/SCRIPT")
}

fn escape_style(value: &str) -> String {
    value
        .replace("</style", "<\\/style")
        .replace("</STYLE", "<\\/STYLE")
}

#[derive(Debug, Deserialize)]
struct BrowserReport {
    errors: Vec<String>,
    rejected_promises: Vec<String>,
    missing_images: Vec<String>,
    blank: bool,
    horizontal_overflow_px: u32,
    #[serde(default)]
    unrendered_math: Vec<String>,
}

async fn inspect_page(
    html_path: &Path,
    browser_path: Option<&Path>,
    operation: &OperationContext,
) -> Result<Vec<PageInspectionIssue>> {
    let browser = resolved_browser_path(browser_path)?
        .context("Could not find Chrome, Chromium, or Edge for page inspection")?;
    let url = format!("file://{}", html_path.canonicalize()?.display());
    let mut issues = Vec::new();
    for (viewport, width, height) in [("desktop", 1440, 900), ("mobile", 390, 844)] {
        let mut command = Command::new(&browser);
        command.args([
            "--headless",
            "--disable-gpu",
            "--allow-file-access-from-files",
            "--dump-dom",
            "--virtual-time-budget=5000",
            &format!("--window-size={width},{height}"),
            &url,
        ]);
        let output = run_command(&mut command, operation, OperationStage::InspectLayout)
            .await
            .context("Could not run browser page inspection")?;
        if !output.status.success() {
            bail!(
                "Browser page inspection exited with status {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let report = parse_browser_report(&String::from_utf8_lossy(&output.stdout))?;
        issues.extend(
            report
                .errors
                .into_iter()
                .map(|message| page_issue(viewport, PageIssueKind::RuntimeError, message, 0)),
        );
        issues.extend(
            report
                .rejected_promises
                .into_iter()
                .map(|message| page_issue(viewport, PageIssueKind::RejectedPromise, message, 0)),
        );
        issues.extend(
            report
                .missing_images
                .into_iter()
                .map(|message| page_issue(viewport, PageIssueKind::MissingImage, message, 0)),
        );
        issues.extend(
            report
                .unrendered_math
                .into_iter()
                .map(|message| page_issue(viewport, PageIssueKind::UnrenderedMath, message, 0)),
        );
        if report.blank {
            issues.push(page_issue(
                viewport,
                PageIssueKind::BlankContent,
                "Page has no visible text or media".to_string(),
                0,
            ));
        }
        if report.horizontal_overflow_px > 2 {
            issues.push(page_issue(
                viewport,
                PageIssueKind::HorizontalOverflow,
                format!(
                    "Page overflows horizontally by {}px",
                    report.horizontal_overflow_px
                ),
                report.horizontal_overflow_px,
            ));
        }
    }
    Ok(issues)
}

fn parse_browser_report(html: &str) -> Result<BrowserReport> {
    let marker = "data-sfumato-page-report=\"";
    let start = html
        .find(marker)
        .map(|index| index + marker.len())
        .context("Browser did not return a page inspection report")?;
    let end = html[start..]
        .find('"')
        .map(|index| start + index)
        .context("Browser returned an incomplete page inspection report")?;
    let decoded = percent_decode(&html[start..end])?;
    serde_json::from_str(&decoded).context("Could not parse page inspection report")
}

fn page_issue(
    viewport: &str,
    kind: PageIssueKind,
    message: String,
    overflow_px: u32,
) -> PageInspectionIssue {
    PageInspectionIssue {
        viewport: viewport.to_string(),
        kind,
        message,
        overflow_px,
    }
}

fn percent_decode(value: &str) -> Result<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = bytes
                .get(index + 1..index + 3)
                .context("Invalid page report encoding")?;
            let hex = std::str::from_utf8(hex).context("Invalid page report encoding")?;
            decoded.push(u8::from_str_radix(hex, 16).context("Invalid page report encoding")?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).context("Page report was not UTF-8")
}

fn page_inspection_error(error: anyhow::Error) -> SfumatoError {
    if let Some(error) = error.downcast_ref::<SfumatoError>() {
        return error.clone();
    }
    let message = format!("{error:#}");
    let class = if message.contains("Could not find Chrome") {
        ErrorClass::Unavailable
    } else {
        ErrorClass::Permanent
    };
    SfumatoError::render(class, message).at_stage(OperationStage::InspectLayout)
}

const INSPECTION_BOOTSTRAP: &str = r#"<script data-sfumato-inspection>
window.__sfumatoErrors = [];
window.__sfumatoRejectedPromises = [];
window.addEventListener('error', event => {
  window.__sfumatoErrors.push(event.message || String(event.error || 'Unknown runtime error'));
});
window.addEventListener('unhandledrejection', event => {
  window.__sfumatoRejectedPromises.push(String(event.reason || 'Unhandled promise rejection'));
});
</script>"#;

const INSPECTION_REPORTER: &str = r#"<script data-sfumato-inspection>
(() => {
  const rawMathSnippets = root => {
    if (!root) return [];
    const candidates = [...root.querySelectorAll('*')].filter(element => {
      if (element.closest('pre, code, mjx-container')) return false;
      return element.children.length === 0;
    });
    return candidates
      .map(element => (element.textContent || '').trim())
      .filter(text => /\\\(|\\\[|\$\$/.test(text))
      .slice(0, 5)
      .map(text => `Unrendered TeX: ${text.slice(0, 160)}`);
  };
  let reported = false;
  const report = () => {
    if (reported) return;
    reported = true;
    const root = document.getElementById('sfumato-page');
    const images = [...document.images];
    const missing = images.filter(image => image.complete && image.naturalWidth === 0)
      .map(image => image.getAttribute('src') || 'unnamed image');
    const visibleMedia = root && root.querySelector('img, svg, canvas, video, audio');
    const text = root ? (root.innerText || '').trim() : '';
    const overflow = Math.max(0, Math.ceil(document.documentElement.scrollWidth - document.documentElement.clientWidth));
    const payload = {
      errors: window.__sfumatoErrors || [],
      rejected_promises: window.__sfumatoRejectedPromises || [],
      missing_images: missing,
      blank: !root || (!text && !visibleMedia),
      horizontal_overflow_px: overflow,
      unrendered_math: rawMathSnippets(root)
    };
    document.documentElement.dataset.sfumatoPageReport = encodeURIComponent(JSON.stringify(payload));
  };
  const mathPromise = window.MathJax?.startup?.promise;
  if (mathPromise) {
    mathPromise.then(report).catch(error => {
      window.__sfumatoErrors.push(`Math rendering failed: ${error}`);
      report();
    });
    setTimeout(report, 4000);
  } else {
    report();
  }
})();
</script>
"#;

#[cfg(test)]
#[path = "../tests/unit/pages.rs"]
mod tests;
