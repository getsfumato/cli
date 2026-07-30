//! Renderer ports used by resource workflows.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    errors::{OperationStage, SfumatoResult},
    generation::{PageInspectionIssue, PageRuntimeSelection, SlideLayoutIssue},
    operation::OperationContext,
    page_plugins::PagePluginPackage,
    themes::ThemePackage,
};
use sfumato_domain::PageDocument;
pub use sfumato_domain::{SectionedDocument, VideoEngine};

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

/// What a catalog item contributes to a scene.
///
/// The planner selects a role, and the catalog resolves the role to the items
/// that serve it. Naming an item directly would let the model reach for
/// whatever is installed rather than what the beat needs.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum VideoCatalogRole {
    /// Steps, branches, or a decision structure.
    Process,
    /// Magnitudes and comparisons.
    Quantity,
    /// Source code shown, typed, diffed, or scrolled.
    Code,
    /// Typographic emphasis over a caption or word list.
    Emphasis,
    /// A boundary between two scenes.
    Transition,
    /// A whole-film treatment applied once, never chosen per scene.
    Grade,
}

impl VideoCatalogRole {
    /// Stable spelling used by plans, prompts, and the catalog manifest.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Process => "process",
            Self::Quantity => "quantity",
            Self::Code => "code",
            Self::Emphasis => "emphasis",
            Self::Transition => "transition",
            Self::Grade => "grade",
        }
    }
}

/// How a catalog item is wired into a composition.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VideoCatalogKind {
    /// A standalone sub-composition included by reference.
    Block,
    /// A snippet merged into the host composition.
    Component,
}

/// One installable Hyperframe registry item Sfumato is willing to use.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct VideoCatalogItem {
    /// Registry ID passed to the renderer's `add` command.
    pub id: String,
    /// Whether the item is referenced or merged.
    pub kind: VideoCatalogKind,
    /// The beat function this item serves.
    pub role: VideoCatalogRole,
    /// Authored runtime of a block, absent for components that carry no timeline.
    ///
    /// A scene shorter than its block truncates the block's own reveal, so this
    /// is a hard constraint on the plan rather than a hint.
    #[serde(default)]
    pub duration_seconds: Option<f32>,
    /// One-line description surfaced to the planner.
    pub summary: String,
}

/// The curated subset of the renderer's registry, with its authored metadata.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct VideoCatalog {
    schema_version: u32,
    catalog_version: String,
    items: Vec<VideoCatalogItem>,
}

/// One way a plan disagrees with the catalog it selected from.
#[derive(Clone, Debug, PartialEq)]
pub enum VideoCatalogViolation {
    /// The plan named an item the curated catalog does not contain.
    UnknownItem {
        /// Scene that made the selection.
        scene: String,
        /// Selected item ID.
        id: String,
    },
    /// The scene is shorter than the block's authored runtime.
    SceneTooShort {
        /// Scene that made the selection.
        scene: String,
        /// Selected item ID.
        id: String,
        /// Scene runtime in seconds.
        scene_seconds: f32,
        /// The item's authored runtime in seconds.
        item_seconds: f32,
    },
    /// A whole-film treatment was selected for a single scene.
    GradePerScene {
        /// Scene that made the selection.
        scene: String,
        /// Selected item ID.
        id: String,
    },
}

impl VideoCatalogViolation {
    /// Whether the selection cannot render at all, rather than rendering poorly.
    ///
    /// An unknown ID or a whole-film treatment names a composition that was
    /// never installed, so keeping it guarantees a broken build. A scene shorter
    /// than its block still renders: the reveal is simply cut off. Callers use
    /// this to decide between dropping a selection and merely warning about it,
    /// and both the drafted plan and a reviewer patch must judge it the same way
    /// or a tolerated draft violation would silently reject every review.
    pub const fn is_unusable(&self) -> bool {
        match self {
            Self::UnknownItem { .. } | Self::GradePerScene { .. } => true,
            Self::SceneTooShort { .. } => false,
        }
    }
}

