use std::{collections::BTreeMap, path::PathBuf};

use serde::Serialize;

use crate::config::Capability;
use crate::project_assets::ProjectAssetReference;
use crate::prompts::PromptProvenance;

#[derive(Clone, Debug)]
pub struct GenerationRequest {
    pub instruction: String,
    pub sources: Vec<PathBuf>,
    pub resource_kind: ResourceKind,
    pub project: Option<String>,
    pub model_overrides: BTreeMap<Capability, String>,
}

#[derive(Clone, Copy, Debug)]
pub enum ResourceKind {
    Slides,
    Page,
    Video,
    Document,
}

#[derive(Clone, Debug, serde::Deserialize, Serialize, PartialEq, Eq)]
pub struct PagePluginSelection {
    pub id: String,
    pub name: String,
    pub version: String,
    pub runtime_hash: String,
}

#[derive(Clone, Debug, serde::Deserialize, Serialize, PartialEq, Eq)]
pub struct PageRuntimeSelection {
    pub id: String,
    pub name: String,
    pub version: String,
    pub runtime_hash: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PageIssueKind {
    RuntimeError,
    RejectedPromise,
    MissingImage,
    MissingVideo,
    BlankContent,
    HorizontalOverflow,
    UnrenderedMath,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct PageInspectionIssue {
    pub viewport: String,
    pub kind: PageIssueKind,
    pub message: String,
    pub overflow_px: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct PageReviewSummary {
    pub enabled: bool,
    pub semantic_review: ReviewStatus,
    pub browser_check: ReviewStatus,
    pub repair: ReviewStatus,
    pub remaining_issues: Vec<PageInspectionIssue>,
}

impl PageReviewSummary {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            semantic_review: if enabled {
                ReviewStatus::Pending
            } else {
                ReviewStatus::Skipped
            },
            browser_check: ReviewStatus::Pending,
            repair: ReviewStatus::NotNeeded,
            remaining_issues: Vec::new(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PageGenerationOutput {
    pub project: String,
    pub title: String,
    pub html_path: PathBuf,
    pub project_instructions: Option<PathBuf>,
    pub models: BTreeMap<String, String>,
    pub plugins: Vec<PagePluginSelection>,
    pub template: Option<String>,
    pub project_assets: Vec<ProjectAssetReference>,
    pub runtimes: Vec<PageRuntimeSelection>,
    pub tools: Vec<GenerationToolSummary>,
    pub artifacts: Vec<PathBuf>,
    pub published_artifacts: Vec<PathBuf>,
    pub review: PageReviewSummary,
    pub prompts: Vec<PromptProvenance>,
}

#[derive(Debug, Serialize)]
pub struct GenerationOutput {
    pub project: String,
    pub project_instructions: Option<PathBuf>,
    pub models: BTreeMap<String, String>,
    pub tools: Vec<GenerationToolSummary>,
    pub template: Option<String>,
    pub project_assets: Vec<ProjectAssetReference>,
    pub artifacts: Vec<PathBuf>,
    pub published_artifacts: Vec<PathBuf>,
    pub review: SlideReviewSummary,
    /// Prompt templates that contributed to model requests in this run.
    pub prompts: Vec<PromptProvenance>,
}

/// Machine-readable result of one document generation.
#[derive(Clone, Debug, Serialize)]
pub struct DocumentGenerationOutput {
    pub project: String,
    pub project_instructions: Option<PathBuf>,
    pub models: BTreeMap<String, String>,
    pub tools: Vec<GenerationToolSummary>,
    pub template: Option<String>,
    pub project_assets: Vec<ProjectAssetReference>,
    pub artifacts: Vec<PathBuf>,
    pub published_artifacts: Vec<PathBuf>,
    pub review: DocumentReviewSummary,
    /// Page setup the document was rendered with.
    pub page_setup: DocumentPageSetup,
    /// Offline runtimes embedded into the printable HTML.
    pub runtimes: Vec<PageRuntimeSelection>,
    /// Prompt templates that contributed to model requests in this run.
    pub prompts: Vec<PromptProvenance>,
}

/// Resolved page geometry and furniture for one document render.
///
/// Resolved once, before any model call, so the drafter, the renderer and the
/// committed manifest all describe the same page.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub struct DocumentPageSetup {
    /// Physical sheet the PDF is printed on.
    pub page_size: DocumentPageSize,
    /// Whether a generated table of contents precedes the body.
    pub table_of_contents: bool,
    /// Whether a generated cover page precedes everything.
    pub cover: bool,
}

/// Physical sheet a document is printed on.
#[derive(Clone, Copy, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DocumentPageSize {
    /// ISO A4, 210 by 297 millimetres.
    #[default]
    A4,
    /// US Letter, 8.5 by 11 inches.
    Letter,
}

impl DocumentPageSize {
    /// Stable identifier used by CSS, themes, and the CLI.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::A4 => "a4",
            Self::Letter => "letter",
        }
    }

    /// The CSS `@page size` value for this sheet.
    pub const fn css_size(self) -> &'static str {
        match self {
            Self::A4 => "210mm 297mm",
            Self::Letter => "8.5in 11in",
        }
    }
}

