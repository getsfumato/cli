use std::collections::{BTreeMap, BTreeSet};

use json_patch::PatchOperation;
use serde::{Deserialize, Serialize};

use crate::{
    Patch, PatchReport, ReviewConstraint, ReviewError, ReviewFormat, ReviewSnapshot,
    ReviewableDocument, RevisionId, ValidationReport, validate_rfc6902_patch,
};

/// Current schema version for semantic video plans.
pub const VIDEO_PLAN_SCHEMA_VERSION: u32 = 1;
/// Current schema version for renderer-owned video source documents.
pub const VIDEO_SOURCE_SCHEMA_VERSION: u32 = 1;

const MAX_VIDEO_SECONDS: u32 = 3_600;
const MAX_TEXT_CHARS: usize = 128_000;
const MAX_SOURCE_CHARS: usize = 1_000_000;

/// Engine selected to materialize a video plan.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VideoEngine {
    /// Offline HTML, CSS, and JavaScript rendered by Hyperframe.
    Hyperframe,
    /// Offline Python scene rendered by Manim.
    Manim,
    /// Asynchronous video-generation model exposed by a connector.
    Model,
}

/// One timed semantic scene in a generated video.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VideoScene {
    /// Stable scene identifier used by source generators.
    pub id: String,
    /// Scene start in seconds.
    pub start_seconds: f32,
    /// Scene duration in seconds.
    pub duration_seconds: f32,
    /// Teaching content communicated by the scene.
    pub content: String,
    /// Visual composition and motion direction.
    pub visual: String,
    /// Reusable project artifact references selected for the scene.
    #[serde(default)]
    pub artifacts: Vec<String>,
}

/// Revision-guarded semantic plan shared by every video engine.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VideoPlanDocument {
    schema_version: u32,
    revision: RevisionId,
    engine: VideoEngine,
    title: String,
    objective: String,
    duration_seconds: u32,
    scenes: Vec<VideoScene>,
    artifacts: Vec<String>,
    visual_direction: String,
    remote_prompt: String,
}

