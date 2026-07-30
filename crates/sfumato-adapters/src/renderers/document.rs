//! Paged.js CLI adapter for printable documents.
//!
//! Pagination has to happen in the same browser session that prints, because the
//! page numbers, the running header and the contents page references are all
//! resolved from the paginator's own counters. Driving a browser directly cannot
//! guarantee that ordering: it prints when it considers the page ready, without
//! knowing the paginator is still working, which yields a different page count on
//! every run. The Paged.js CLI owns that ordering, the same way the Marp CLI owns
//! it for decks.

use std::{path::Path, time::Duration};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use sfumato_core::{
    errors::{ErrorClass, OperationStage, SfumatoError, SfumatoResult},
    generation::DocumentFormatIssue,
    operation::OperationContext,
    renderers::{DocumentRenderRequest, DocumentRenderer, RenderedDocument},
};
use tokio::process::Command;

use crate::{renderers::resolved_browser_path, runtime::run_command};

/// How long the renderer may spend on one document.
const RENDER_TIMEOUT_MS: u32 = 120_000;

/// Hard wall-clock bound on the measuring browser pass.
const MEASURE_DEADLINE: Duration = Duration::from_secs(60);

/// Heading levels that become PDF outline entries.
///
/// The level-1 heading lives on the cover, so the outline is rooted at the
/// document's own sections.
const OUTLINE_TAGS: &str = "h2,h3,h4";

/// Renders printable documents with the Paged.js CLI.
#[derive(Clone, Copy, Debug, Default)]
pub struct PagedDocumentCliRenderer;

/// Resolves the CLI, preferring the version Sfumato pins.
///
/// A managed install is preferred over whatever the shell happens to expose:
/// pagination output depends on the paginator's version, so a pinned copy keeps
/// one machine's documents comparable with another's. A global install still
/// works, which keeps the command usable before `renderer install` is run.
fn executable() -> std::ffi::OsString {
    let managed = dirs::home_dir()
        .map(|home| home.join(".sfumato/renderers/pagedjs/node_modules/.bin/pagedjs-cli"))
        .filter(|path| path.is_file());
    match managed {
        Some(path) => path.into_os_string(),
        None => "pagedjs-cli".into(),
    }
}

#[derive(Debug, thiserror::Error)]
enum PagedError {
    #[error(
        "The Paged.js CLI is not installed. Run `sfumato renderer install pagedjs`, or install it globally with `npm install -g pagedjs-cli`."
    )]
    Missing,
}

#[async_trait]
impl DocumentRenderer for PagedDocumentCliRenderer {
    async fn render_pdf(
        &self,
        request: DocumentRenderRequest<'_>,
        operation: &OperationContext,
    ) -> SfumatoResult<RenderedDocument> {
        let result = paginate(&request, operation, OperationStage::Render, false).await;
        render_result(
            result.map(|pages| RenderedDocument { pages }),
            OperationStage::Render,
        )
    }

    async fn inspect_format(
        &self,
        request: DocumentRenderRequest<'_>,
        operation: &OperationContext,
    ) -> SfumatoResult<Vec<DocumentFormatIssue>> {
        render_result(
            inspect(&request, operation).await,
            OperationStage::InspectLayout,
        )
    }
}

