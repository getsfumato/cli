use std::{collections::BTreeMap, path::PathBuf};

use serde::Serialize;

use crate::config::Capability;
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
    pub artifacts: Vec<PathBuf>,
    pub published_artifacts: Vec<PathBuf>,
    pub review: SlideReviewSummary,
    /// Prompt templates that contributed to model requests in this run.
    pub prompts: Vec<PromptProvenance>,
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
