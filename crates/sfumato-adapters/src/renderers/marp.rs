use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use sfumato_core::{
    errors::OperationStage, generation::SlideLayoutIssue, operation::OperationContext,
    renderers::SlideRenderer,
};
use tokio::process::Command;

use crate::runtime::run_command;

/// Marp CLI and headless-browser renderer adapter.
#[derive(Clone, Copy, Debug, Default)]
pub struct MarpCliRenderer;

#[derive(Debug, thiserror::Error)]
enum MarpError {
    #[error("Marp CLI is not installed. Install @marp-team/marp-cli to export PDFs.")]
    Missing,
}

#[async_trait]
impl SlideRenderer for MarpCliRenderer {
    async fn render_pdf(
        &self,
        markdown_path: &Path,
        theme_css_path: &Path,
        pdf_path: &Path,
        browser_path: Option<&Path>,
        operation: &OperationContext,
    ) -> Result<()> {
        let args = command_args(markdown_path, theme_css_path, pdf_path, browser_path)?;
        let mut command = Command::new("marp");
        command.args(args);
        let output = run_command(&mut command, operation, OperationStage::Render).await;
        let output = match output {
            Ok(output) => output,
            Err(error) if is_not_found(&error) => {
                return Err(MarpError::Missing.into());
            }
            Err(error) => return Err(error).context("Could not run Marp CLI"),
        };
        if !output.status.success() {
            bail!(
                "Marp CLI exited with status {}{}{}",
                output.status,
                format_stream("stdout", String::from_utf8_lossy(&output.stdout).trim()),
                format_stream("stderr", String::from_utf8_lossy(&output.stderr).trim())
            );
        }
        Ok(())
    }

    async fn inspect_layout(
        &self,
        markdown_path: &Path,
        theme_css_path: &Path,
        html_path: &Path,
        browser_path: Option<&Path>,
        operation: &OperationContext,
    ) -> Result<Vec<SlideLayoutIssue>> {
        render_html(markdown_path, theme_css_path, html_path, operation).await?;
        inject_layout_inspector(html_path)?;
        let browser = resolved_browser_path(browser_path)?
            .context("Could not find Chrome, Chromium, or Edge for Marp layout inspection")?;
        let url = format!("file://{}", html_path.canonicalize()?.display());
        let mut command = Command::new(browser);
        command.args([
            "--headless",
            "--disable-gpu",
            "--allow-file-access-from-files",
            "--dump-dom",
            "--virtual-time-budget=3000",
            &url,
        ]);
        let output = run_command(&mut command, operation, OperationStage::InspectLayout)
            .await
            .context("Could not run the browser for Marp layout inspection")?;
        if !output.status.success() {
            bail!(
                "Browser layout inspection exited with status {}{}",
                output.status,
                format_stream("stderr", String::from_utf8_lossy(&output.stderr).trim())
            );
        }
        parse_layout_report(&String::from_utf8_lossy(&output.stdout))
    }
}

async fn render_html(
    markdown_path: &Path,
    theme_css_path: &Path,
    html_path: &Path,
    operation: &OperationContext,
) -> Result<()> {
    let mut command = Command::new("marp");
    command.args([
        "--template".as_ref(),
        "bare".as_ref(),
        "--allow-local-files".as_ref(),
        "--theme".as_ref(),
        theme_css_path.as_os_str(),
        markdown_path.as_os_str(),
        "-o".as_ref(),
        html_path.as_os_str(),
    ]);
    let output = run_command(&mut command, operation, OperationStage::InspectLayout).await;
    let output = match output {
        Ok(output) => output,
        Err(error) if is_not_found(&error) => {
            return Err(MarpError::Missing.into());
        }
        Err(error) => return Err(error).context("Could not run Marp CLI"),
    };
    if !output.status.success() {
        bail!(
            "Marp HTML preview exited with status {}{}",
            output.status,
            format_stream("stderr", String::from_utf8_lossy(&output.stderr).trim())
        );
    }
    Ok(())
}

fn is_not_found(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<std::io::Error>()
        .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound)
}