impl std::str::FromStr for DocumentPageSize {
    type Err = crate::errors::SfumatoError;

    fn from_str(value: &str) -> crate::errors::SfumatoResult<Self> {
        match value.to_ascii_lowercase().as_str() {
            "a4" => Ok(Self::A4),
            "letter" => Ok(Self::Letter),
            _ => Err(crate::errors::SfumatoError::validation(format!(
                "Unsupported page size '{value}'. Use a4 or letter."
            ))),
        }
    }
}

/// Review and validation state for a generated video.
#[derive(Clone, Debug, Serialize)]
pub struct VideoReviewSummary {
    /// Whether semantic review was requested.
    pub enabled: bool,
    /// Semantic plan review status.
    pub semantic_review: ReviewStatus,
    /// Renderer-source repair status.
    pub source_repair: ReviewStatus,
    /// Snapshot/contact-sheet visual review status.
    pub visual_review: ReviewStatus,
    /// Final MP4 inspection status.
    pub media_inspection: ReviewStatus,
}

/// How snapshot evidence was reviewed for a Hyperframe production.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoVisualReviewMode {
    /// No visual review was requested.
    Disabled,
    /// Snapshots are waiting for an explicit human approval.
    HumanApprovalRequired,
    /// An image-capable reviewer inspected the contact sheet.
    Automated,
    /// Snapshots exist, but no compatible image reviewer was configured.
    EvidenceOnly,
}

/// Managed, immutable Hyperframe review session returned by a paused run.
#[derive(Clone, Debug, Serialize)]
pub struct VideoReviewSession {
    /// Identifier accepted by `sfumato video preview` and `sfumato video approve`.
    pub review_id: String,
    /// Current session state.
    pub status: String,
    /// Root containing the exact source bundle and visual evidence.
    pub root: PathBuf,
}

/// Structured result of an automated visual review when one was available.
#[derive(Clone, Debug, Serialize)]
pub struct VideoVisualReport {
    /// Whether the result permits rendering.
    pub approved: bool,
    /// Snapshot-level findings. Empty means no automated findings were produced.
    pub findings: Vec<String>,
}

impl VideoReviewSummary {
    /// Creates the initial review state.
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            semantic_review: if enabled {
                ReviewStatus::Pending
            } else {
                ReviewStatus::Skipped
            },
            source_repair: ReviewStatus::NotNeeded,
            visual_review: ReviewStatus::Skipped,
            media_inspection: ReviewStatus::Pending,
        }
    }
}