impl VideoPlanDocument {
    /// Creates and validates a video plan produced by the drafting workflow.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        engine: VideoEngine,
        title: impl Into<String>,
        objective: impl Into<String>,
        duration_seconds: u32,
        scenes: Vec<VideoScene>,
        artifacts: Vec<String>,
        visual_direction: impl Into<String>,
        remote_prompt: impl Into<String>,
    ) -> Result<Self, ReviewError> {
        let mut document = Self {
            schema_version: VIDEO_PLAN_SCHEMA_VERSION,
            revision: revision_for("video-plan"),
            engine,
            title: title.into().trim().to_owned(),
            objective: objective.into().trim().to_owned(),
            duration_seconds,
            scenes,
            artifacts,
            visual_direction: visual_direction.into().trim().to_owned(),
            remote_prompt: remote_prompt.into().trim().to_owned(),
        };
        document.refresh_revision();
        document.validate_document()?;
        Ok(document)
    }

    /// Returns the optimistic-concurrency revision.
    pub fn revision(&self) -> &RevisionId {
        &self.revision
    }
    /// Returns the selected renderer engine.
    pub const fn engine(&self) -> VideoEngine {
        self.engine
    }
    /// Returns the human-readable title.
    pub fn title(&self) -> &str {
        &self.title
    }
    /// Returns the requested duration.
    pub const fn duration_seconds(&self) -> u32 {
        self.duration_seconds
    }
    /// Returns the reviewed scene sequence.
    pub fn scenes(&self) -> &[VideoScene] {
        &self.scenes
    }
    /// Returns reusable artifact references selected by the planner.
    pub fn artifacts(&self) -> &[String] {
        &self.artifacts
    }
    /// Returns engine-neutral visual direction.
    pub fn visual_direction(&self) -> &str {
        &self.visual_direction
    }
    /// Returns the reviewed prompt used by direct video models.
    pub fn remote_prompt(&self) -> &str {
        &self.remote_prompt
    }

    fn refresh_revision(&mut self) {
        let mut value = self.clone();
        value.revision = revision_for("video-plan");
        self.revision = revision_for(&serde_json::to_string(&value).unwrap_or_default());
    }

    fn validate_document(&self) -> Result<ValidationReport, ReviewError> {
        if self.schema_version != VIDEO_PLAN_SCHEMA_VERSION {
            return invalid_video("unsupported video-plan schema version");
        }
        validate_text("title", &self.title, 200, true)?;
        validate_text("objective", &self.objective, MAX_TEXT_CHARS, true)?;
        validate_text(
            "visual_direction",
            &self.visual_direction,
            MAX_TEXT_CHARS,
            true,
        )?;
        if self.engine == VideoEngine::Model {
            validate_text("remote_prompt", &self.remote_prompt, MAX_TEXT_CHARS, true)?;
        }
        if self.duration_seconds == 0 || self.duration_seconds > MAX_VIDEO_SECONDS {
            return invalid_video("duration must be between 1 and 3600 seconds");
        }
        if self.scenes.is_empty() {
            return invalid_video("video plan must contain at least one scene");
        }
        let mut ids = BTreeSet::new();
        for scene in &self.scenes {
            if scene.id.trim().is_empty() || !ids.insert(scene.id.clone()) {
                return invalid_video("scene identifiers must be non-empty and unique");
            }
            if !scene.start_seconds.is_finite()
                || !scene.duration_seconds.is_finite()
                || scene.start_seconds < 0.0
                || scene.duration_seconds <= 0.0
                || scene.start_seconds + scene.duration_seconds
                    > self.duration_seconds as f32 + 0.05
            {
                return invalid_video(format!("scene '{}' has invalid timing", scene.id));
            }
            validate_text("scene content", &scene.content, MAX_TEXT_CHARS, true)?;
            validate_text("scene visual", &scene.visual, MAX_TEXT_CHARS, true)?;
        }
        let mut expected = self.clone();
        expected.refresh_revision();
        if self.revision != expected.revision {
            return invalid_video("video plan contains a stale revision");
        }
        Ok(ValidationReport::default())
    }

    fn validate_patch_intent(&self, patch: &Patch) -> Result<(), ReviewError> {
        validate_rfc6902_patch(patch)?;
        let mut tested = false;
        for operation in &patch.0 {
            if let PatchOperation::Test(test) = operation {
                if test.path.as_str() != "/revision" {
                    return invalid_patch("video review may only test `/revision`");
                }
                tested = true;
                continue;
            }
            if !tested {
                return invalid_patch("video patch must test `/revision` before mutations");
            }
            let path = operation_path(operation);
            if !matches!(
                operation,
                PatchOperation::Add(_) | PatchOperation::Remove(_) | PatchOperation::Replace(_)
            ) || !is_plan_path(path)
            {
                return invalid_patch(format!("reviewer cannot mutate video field `{path}`"));
            }
        }
        Ok(())
    }
}

impl ReviewableDocument for VideoPlanDocument {
    fn snapshot(&self) -> Result<ReviewSnapshot, ReviewError> {
        self.validate_document()?;
        Ok(ReviewSnapshot {
            schema_version: VIDEO_PLAN_SCHEMA_VERSION,
            format: ReviewFormat::VideoPlan,
            revision: self.revision.clone(),
            document: serde_json::to_value(self)
                .map_err(|source| ReviewError::DocumentEncoding { source })?,
            constraints: vec![
                ReviewConstraint::Rfc6902Only,
                ReviewConstraint::TestDocumentRevision,
                ReviewConstraint::PreserveMetadata,
                ReviewConstraint::VideoPlanFieldsOnly,
            ],
        })
    }

    fn validate_patch(&self, patch: &Patch) -> Result<(), ReviewError> {
        self.validate_patch_intent(patch)
    }