fn inject_layout_inspector(html_path: &Path) -> Result<()> {
    let html = std::fs::read_to_string(html_path)
        .with_context(|| format!("Could not read {}", html_path.display()))?;
    let script = r#"<script>
(() => {
  const inspect = () => {
  const issues = [...document.querySelectorAll('svg[data-marpit-svg] foreignObject > section')]
    .map((section, index) => {
      const bounds = section.getBoundingClientRect();
      let right = Math.max(0, section.scrollWidth - section.clientWidth);
      let bottom = Math.max(0, section.scrollHeight - section.clientHeight);
      for (const element of section.querySelectorAll('*')) {
        if (element.matches('script, style') || element.closest('header, footer')) continue;
        const style = getComputedStyle(element);
        if (style.display === 'none' || style.visibility === 'hidden') continue;
        const rect = element.getBoundingClientRect();
        right = Math.max(right, rect.right - bounds.right);
        bottom = Math.max(bottom, rect.bottom - bounds.bottom);
      }
      const horizontal = Math.max(0, Math.ceil(right));
      const vertical = Math.max(0, Math.ceil(bottom));
      const heading = section.querySelector('h1, h2');
      return {
        slide: index + 1,
        title: heading ? heading.textContent.trim() : `Slide ${index + 1}`,
        vertical_overflow_px: vertical,
        horizontal_overflow_px: horizontal
      };
    })
    .filter((issue) => issue.vertical_overflow_px > 2 || issue.horizontal_overflow_px > 2);
  document.documentElement.dataset.sfumatoLayout = encodeURIComponent(JSON.stringify(issues));
  };
  inspect();
  window.addEventListener('load', () => requestAnimationFrame(() => requestAnimationFrame(inspect)), { once: true });
  setTimeout(inspect, 1000);
})();
</script>"#;
    let rendered = html.replacen("</body>", &format!("{script}</body>"), 1);
    std::fs::write(html_path, rendered)
        .with_context(|| format!("Could not prepare {}", html_path.display()))
}

fn parse_layout_report(html: &str) -> Result<Vec<SlideLayoutIssue>> {
    let marker = "data-sfumato-layout=\"";
    let start = html
        .find(marker)
        .map(|index| index + marker.len())
        .context("Browser did not return a Marp layout report")?;
    let end = html[start..]
        .find('"')
        .map(|index| start + index)
        .context("Browser returned an incomplete Marp layout report")?;
    serde_json::from_str(&percent_decode(&html[start..end])?)
        .context("Could not parse Marp layout report")
}

fn percent_decode(value: &str) -> Result<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = bytes
                .get(index + 1..index + 3)
                .context("Invalid percent-encoded layout report")?;
            let hex = std::str::from_utf8(hex).context("Invalid layout report encoding")?;
            decoded.push(u8::from_str_radix(hex, 16).context("Invalid layout report encoding")?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).context("Layout report was not UTF-8")
}

fn command_args(
    markdown_path: &Path,
    theme_css_path: &Path,
    pdf_path: &Path,
    configured_browser_path: Option<&Path>,
) -> Result<Vec<OsString>> {
    let mut args = vec![
        "--allow-local-files".into(),
        "--theme".into(),
        theme_css_path.as_os_str().to_owned(),
        markdown_path.as_os_str().to_owned(),
        "-o".into(),
        pdf_path.as_os_str().to_owned(),
    ];
    if let Some(browser_path) = resolved_browser_path(configured_browser_path)? {
        args.splice(
            0..0,
            [
                "--browser".into(),
                "chrome".into(),
                "--browser-path".into(),
                browser_path.as_os_str().to_owned(),
            ],
        );
    }
    Ok(args)
}

fn resolved_browser_path(configured: Option<&Path>) -> Result<Option<PathBuf>> {
    if let Some(path) = configured {
        if !path.is_file() {
            bail!(
                "Configured Marp browser path does not exist or is not a file: {}",
                path.display()
            );
        }
        return Ok(Some(path.to_path_buf()));
    }
    Ok(detected_browser_path())
}

fn detected_browser_path() -> Option<PathBuf> {
    [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|path| path.is_file())
}

fn format_stream(label: &str, value: &str) -> String {
    if value.is_empty() {
        String::new()
    } else {
        format!("\n\n{label}:\n{value}")
    }
}

#[cfg(test)]
#[path = "../../tests/unit/renderers_marp.rs"]
mod tests;