/// Machine-readable output for standalone video generation.
#[derive(Debug, Serialize)]
pub struct VideoGenerationOutput {
    /// Selected project.
    pub project: String,
    /// Generated educational title.
    pub title: String,
    /// Selected video engine.
    pub engine: sfumato_domain::VideoEngine,
    /// Committed MP4 path.
    pub video_path: PathBuf,
    /// Model profiles selected by role or capability.
    pub models: BTreeMap<String, String>,
    /// Tools exposed to the planner.
    pub tools: Vec<GenerationToolSummary>,
    /// Reusable artifacts selected by the plan.
    pub project_assets: Vec<ProjectAssetReference>,
    /// Files in the committed immutable revision.
    pub artifacts: Vec<PathBuf>,
    /// Published processed artifacts.
    pub published_artifacts: Vec<PathBuf>,
    /// Review and validation state.
    pub review: VideoReviewSummary,
    /// Visual-review path used for this production.
    pub visual_review_mode: VideoVisualReviewMode,
    /// Persisted session when rendering has been paused for a human.
    pub review_session: Option<VideoReviewSession>,
    /// Automated visual-review findings, if an image reviewer was configured.
    pub visual_report: Option<VideoVisualReport>,
    /// Prompt provenance.
    pub prompts: Vec<PromptProvenance>,
    /// Non-fatal workflow warnings.
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct GenerationToolSummary {
    pub name: String,
    pub description: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct SlideReviewSummary {
    pub enabled: bool,
    pub semantic_review: ReviewStatus,
    pub context_compaction: ReviewStatus,
    pub layout_check: ReviewStatus,
    pub repair: ReviewStatus,
    pub remaining_issues: Vec<SlideLayoutIssue>,
}

impl SlideReviewSummary {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            semantic_review: ReviewStatus::Skipped,
            context_compaction: ReviewStatus::Skipped,
            layout_check: ReviewStatus::Skipped,
            repair: ReviewStatus::Skipped,
            remaining_issues: Vec::new(),
        }
    }

    pub fn enabled() -> Self {
        Self {
            enabled: true,
            semantic_review: ReviewStatus::Pending,
            context_compaction: ReviewStatus::NotNeeded,
            layout_check: ReviewStatus::Pending,
            repair: ReviewStatus::NotNeeded,
            remaining_issues: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStatus {
    Pending,
    Completed,
    Skipped,
    Failed,
    NotNeeded,
    Accepted,
    Rejected,
}

#[derive(Clone, Debug, Serialize, serde::Deserialize, PartialEq)]
pub struct SlideLayoutIssue {
    pub slide: usize,
    pub title: String,
    pub vertical_overflow_px: u32,
    pub horizontal_overflow_px: u32,
}

/// Review and validation state for one generated document.
#[derive(Clone, Debug, Serialize)]
pub struct DocumentReviewSummary {
    pub enabled: bool,
    pub semantic_review: ReviewStatus,
    pub context_compaction: ReviewStatus,
    pub format_check: ReviewStatus,
    pub repair: ReviewStatus,
    pub remaining_issues: Vec<DocumentFormatIssue>,
}

impl DocumentReviewSummary {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            semantic_review: ReviewStatus::Skipped,
            context_compaction: ReviewStatus::Skipped,
            format_check: ReviewStatus::Skipped,
            repair: ReviewStatus::Skipped,
            remaining_issues: Vec::new(),
        }
    }

    pub fn enabled() -> Self {
        Self {
            enabled: true,
            semantic_review: ReviewStatus::Pending,
            context_compaction: ReviewStatus::NotNeeded,
            format_check: ReviewStatus::Pending,
            repair: ReviewStatus::NotNeeded,
            remaining_issues: Vec::new(),
        }
    }
}

/// What went wrong on one page of a paginated document.
///
/// A deck overflows because a slide is a fixed box; prose reflows instead, so
/// these name the defects that survive pagination rather than a single overflow
/// measurement.
#[derive(Clone, Copy, Debug, Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DocumentFormatIssueKind {
    /// Content reaches past the page's text column, horizontally.
    OverflowsTextColumn,
    /// A single block is taller than one page can ever hold.
    TallerThanPage,
    /// A heading sits at the very bottom with its body on the next page.
    OrphanedHeading,
    /// A page carries almost nothing because a block could not be split.
    NearlyEmptyPage,
}

/// One format defect measured on the paginated document.
#[derive(Clone, Debug, Serialize, serde::Deserialize, PartialEq)]
pub struct DocumentFormatIssue {
    /// One-based page number the defect appears on.
    pub page: usize,
    /// One-based reading position of the section that owns the defect.
    pub section: usize,
    /// Heading of that section, for reporting.
    pub heading: String,
    /// What is wrong.
    pub kind: DocumentFormatIssueKind,
    /// How far past its bounds the offending content reaches, in pixels.
    pub overflow_px: u32,
    /// The offending element, as a short CSS-like description.
    pub element: String,
}