/// Runs the CLI once, returning the page count it reported.
///
/// `html` asks for the paginated markup instead of a PDF. Both come from the same
/// deterministic pagination, so a measurement taken from one describes the other.
async fn paginate(
    request: &DocumentRenderRequest<'_>,
    operation: &OperationContext,
    stage: OperationStage,
    html: bool,
) -> Result<usize> {
    let output_path = request.workspace_root.join(request.output);
    // A stale artifact from an earlier attempt would read as a fresh success.
    let _ = std::fs::remove_file(&output_path);

    let mut command = Command::new(executable());
    command
        .arg(request.document)
        .arg("--output")
        .arg(request.output)
        .arg("--timeout")
        .arg(RENDER_TIMEOUT_MS.to_string())
        // The document is assembled offline on purpose; a renderer that reached
        // the network would silently reintroduce the dependency core validation
        // exists to prevent.
        .arg("--blockRemote")
        .arg("--allowedPath")
        .arg(request.workspace_root)
        // Resolve every relative asset path the way the document expects.
        .current_dir(request.workspace_root);
    if html {
        command.arg("--html");
    } else {
        command.arg("--outline-tags").arg(OUTLINE_TAGS);
    }
    if let Some(browser) = resolved_browser_path(None)? {
        // Reuse the browser Sfumato already requires rather than a second one.
        command.env("PUPPETEER_EXECUTABLE_PATH", browser);
    }

    let output = run_command(&mut command, operation, stage).await;
    let output = match output {
        Ok(output) => output,
        Err(error) if is_not_found(&error) => return Err(PagedError::Missing.into()),
        Err(error) => return Err(error).context("Could not run the Paged.js CLI"),
    };
    if !output.status.success() {
        bail!(
            "The Paged.js CLI exited with status {}{}{}",
            output.status,
            format_stream("stdout", String::from_utf8_lossy(&output.stdout).trim()),
            format_stream("stderr", String::from_utf8_lossy(&output.stderr).trim())
        );
    }
    if !output_path.is_file() {
        bail!(
            "The Paged.js CLI reported success but wrote nothing to {}",
            output_path.display()
        );
    }
    let reported = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let pages = reported_pages(&reported)
        .context("The Paged.js CLI did not report how many pages it rendered")?;
    if pages == 0 {
        bail!("The Paged.js CLI rendered no pages; the document has no printable content");
    }
    Ok(pages)
}

/// Paginates to HTML and measures the defects that survive pagination.
async fn inspect(
    request: &DocumentRenderRequest<'_>,
    operation: &OperationContext,
) -> Result<Vec<DocumentFormatIssue>> {
    let paginated = request.output.with_extension("paginated.html");
    let paginated_request = DocumentRenderRequest {
        workspace_root: request.workspace_root,
        document: request.document,
        output: &paginated,
        setup: request.setup,
    };
    paginate(
        &paginated_request,
        operation,
        OperationStage::InspectLayout,
        true,
    )
    .await?;

    let paginated_path = request.workspace_root.join(&paginated);
    let instrumented = paginated_path.with_extension("measure.html");
    let markup = std::fs::read_to_string(&paginated_path)
        .with_context(|| format!("Could not read {}", paginated_path.display()))?;
    // The CLI leaves its paginator in the output. Measuring with it still in
    // place would paginate markup that is already paginated.
    let static_markup = strip_scripts(&markup);
    std::fs::write(
        &instrumented,
        static_markup.replacen("</body>", &format!("{FORMAT_INSPECTOR}</body>"), 1),
    )
    .with_context(|| format!("Could not prepare {}", instrumented.display()))?;
    let measured = measure_in_browser(&instrumented, operation).await;
    // Both files exist only to carry the measurement; leaving them behind would
    // ship extra, script-bearing HTML inside the revision.
    let _ = std::fs::remove_file(&instrumented);
    let _ = std::fs::remove_file(&paginated_path);
    measured
}

/// Removes every script element from already-paginated markup.
pub(crate) fn strip_scripts(markup: &str) -> String {
    let mut output = String::with_capacity(markup.len());
    let mut rest = markup;
    while let Some(start) = rest.find("<script") {
        let tail = &rest[start..];
        let Some(offset) = tail.find("</script>") else {
            break;
        };
        output.push_str(&rest[..start]);
        rest = &tail[offset + "</script>".len()..];
    }
    output.push_str(rest);
    output
}

