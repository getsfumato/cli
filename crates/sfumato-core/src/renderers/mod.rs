//! Renderer ports used by resource workflows.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use async_trait::async_trait;
use serde::Serialize;

use crate::{
    errors::{OperationStage, SfumatoResult},
    generation::{PageInspectionIssue, PageRuntimeSelection, SlideLayoutIssue},
    operation::OperationContext,
    page_plugins::PagePluginPackage,
    themes::ThemePackage,
};
use sfumato_domain::PageDocument;
pub use sfumato_domain::VideoEngine;

/// Semantic Mermaid styling derived from a Sfumato theme.
#[derive(Clone, Debug, Serialize)]
pub struct MermaidThemeConfig {
    theme: &'static str,
    #[serde(rename = "themeVariables")]
    theme_variables: BTreeMap<String, String>,
}

impl MermaidThemeConfig {
    /// Creates a custom Mermaid base-theme configuration.
    pub fn new(theme_variables: BTreeMap<String, String>) -> Self {
        Self {
            theme: "base",
            theme_variables,
        }
    }
}

/// Port for rendering Mermaid source into SVG artifacts.
#[async_trait]
pub trait DiagramRenderer: Send + Sync {
    /// Renders one Mermaid input file into an SVG output file.
    async fn render_svg(
        &self,
        input_path: &Path,
        output_path: &Path,
        theme: &MermaidThemeConfig,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<String>;
}

/// Port for Marp PDF rendering and browser-backed layout inspection.
#[async_trait]
pub trait SlideRenderer: Send + Sync {
    /// Renders a themed Marp Markdown file into PDF.
    async fn render_pdf(
        &self,
        markdown_path: &Path,
        theme_css_path: &Path,
        pdf_path: &Path,
        browser_path: Option<&Path>,
        operation: &OperationContext,
    ) -> SfumatoResult<()>;

    /// Measures horizontal and vertical overflow in a themed Marp deck.
    async fn inspect_layout(
        &self,
        markdown_path: &Path,
        theme_css_path: &Path,
        html_path: &Path,
        browser_path: Option<&Path>,
        operation: &OperationContext,
    ) -> SfumatoResult<Vec<SlideLayoutIssue>>;
}

/// Complete inputs for assembling a validated standalone page.
pub struct PageAssemblyRequest<'a> {
    /// Structured page fragments supplied by the model workflow.
    pub document: &'a PageDocument,
    /// Selected visual theme package.
    pub theme: &'a ThemePackage,
    /// Offline plugin runtimes selected for this page.
    pub plugins: &'a [PagePluginPackage],
    /// Generated assets that page markup may reference.
    pub allowed_assets: &'a [std::path::PathBuf],
    /// Whether browser diagnostics should be embedded in the output.
    pub inspection: bool,
}

/// Validated standalone page plus the built-in runtimes embedded into it.
#[derive(Debug)]
pub struct AssembledPage {
    /// Complete standalone HTML document.
    pub html: String,
    /// Automatically selected renderer runtimes and integrity metadata.
    pub runtimes: Vec<PageRuntimeSelection>,
}

/// Deterministic HTML compiler and static validator for page fragments.
pub trait PageAssembler: Send + Sync {
    /// Validates fragments and assembles one standalone HTML document.
    fn assemble(&self, request: PageAssemblyRequest<'_>) -> SfumatoResult<AssembledPage>;
}

/// Browser-backed runtime and responsive-layout inspection for generated pages.
#[async_trait]
pub trait PageInspector: Send + Sync {
    /// Runs responsive browser inspection against one assembled page.
    async fn inspect(
        &self,
        html_path: &Path,
        browser_path: Option<&Path>,
        operation: &OperationContext,
    ) -> SfumatoResult<Vec<PageInspectionIssue>>;
}

/// Engine-specific local video rendering parameters.
#[derive(Clone, Debug)]
pub struct VideoRenderRequest {
    /// Hyperframe or Manim source bundle root.
    pub source_root: PathBuf,
    /// Final MP4 destination in transaction staging.
    pub output_path: PathBuf,
    /// Requested duration in seconds.
    pub duration_seconds: u32,
    /// Output width in pixels.
    pub width: u32,
    /// Output height in pixels.
    pub height: u32,
    /// Output frame rate.
    pub fps: u32,
    /// Renderer quality name.
    pub quality: String,
}

/// Metadata measured from a rendered MP4.
#[derive(Clone, Debug, Serialize)]
pub struct VideoInspection {
    /// Measured duration in seconds.
    pub duration_seconds: f64,
    /// Encoded width.
    pub width: u32,
    /// Encoded height.
    pub height: u32,
    /// Whether at least one audio stream exists.
    pub has_audio: bool,
    /// Primary video codec.
    pub video_codec: String,
}

/// Local Hyperframe and Manim process adapter.
#[async_trait]
pub trait VideoRenderer: Send + Sync {
    /// Validates and renders one local generated-code project.
    async fn render(
        &self,
        engine: VideoEngine,
        request: &VideoRenderRequest,
        operation: &OperationContext,
    ) -> SfumatoResult<()>;

    /// Validates that a final file is a playable video with expected shape.
    async fn inspect(
        &self,
        video_path: &Path,
        operation: &OperationContext,
    ) -> SfumatoResult<VideoInspection>;
}

/// Installation and health state for one generated-code renderer.
#[derive(Clone, Debug, Serialize)]
pub struct RendererStatus {
    /// Stable renderer ID.
    pub id: String,
    /// Pinned release managed by Sfumato.
    pub version: String,
    /// Whether the managed executable is installed.
    pub installed: bool,
    /// Whether all external dependencies are currently available.
    pub healthy: bool,
    /// Human-readable diagnostics without terminal formatting.
    pub details: Vec<String>,
}

/// Explicit lifecycle for optional local renderers.
#[async_trait]
pub trait RendererManager: Send + Sync {
    /// Lists supported renderer packages and health state.
    async fn list(&self, operation: &OperationContext) -> SfumatoResult<Vec<RendererStatus>>;
    /// Installs one pinned renderer into Sfumato's managed directory.
    async fn install(
        &self,
        id: &str,
        operation: &OperationContext,
    ) -> SfumatoResult<RendererStatus>;
    /// Removes one managed renderer package.
    fn remove(&self, id: &str) -> SfumatoResult<RendererStatus>;
    /// Runs dependency and executable checks.
    async fn doctor(
        &self,
        id: Option<&str>,
        operation: &OperationContext,
    ) -> SfumatoResult<Vec<RendererStatus>>;
}