    fn apply_patch(&mut self, patch: &Patch) -> Result<PatchReport, ReviewError> {
        self.validate_patch_intent(patch)?;
        let original = self.clone();
        let mut value = serde_json::to_value(&original)
            .map_err(|source| ReviewError::DocumentEncoding { source })?;
        json_patch::patch(&mut value, patch)
            .map_err(|source| ReviewError::PatchApplication { source })?;
        let mut candidate: Self = serde_json::from_value(value)
            .map_err(|source| ReviewError::InvalidPatchedVideo { source })?;
        candidate.refresh_revision();
        candidate.validate_document()?;
        let changed_nodes = changed_plan_fields(&original, &candidate);
        *self = candidate;
        Ok(PatchReport {
            operations: patch.0.len(),
            changed_nodes,
        })
    }

    fn validate(&self) -> Result<ValidationReport, ReviewError> {
        self.validate_document()
    }

    fn render(&self) -> Result<String, ReviewError> {
        self.validate_document()?;
        serde_json::to_string_pretty(self)
            .map_err(|source| ReviewError::DocumentEncoding { source })
    }
}

/// Revision-guarded renderer source files repaired after local validation errors.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VideoSourceDocument {
    schema_version: u32,
    revision: RevisionId,
    engine: VideoEngine,
    files: BTreeMap<String, String>,
}

impl VideoSourceDocument {
    /// Creates a validated source bundle for Hyperframe or Manim.
    pub fn new(engine: VideoEngine, files: BTreeMap<String, String>) -> Result<Self, ReviewError> {
        if engine == VideoEngine::Model {
            return invalid_video("direct model videos do not have renderer source files");
        }
        let mut document = Self {
            schema_version: VIDEO_SOURCE_SCHEMA_VERSION,
            revision: revision_for("video-source"),
            engine,
            files,
        };
        document.refresh_revision();
        document.validate_document()?;
        Ok(document)
    }

    /// Returns source files by safe relative name.
    pub fn files(&self) -> &BTreeMap<String, String> {
        &self.files
    }
    /// Returns the selected renderer engine.
    pub const fn engine(&self) -> VideoEngine {
        self.engine
    }

    fn refresh_revision(&mut self) {
        let encoded = serde_json::to_string(&(self.engine, &self.files)).unwrap_or_default();
        self.revision = revision_for(&encoded);
    }

    fn validate_document(&self) -> Result<ValidationReport, ReviewError> {
        if self.schema_version != VIDEO_SOURCE_SCHEMA_VERSION || self.files.is_empty() {
            return invalid_video("video source has an invalid schema or empty file set");
        }
        for (path, source) in &self.files {
            if !safe_source_path(path)
                || source.chars().count() > MAX_SOURCE_CHARS
                || source.contains('\0')
            {
                return invalid_video(format!("invalid renderer source file `{path}`"));
            }
        }
        let mut expected = self.clone();
        expected.refresh_revision();
        if self.revision != expected.revision {
            return invalid_video("video source contains a stale revision");
        }
        Ok(ValidationReport::default())
    }
}

impl ReviewableDocument for VideoSourceDocument {
    fn snapshot(&self) -> Result<ReviewSnapshot, ReviewError> {
        self.validate_document()?;
        Ok(ReviewSnapshot {
            schema_version: VIDEO_SOURCE_SCHEMA_VERSION,
            format: ReviewFormat::VideoSource,
            revision: self.revision.clone(),
            document: serde_json::to_value(self)
                .map_err(|source| ReviewError::DocumentEncoding { source })?,
            constraints: vec![
                ReviewConstraint::Rfc6902Only,
                ReviewConstraint::TestDocumentRevision,
                ReviewConstraint::PreserveMetadata,
                ReviewConstraint::VideoSourceFilesOnly,
            ],
        })
    }

    fn validate_patch(&self, patch: &Patch) -> Result<(), ReviewError> {
        validate_rfc6902_patch(patch)?;
        let mut tested = false;
        for operation in &patch.0 {
            if let PatchOperation::Test(test) = operation {
                if test.path.as_str() != "/revision" {
                    return invalid_patch("source repair may only test `/revision`");
                }
                tested = true;
                continue;
            }
            if !tested {
                return invalid_patch("source patch must test `/revision` before mutations");
            }
            let PatchOperation::Replace(replace) = operation else {
                return invalid_patch("source repair only accepts `test` and `replace`");
            };
            if !replace.path.as_str().starts_with("/files/") {
                return invalid_patch("source repair may only replace declared files");
            }
        }
        Ok(())
    }