impl std::fmt::Display for VideoCatalogViolation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownItem { scene, id } => write!(
                formatter,
                "scene '{scene}' selected '{id}', which is not in the managed catalog"
            ),
            Self::SceneTooShort {
                scene,
                id,
                scene_seconds,
                item_seconds,
            } => write!(
                formatter,
                "scene '{scene}' lasts {scene_seconds}s but '{id}' is authored for {item_seconds}s, so its reveal would be cut off"
            ),
            Self::GradePerScene { scene, id } => write!(
                formatter,
                "scene '{scene}' selected '{id}', which is a whole-film treatment rather than a scene element"
            ),
        }
    }
}

impl VideoCatalog {
    /// Parses a catalog manifest, rejecting an unsupported schema.
    pub fn parse(manifest: &str) -> SfumatoResult<Self> {
        let catalog: Self = serde_json::from_str(manifest).map_err(|error| {
            crate::errors::SfumatoError::config(format_args!(
                "Managed video catalog manifest is invalid: {error}"
            ))
        })?;
        if catalog.schema_version != 2 {
            return Err(crate::errors::SfumatoError::config(format_args!(
                "Unsupported managed video catalog schema {}",
                catalog.schema_version
            )));
        }
        Ok(catalog)
    }

    /// Version string used to detect a stale managed installation.
    pub fn version(&self) -> &str {
        &self.catalog_version
    }

    /// Every curated item.
    pub fn items(&self) -> &[VideoCatalogItem] {
        &self.items
    }

    /// Looks one item up by registry ID.
    pub fn find(&self, id: &str) -> Option<&VideoCatalogItem> {
        self.items.iter().find(|item| item.id == id)
    }

    /// Items serving one beat function.
    pub fn for_role(&self, role: VideoCatalogRole) -> Vec<&VideoCatalogItem> {
        self.items.iter().filter(|item| item.role == role).collect()
    }

    /// Renders the planner-facing summary, grouped by role.
    ///
    /// Generated rather than hand-written so the prompt cannot drift from the
    /// items actually installed.
    pub fn summary(&self) -> String {
        let mut sections: BTreeMap<VideoCatalogRole, Vec<String>> = BTreeMap::new();
        for item in &self.items {
            let entry = match item.duration_seconds {
                Some(seconds) => format!("{} ({seconds}s: {})", item.id, item.summary),
                None => format!("{} ({})", item.id, item.summary),
            };
            sections.entry(item.role).or_default().push(entry);
        }
        let mut summary = format!("managed catalog {}", self.catalog_version);
        for (role, entries) in sections {
            summary.push_str(&format!("\n- {}: {}", role.as_str(), entries.join("; ")));
        }
        summary
    }