/// Measures already-paginated markup in a headless browser.
///
/// Reads the report off the browser's output stream and stops there rather than
/// waiting for the process to exit: headless Chrome with a virtual-time budget
/// does not reliably exit on a document this size, and the report attribute rides
/// on `<html>`, which is the first thing it serializes.
async fn measure_in_browser(
    html_path: &Path,
    operation: &OperationContext,
) -> Result<Vec<DocumentFormatIssue>> {
    use tokio::io::AsyncReadExt;

    operation.checkpoint(OperationStage::InspectLayout)?;
    let browser = resolved_browser_path(None)?
        .context("Could not find Chrome, Chromium, or Edge to measure the document")?;
    let profile = tempfile::tempdir().context("Could not create a browser profile directory")?;
    let url = format!(
        "file://{}",
        html_path
            .canonicalize()
            .with_context(|| format!("Could not resolve {}", html_path.display()))?
            .display()
    );
    let mut child = Command::new(browser)
        .arg("--headless")
        .arg("--disable-gpu")
        .arg("--allow-file-access-from-files")
        // Chrome shares one profile directory by default, so two concurrent
        // renders corrupt each other's output.
        .arg(format!("--user-data-dir={}", profile.path().display()))
        .arg("--dump-dom")
        .arg("--virtual-time-budget=15000")
        .arg(url)
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("Could not run the browser to measure the document")?;
    let mut stdout = child
        .stdout
        .take()
        .context("The browser produced no output stream")?;

    let mut dom = String::new();
    let mut buffer = [0_u8; 16_384];
    let collected = async {
        loop {
            let read = stdout.read(&mut buffer).await?;
            if read == 0 {
                break;
            }
            dom.push_str(&String::from_utf8_lossy(&buffer[..read]));
            if report_is_complete(&dom) {
                break;
            }
        }
        Ok::<(), anyhow::Error>(())
    };
    let outcome = tokio::time::timeout(MEASURE_DEADLINE, collected).await;
    let _ = child.start_kill();
    let _ = child.wait().await;
    drop(profile);
    match outcome {
        Ok(result) => result?,
        Err(_) => bail!(
            "The browser exceeded its {}s deadline while measuring the document",
            MEASURE_DEADLINE.as_secs()
        ),
    }
    parse_format_report(&dom)
}

/// Whether the serialized DOM already carries a complete report attribute.
pub(crate) fn report_is_complete(dom: &str) -> bool {
    let marker = "data-sfumato-format=\"";
    dom.find(marker)
        .is_some_and(|start| dom[start + marker.len()..].contains('"'))
}

/// Reads the page count the CLI printed.
pub(crate) fn reported_pages(output: &str) -> Option<usize> {
    let marker = "Rendering ";
    let start = output.find(marker)? + marker.len();
    let rest = &output[start..];
    let end = rest.find(' ')?;
    rest[..end].parse().ok()
}

fn is_not_found(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<std::io::Error>()
        .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound)
}

pub(crate) fn parse_format_report(dom: &str) -> Result<Vec<DocumentFormatIssue>> {
    let marker = "data-sfumato-format=\"";
    let start = dom
        .find(marker)
        .map(|index| index + marker.len())
        .context("The browser did not return a document format report")?;
    let end = dom[start..]
        .find('"')
        .map(|index| start + index)
        .context("The browser returned an incomplete document format report")?;
    let payload = percent_decode(&dom[start..end])?;
    let report: FormatReport =
        serde_json::from_str(&payload).context("Could not parse the document format report")?;
    if report.pages == 0 {
        bail!("The paginated document carries no page boxes to measure");
    }
    Ok(report.issues)
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct FormatReport {
    pub(crate) pages: usize,
    pub(crate) issues: Vec<DocumentFormatIssue>,
}

fn percent_decode(value: &str) -> Result<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = bytes
                .get(index + 1..index + 3)
                .context("Invalid percent-encoded format report")?;
            let hex = std::str::from_utf8(hex).context("Invalid format report encoding")?;
            decoded.push(u8::from_str_radix(hex, 16).context("Invalid format report encoding")?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).context("Format report was not UTF-8")
}

fn format_stream(label: &str, value: &str) -> String {
    if value.is_empty() {
        String::new()
    } else {
        format!("\n\n{label}:\n{value}")
    }
}

fn render_result<T>(result: Result<T>, stage: OperationStage) -> SfumatoResult<T> {
    result.map_err(|error| {
        if let Some(error) = error.downcast_ref::<SfumatoError>() {
            let mut error = error.clone();
            if error.stage.is_none() {
                error.stage = Some(stage);
            }
            return error;
        }
        let message = format!("{error:#}");
        let class = if error.downcast_ref::<PagedError>().is_some()
            || message.contains("Could not find Chrome")
        {
            ErrorClass::Unavailable
        } else {
            ErrorClass::Permanent
        };
        SfumatoError::render(class, message).at_stage(stage)
    })
}

