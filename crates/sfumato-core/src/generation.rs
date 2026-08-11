use std::{collections::BTreeMap, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::Capability;
use crate::project_assets::ProjectAssetReference;
use crate::prompts::PromptProvenance;

#[derive(Clone, Debug)]
/// What a caller asks for, before any configuration is resolved.
pub struct GenerationRequest {
    /// What to make, in the user's words. The one thing with no default.
    pub instruction: String,
    /// Files and directories to work from. Refused, not ignored, when the project
    /// is grounded in a brain: reading the brain while appearing to read these
    /// would answer a different question than the one asked.
    pub sources: Vec<PathBuf>,
    /// Which kind of resource.
    pub resource_kind: ResourceKind,
    /// Project to act on; `None` uses the active one.
    pub project: Option<String>,
    /// Per-capability profile choices for this run.
    pub model_overrides: BTreeMap<Capability, String>,
}
/// Which kind of resource a request is for.

#[derive(Clone, Copy, Debug)]
pub enum ResourceKind {
    /// A Marp deck, exported to PDF.
    Slides,
    /// A self-contained interactive HTML page.
    Page,
    /// An MP4 film.
    Video,
    /// A paginated prose document, exported to PDF.
    Document,
}

#[derive(Clone, Debug, serde::Deserialize, Serialize, PartialEq, Eq)]
/// A page plugin as it was actually resolved for one run.
///
/// Carries the resolved version and hash rather than the request, so a revision
/// records what was really embedded and not what was asked for.
pub struct PagePluginSelection {
    /// Registry identifier.
    pub id: String,
    /// Display name.
    pub name: String,
    /// The version that was embedded.
    pub version: String,
    /// SHA-256 of the runtime that was embedded, which is what makes the revision
    /// reproducible.
    pub runtime_hash: String,
}

#[derive(Clone, Debug, serde::Deserialize, Serialize, PartialEq, Eq)]
/// A runtime the page needed and got, such as MathJax for TeX.
///
/// Distinct from a plugin because nobody asked for it: it is added because the
/// content requires it, and recording it is how a page stays explicable offline.
pub struct PageRuntimeSelection {
    /// Runtime identifier.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Embedded version.
    pub version: String,
    /// SHA-256 of what was embedded.
    pub runtime_hash: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
/// What a browser found wrong with a generated page.
///
/// Enumerated rather than free text because each kind has a different repair, and
/// the drafter is told which one it is.
pub enum PageIssueKind {
    /// The page threw while running.
    RuntimeError,
    /// A promise rejected with nobody handling it, which usually means an async
    /// failure the page swallowed.
    RejectedPromise,
    /// An `img` resolved to nothing.
    MissingImage,
    /// A `video` resolved to nothing.
    MissingVideo,
    /// The page rendered, and rendered nothing — the failure that looks like success
    /// until someone opens it.
    BlankContent,
    /// Content ran off the side, which no viewport can recover from.
    HorizontalOverflow,
    /// TeX was left as source because the math runtime never ran.
    UnrenderedMath,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
/// One problem found in one viewport.
pub struct PageInspectionIssue {
    /// Which viewport surfaced it; a page is checked at desktop and mobile widths.
    pub viewport: String,
    /// What kind of problem.
    pub kind: PageIssueKind,
    /// The detail handed to the repair prompt.
    pub message: String,
    /// How far content overflowed, for the overflow kinds. Zero otherwise.
    pub overflow_px: u32,
}

#[derive(Clone, Debug, Serialize)]
/// What the review pass did, and what it could not fix.
pub struct PageReviewSummary {
    /// Whether review ran at all; `--no-review` turns it off.
    pub enabled: bool,
    /// Whether content was reviewed against the instruction and sources.
    pub semantic_review: ReviewStatus,
    /// Whether the page was opened and measured.
    pub browser_check: ReviewStatus,
    /// Whether a repair was attempted.
    pub repair: ReviewStatus,
    /// Problems that survived repair. Reported rather than hidden: a page that is
    /// still wrong is worth saying so about.
    pub remaining_issues: Vec<PageInspectionIssue>,
}

impl PageReviewSummary {
    /// An empty summary for a run that has not reviewed anything yet.
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
/// What a page generation produced.
pub struct PageGenerationOutput {
    /// The project this was generated in.
    pub project: String,
    /// Title the drafter chose, or the one the caller supplied.
    pub title: String,
    /// The assembled page.
    pub html_path: PathBuf,
    /// The project instruction file that shaped the run, when one was found.
    pub project_instructions: Option<PathBuf>,
    /// Which profile served each capability, so a revision records what made it.
    pub models: BTreeMap<String, String>,
    /// Plugins embedded, with the versions actually used.
    pub plugins: Vec<PagePluginSelection>,
    /// The structural template used, if any.
    pub template: Option<String>,
    /// Project assets referenced, such as a logo.
    pub project_assets: Vec<ProjectAssetReference>,
    /// Runtimes embedded because the content needed them.
    pub runtimes: Vec<PageRuntimeSelection>,
    /// Tools the drafter was offered.
    pub tools: Vec<GenerationToolSummary>,
    /// Files committed to the managed revision, which stays authoritative.
    pub artifacts: Vec<PathBuf>,
    /// Copies published beside the sources, when publishing was asked for.
    pub published_artifacts: Vec<PathBuf>,
    /// What review did, and what it could not fix.
    pub review: PageReviewSummary,
    /// Which prompt templates were used, and from which layer, so a surprising
    /// result can be traced to an override.
    pub prompts: Vec<PromptProvenance>,
}

#[derive(Debug, Serialize)]
/// What a slide generation produced.
pub struct GenerationOutput {
    /// The project this was generated in.
    pub project: String,
    /// The project instruction file that shaped the run, when one was found.
    pub project_instructions: Option<PathBuf>,
    /// Which profile served each capability, so a revision records what made it.
    pub models: BTreeMap<String, String>,
    /// Tools the drafter was offered.
    pub tools: Vec<GenerationToolSummary>,
    /// The structural template used, if any.
    pub template: Option<String>,
    /// Project assets referenced, such as a logo.
    pub project_assets: Vec<ProjectAssetReference>,
    /// Files committed to the managed revision, which stays authoritative.
    pub artifacts: Vec<PathBuf>,
    /// Copies published beside the sources, when publishing was asked for.
    pub published_artifacts: Vec<PathBuf>,
    /// What review did, and what it could not fix.
    pub review: SlideReviewSummary,
    /// Prompt templates that contributed to model requests in this run.
    /// Which prompt templates were used, and from which layer, so a surprising
    /// result can be traced to an override.
    pub prompts: Vec<PromptProvenance>,
}

/// Machine-readable result of one document generation.
#[derive(Clone, Debug, Serialize)]
pub struct DocumentGenerationOutput {
    /// The project this was generated in.
    pub project: String,
    /// The project instruction file that shaped the run, when one was found.
    pub project_instructions: Option<PathBuf>,
    /// Which profile served each capability, so a revision records what made it.
    pub models: BTreeMap<String, String>,
    /// Tools the drafter was offered.
    pub tools: Vec<GenerationToolSummary>,
    /// The structural template used, if any.
    pub template: Option<String>,
    /// Project assets referenced, such as a logo.
    pub project_assets: Vec<ProjectAssetReference>,
    /// Files committed to the managed revision, which stays authoritative.
    pub artifacts: Vec<PathBuf>,
    /// Copies published beside the sources, when publishing was asked for.
    pub published_artifacts: Vec<PathBuf>,
    /// What review did, and what it could not fix.
    pub review: DocumentReviewSummary,
    /// Page setup the document was rendered with.
    pub page_setup: DocumentPageSetup,
    /// Offline runtimes embedded into the printable HTML.
    /// Runtimes embedded because the content needed them.
    pub runtimes: Vec<PageRuntimeSelection>,
    /// Prompt templates that contributed to model requests in this run.
    /// Which prompt templates were used, and from which layer, so a surprising
    /// result can be traced to an override.
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
    /// Visual defects that survived repair.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frame_defects: Vec<VideoFrameDefect>,
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

/// What one snapshot of a rendered video contains.
///
/// Measured in the adapter and judged in the core, the same split the slide layout
/// inspector uses: decoding a PNG is infrastructure, deciding that a frame is too
/// empty to ship is policy.
#[derive(Clone, Debug, Serialize)]
pub struct VideoFrameMeasurement {
    /// Timeline position the frame was captured at, in seconds.
    pub at_seconds: f32,
    /// Share of pixels that differ from the frame's own dominant colour.
    ///
    /// A frame showing only its background reads as zero here whatever that
    /// background happens to be, so it needs no knowledge of the theme.
    pub ink_ratio: f32,
    /// Distinct quantised colours present.
    ///
    /// Separates a genuinely empty frame from a flat but deliberate one, such as a
    /// single word on a solid ground.
    pub distinct_colours: u32,
}

/// One way a rendered video looks wrong.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VideoFrameDefectKind {
    /// The frame carries no visible content at all.
    BlankFrame,
    /// A scene begins on an empty frame, so the cut lands on nothing.
    EmptySceneStart,
}

/// One measured visual defect in a rendered video.
#[derive(Clone, Debug, Serialize)]
pub struct VideoFrameDefect {
    /// Timeline position of the offending frame, in seconds.
    pub at_seconds: f32,
    /// Scene the position belongs to, one-based, when it maps to one.
    pub scene: Option<usize>,
    /// What is wrong.
    pub kind: VideoFrameDefectKind,
    /// The measurement that produced the verdict.
    pub measurement: VideoFrameMeasurement,
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
///
/// Deserialised straight from the reviewer's answer, so the shape of the contract
/// lives in one place instead of being restated by a parser.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VideoVisualReport {
    /// Whether the result permits rendering.
    pub approved: bool,
    /// Snapshot-level findings. Empty means no automated findings were produced.
    #[serde(default)]
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
            frame_defects: Vec::new(),
        }
    }
}

/// Machine-readable output for standalone video generation.
#[derive(Debug, Serialize)]
pub struct VideoGenerationOutput {
    /// Selected project.
    /// The project this was generated in.
    pub project: String,
    /// Generated educational title.
    pub title: String,
    /// Selected video engine.
    pub engine: sfumato_domain::VideoEngine,
    /// Committed MP4 path.
    pub video_path: PathBuf,
    /// Model profiles selected by role or capability.
    /// Which profile served each capability, so a revision records what made it.
    pub models: BTreeMap<String, String>,
    /// Tools exposed to the planner.
    /// Tools the drafter was offered.
    pub tools: Vec<GenerationToolSummary>,
    /// Reusable artifacts selected by the plan.
    /// Project assets referenced, such as a logo.
    pub project_assets: Vec<ProjectAssetReference>,
    /// Files in the committed immutable revision.
    /// Files committed to the managed revision, which stays authoritative.
    pub artifacts: Vec<PathBuf>,
    /// Published processed artifacts.
    /// Copies published beside the sources, when publishing was asked for.
    pub published_artifacts: Vec<PathBuf>,
    /// Review and validation state.
    pub review: VideoReviewSummary,
    /// Visual-review path used for this production.
    pub visual_review_mode: VideoVisualReviewMode,
    /// Persisted session when rendering has been paused for a human.
    pub review_session: Option<VideoReviewSession>,
    /// Automated visual-review findings, if an image reviewer was configured.
    pub visual_report: Option<VideoVisualReport>,
    /// Spoken narration layered onto the film, when it speaks.
    pub narration: Option<VideoNarrationSummary>,
    /// Prompt provenance.
    /// Which prompt templates were used, and from which layer, so a surprising
    /// result can be traced to an override.
    pub prompts: Vec<PromptProvenance>,
    /// Non-fatal workflow warnings.
    pub warnings: Vec<String>,
}

/// What was spoken over one film, for callers that never see the audio.
#[derive(Clone, Debug, Serialize)]
pub struct VideoNarrationSummary {
    /// Model profile that voiced the film.
    pub profile: String,
    /// Number of spoken passages, normally one per scene.
    pub segments: usize,
    /// Total spoken length in seconds, gaps included.
    pub spoken_seconds: f32,
    /// Caption groups generated from the provider's word alignment.
    pub caption_groups: usize,
}

#[derive(Clone, Debug, Serialize)]
/// A tool the drafter was offered, as a caller reports it.
pub struct GenerationToolSummary {
    /// Tool name.
    pub name: String,
    /// What it does.
    pub description: String,
}

#[derive(Clone, Debug, Serialize)]
/// What the deck review pass did.
pub struct SlideReviewSummary {
    /// Whether review ran; `--no-review` turns it off.
    pub enabled: bool,
    /// Whether content was reviewed against the instruction and sources.
    pub semantic_review: ReviewStatus,
    /// Whether the input had to be re-sent smaller to fit the context window.
    pub context_compaction: ReviewStatus,
    /// Whether the rendered deck was measured in a browser.
    pub layout_check: ReviewStatus,
    /// Whether overflowing slides were re-authored.
    pub repair: ReviewStatus,
    /// Slides that still overflow. Reported rather than hidden.
    pub remaining_issues: Vec<SlideLayoutIssue>,
}

impl SlideReviewSummary {
    /// A summary for a run with review turned off.
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

    /// A summary for a run that is about to review.
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
/// How one step of a review pass ended.
///
/// `Skipped`, `NotNeeded` and `Failed` are deliberately distinct: turned off, had
/// nothing to do, and tried and could not. Collapsing them would make a summary
/// unable to say whether silence meant success.
pub enum ReviewStatus {
    /// Not reached yet.
    Pending,
    /// Ran and finished.
    Completed,
    /// Deliberately not run.
    Skipped,
    /// Ran and could not finish.
    Failed,
    /// Nothing to do — there was no defect to repair.
    NotNeeded,
    /// A human approved a paused review.
    Accepted,
    /// A human rejected one.
    Rejected,
}

#[derive(Clone, Debug, Serialize, serde::Deserialize, PartialEq)]
/// One slide that does not fit its frame.
pub struct SlideLayoutIssue {
    /// Slide number in the deck, from one.
    pub slide: usize,
    /// Its heading, so a report names something recognisable.
    pub title: String,
    /// How far content ran past the bottom.
    pub vertical_overflow_px: u32,
    /// How far content ran past the side.
    pub horizontal_overflow_px: u32,
}

/// Review and validation state for one generated document.
#[derive(Clone, Debug, Serialize)]
pub struct DocumentReviewSummary {
    /// Whether review ran.
    pub enabled: bool,
    /// Whether content was reviewed against the instruction and sources.
    pub semantic_review: ReviewStatus,
    /// Whether the input had to be re-sent smaller.
    pub context_compaction: ReviewStatus,
    /// Whether the paginated result was measured, which is where prose reflow shows.
    pub format_check: ReviewStatus,
    /// Whether badly paginated content was re-authored.
    pub repair: ReviewStatus,
    /// Defects that survived. Reported rather than hidden.
    pub remaining_issues: Vec<DocumentFormatIssue>,
}

impl DocumentReviewSummary {
    /// A summary for a run with review turned off.
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

    /// A summary for a run that is about to review.
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