    /// Checks selected items against the catalog's own constraints.
    ///
    /// Deterministic on purpose: every violation here is a fact about ids and
    /// durations, so spending a model on it would be both slower and weaker.
    pub fn validate_selection(
        &self,
        scene: &str,
        scene_seconds: f32,
        selected: &[String],
    ) -> Vec<VideoCatalogViolation> {
        let mut violations = Vec::new();
        for id in selected {
            let Some(item) = self.find(id) else {
                violations.push(VideoCatalogViolation::UnknownItem {
                    scene: scene.to_string(),
                    id: id.clone(),
                });
                continue;
            };
            if item.role == VideoCatalogRole::Grade {
                violations.push(VideoCatalogViolation::GradePerScene {
                    scene: scene.to_string(),
                    id: id.clone(),
                });
            }
            if let Some(item_seconds) = item.duration_seconds
                && scene_seconds < item_seconds
            {
                violations.push(VideoCatalogViolation::SceneTooShort {
                    scene: scene.to_string(),
                    id: id.clone(),
                    scene_seconds,
                    item_seconds,
                });
            }
        }
        violations
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

/// Complete inputs for turning document Markdown into printable HTML.
pub struct DocumentAssemblyRequest<'a> {
    /// Validated document the assembler renders.
    pub document: &'a SectionedDocument,
    /// Selected visual theme package.
    pub theme: &'a ThemePackage,
    /// Resolved page geometry and furniture.
    pub setup: crate::generation::DocumentPageSetup,
    /// Project name shown on the cover.
    pub project: &'a str,
    /// Cover date, taken from the revision rather than the clock so the same
    /// revision always produces the same page.
    pub revision_date: &'a str,
    /// Generated assets the markup may reference.
    pub allowed_assets: &'a [PathBuf],
}

/// Printable HTML plus the offline runtimes embedded into it.
#[derive(Debug)]
pub struct AssembledDocument {
    /// Complete standalone HTML document, ready to paginate.
    pub html: String,
    /// Runtimes embedded into the document, for the revision manifest.
    pub runtimes: Vec<PageRuntimeSelection>,
}

/// Deterministic Markdown-to-printable-HTML compiler for documents.
pub trait DocumentAssembler: Send + Sync {
    /// Assembles one printable HTML document from validated Markdown.
    fn assemble(&self, request: DocumentAssemblyRequest<'_>) -> SfumatoResult<AssembledDocument>;
}

/// Complete inputs for rendering one printable document workspace.
///
/// A workspace root rather than a single path, because a document is a tree: the
/// printable HTML resolves its diagrams, images and fonts relative to that root.
/// A renderer that runs somewhere else therefore receives the whole root, which
/// is also what a future service boundary would have to ship.
pub struct DocumentRenderRequest<'a> {
    /// Directory holding the document and every asset it references.
    pub workspace_root: &'a Path,
    /// Printable HTML, relative to the workspace root.
    pub document: &'a Path,
    /// Artifact destination, relative to the workspace root.
    pub output: &'a Path,
    /// Page setup the document was assembled for.
    pub setup: crate::generation::DocumentPageSetup,
}

/// What one rendering pass produced.
#[derive(Clone, Debug)]
pub struct RenderedDocument {
    /// Pages the renderer reported writing.
    pub pages: usize,
}

/// Pagination and PDF export for printable documents.
///
/// Pagination, the page numbers, the running header and the contents page
/// references are all resolved by one paginating renderer in a single session.
/// Splitting them — paginating in one process and printing in another — loses the
/// cross-page references, and printing without waiting for pagination produces a
/// different page count on every run.
#[async_trait]
pub trait DocumentRenderer: Send + Sync {
    /// Renders the document to PDF.
    async fn render_pdf(
        &self,
        request: DocumentRenderRequest<'_>,
        operation: &OperationContext,
    ) -> SfumatoResult<RenderedDocument>;

    /// Paginates the document and measures its page-format defects.
    async fn inspect_format(
        &self,
        request: DocumentRenderRequest<'_>,
        operation: &OperationContext,
    ) -> SfumatoResult<Vec<crate::generation::DocumentFormatIssue>>;
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
    /// Validates renderer source before a costly final encode.
    async fn validate(
        &self,
        _engine: VideoEngine,
        _request: &VideoRenderRequest,
        _operation: &OperationContext,
    ) -> SfumatoResult<()> {
        Ok(())
    }

    /// Captures deterministic visual evidence at requested timeline positions.
    async fn snapshot(
        &self,
        _engine: VideoEngine,
        _request: &VideoRenderRequest,
        _timestamps: &[f32],
        _output_dir: &Path,
        _operation: &OperationContext,
    ) -> SfumatoResult<Vec<PathBuf>> {
        Ok(Vec::new())
    }

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

    /// The curated catalog this renderer can wire into a composition.
    ///
    /// Owned by the renderer because the items ship with its managed
    /// installation; workflows read it instead of restating the item list.
    fn catalog(&self, _engine: VideoEngine) -> SfumatoResult<Option<VideoCatalog>> {
        Ok(None)
    }
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