    fn apply_patch(&mut self, patch: &Patch) -> Result<PatchReport, ReviewError> {
        self.validate_patch(patch)?;
        let original = self.clone();
        let mut value = serde_json::to_value(&original)
            .map_err(|source| ReviewError::DocumentEncoding { source })?;
        json_patch::patch(&mut value, patch)
            .map_err(|source| ReviewError::PatchApplication { source })?;
        let mut candidate: Self = serde_json::from_value(value)
            .map_err(|source| ReviewError::InvalidPatchedVideo { source })?;
        if candidate.files.keys().collect::<Vec<_>>() != original.files.keys().collect::<Vec<_>>() {
            return invalid_patch("source repair cannot add, remove, or rename files");
        }
        candidate.refresh_revision();
        candidate.validate_document()?;
        let changed_nodes = candidate
            .files
            .iter()
            .filter_map(|(path, value)| {
                (original.files.get(path) != Some(value)).then_some(path.clone())
            })
            .collect();
        *self = candidate;
        Ok(PatchReport {
            operations: patch.0.len(),
            changed_nodes,
        })
    }

    fn validate(&self) -> Result<ValidationReport, ReviewError> {
        self.validate_document()
    }

    fn render(&self) -> Result<String, ReviewError> {
        self.validate_document()?;
        serde_json::to_string_pretty(self)
            .map_err(|source| ReviewError::DocumentEncoding { source })
    }
}

fn operation_path(operation: &PatchOperation) -> &str {
    match operation {
        PatchOperation::Add(value) => value.path.as_str(),
        PatchOperation::Remove(value) => value.path.as_str(),
        PatchOperation::Replace(value) => value.path.as_str(),
        PatchOperation::Move(value) => value.path.as_str(),
        PatchOperation::Copy(value) => value.path.as_str(),
        PatchOperation::Test(value) => value.path.as_str(),
    }
}

fn is_plan_path(path: &str) -> bool {
    [
        "/title",
        "/objective",
        "/scenes",
        "/artifacts",
        "/visual_direction",
        "/remote_prompt",
    ]
    .iter()
    .any(|allowed| path == *allowed || path.starts_with(&format!("{allowed}/")))
}

fn changed_plan_fields(left: &VideoPlanDocument, right: &VideoPlanDocument) -> Vec<String> {
    [
        ("title", left.title != right.title),
        ("objective", left.objective != right.objective),
        ("scenes", left.scenes != right.scenes),
        ("artifacts", left.artifacts != right.artifacts),
        (
            "visual_direction",
            left.visual_direction != right.visual_direction,
        ),
        ("remote_prompt", left.remote_prompt != right.remote_prompt),
    ]
    .into_iter()
    .filter_map(|(name, changed)| changed.then_some(name.to_owned()))
    .collect()
}

fn validate_text(
    field: &str,
    value: &str,
    maximum: usize,
    required: bool,
) -> Result<(), ReviewError> {
    if required && value.trim().is_empty() {
        return invalid_video(format!("{field} cannot be empty"));
    }
    if value.chars().count() > maximum || value.contains('\0') {
        return invalid_video(format!(
            "{field} is too large or contains invalid characters"
        ));
    }
    Ok(())
}

fn safe_source_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains("..")
        && !path.contains('\\')
        && path.split('/').all(|part| !part.is_empty() && part != ".")
}

fn revision_for(value: &str) -> RevisionId {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    RevisionId::new(format!("{hash:016x}")).expect("hex video revision is a valid domain token")
}

fn invalid_patch<T>(message: impl Into<String>) -> Result<T, ReviewError> {
    Err(ReviewError::InvalidPatch(message.into()))
}

fn invalid_video<T>(message: impl Into<String>) -> Result<T, ReviewError> {
    Err(ReviewError::InvalidVideo(message.into()))
}

#[cfg(test)]
#[path = "../tests/unit/video.rs"]
mod tests;