/// Measures the defects that survive pagination, page by page.
///
/// Prose reflows, so there is no single overflow number the way there is for a
/// fixed slide. What matters is content that cannot fit the column no matter
/// where it breaks, and furniture that broke in an ugly place. The markup is
/// already paginated when this runs, so it measures once and reports.
const FORMAT_INSPECTOR: &str = r#"<script>
(() => {
  const NEARLY_EMPTY_RATIO = 0.2;
  const TOLERANCE_PX = 2;
  const report = (payload) =>
    document.documentElement.setAttribute('data-sfumato-format', encodeURIComponent(JSON.stringify(payload)));

  const measure = () => {
    const pages = [...document.querySelectorAll('.pagedjs_page')];
    const issues = [];
    const headings = [...document.querySelectorAll('.sfumato-document h2, .sfumato-document h3, .sfumato-document h4, .sfumato-document h5, .sfumato-document h6')];
    // Precomputed once: resolving an element's section by comparing it against
    // every heading turns measurement into O(elements x headings), which on a
    // paginated document is slow enough to look like a hang.
    const owner = new Map();
    headings.forEach((heading, index) => {
      const info = { section: index + 1, heading: heading.textContent.trim() };
      owner.set(heading, info);
      let node = heading.nextElementSibling;
      while (node && !owner.has(node)) {
        owner.set(node, info);
        node = node.nextElementSibling;
      }
    });
    const sectionFor = (element) => {
      let node = element;
      while (node) {
        const info = owner.get(node);
        if (info) return info;
        node = node.previousElementSibling || node.parentElement;
      }
      return { section: 0, heading: '' };
    };
    const describe = (element) => {
      const name = element.tagName.toLowerCase();
      const classes = typeof element.className === 'string' ? element.className.trim().split(/\s+/) : [];
      return classes[0] ? name + '.' + classes[0] : name;
    };
    const push = (element, page, kind, overflow) => {
      const { section, heading } = sectionFor(element);
      issues.push({
        page,
        section,
        heading,
        kind,
        overflow_px: Math.max(0, Math.ceil(overflow)),
        element: describe(element)
      });
    };

    pages.forEach((page, index) => {
      const number = index + 1;
      const area = page.querySelector('.pagedjs_page_content') || page;
      const bounds = area.getBoundingClientRect();
      if (bounds.width <= 0 || bounds.height <= 0) return;
      let filled = 0;
      for (const element of area.querySelectorAll('p, li, pre, table, img, svg, h2, h3, h4, h5, h6, blockquote')) {
        const rect = element.getBoundingClientRect();
        if (rect.height <= 0) continue;
        filled = Math.max(filled, rect.bottom - bounds.top);
        const horizontal = rect.right - bounds.right;
        if (horizontal > TOLERANCE_PX) {
          push(element, number, 'overflows_text_column', horizontal);
        }
        if (rect.height - bounds.height > TOLERANCE_PX) {
          push(element, number, 'taller_than_page', rect.height - bounds.height);
        }
        if (/^H[2-6]$/.test(element.tagName)) {
          const remaining = bounds.bottom - rect.bottom;
          const line = parseFloat(getComputedStyle(element).lineHeight) || rect.height;
          if (remaining >= 0 && remaining < line * 1.5) {
            push(element, number, 'orphaned_heading', line * 1.5 - remaining);
          }
        }
      }
      // The last page ends where the content ends, so a short final page is
      // expected rather than a defect.
      if (number < pages.length && filled > 0 && filled < bounds.height * NEARLY_EMPTY_RATIO) {
        push(area, number, 'nearly_empty_page', bounds.height - filled);
      }
    });
    return { pages: pages.length, issues };
  };

  report(measure());
})();
</script>"#;

#[cfg(test)]
#[path = "../../tests/unit/renderers_document.rs"]
mod tests;
