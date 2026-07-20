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

/// Review and validation state for a generated video.
#[derive(Clone, Debug, Serialize)]
pub struct VideoReviewSummary {
    /// Whether semantic review was requested.
    pub enabled: bool,
    /// Semantic plan review status.
    pub semantic_review: ReviewStatus,
    /// Renderer-source repair status.
    pub source_repair: ReviewStatus,
    /// Final MP4 inspection status.
    pub media_inspection: ReviewStatus,
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
