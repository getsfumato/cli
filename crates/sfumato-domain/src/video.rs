use std::collections::{BTreeMap, BTreeSet};

use json_patch::PatchOperation;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::{
    Patch, PatchReport, ReviewConstraint, ReviewError, ReviewFormat, ReviewSnapshot,
    ReviewableDocument, RevisionId, ValidationReport, validate_rfc6902_patch,
};

/// Current schema version for semantic video plans.
pub const VIDEO_PLAN_SCHEMA_VERSION: u32 = 2;
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

/// Creative workflow selected for a Hyperframe production.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum VideoWorkflow {
    /// Infer the best silent workflow from the supplied brief.
    #[default]
    Auto,
    /// A concept-first educational explanation.
    Explainer,
    /// A short silent motion-first unit.
    MotionGraphics,
    /// A product or website launch story.
    ProductLaunch,
    /// Designed overlays around supplied talking-head footage.
    TalkingHead,
    /// A presentation-like sequence of visual beats.
    Slideshow,
    /// A custom production that does not fit a specialised workflow.
    General,
}

impl VideoWorkflow {
    /// Stable CLI/prompt spelling for this workflow.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Explainer => "explainer",
            Self::MotionGraphics => "motion-graphics",
            Self::ProductLaunch => "product-launch",
            Self::TalkingHead => "talking-head",
            Self::Slideshow => "slideshow",
            Self::General => "general",
        }
    }
}

/// Production direction which turns a semantic scene into a buildable beat.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VideoSceneProduction {
    #[serde(default, deserialize_with = "deserialize_text_or_structured")]
    /// The beat's story function such as hook, proof, or payoff.
    pub narrative_role: String,
    #[serde(default, deserialize_with = "deserialize_text_list_or_structured")]
    /// Copy visible in the rendered frame.
    pub on_screen_copy: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_text_or_structured")]
    /// The primary visual focal point.
    pub focal_element: String,
    #[serde(default, deserialize_with = "deserialize_text_or_structured")]
    /// Video-frame layout strategy.
    pub layout: String,
    #[serde(default, deserialize_with = "deserialize_text_list_or_structured")]
    /// Background, midground, and foreground layers to author.
    pub layers: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_text_list_or_structured")]
    /// Named seek-safe motion rules selected for this beat.
    pub motion_rules: Vec<String>,
    /// Managed catalog item IDs wired into this beat.
    ///
    /// Kept separate from `artifacts`, which references project assets: these
    /// are renderer registry items with authored durations, so conflating the
    /// two makes both unvalidatable. Deliberately not tolerant of arbitrary
    /// shapes — an ID that is not a plain string is a planning error worth
    /// surfacing rather than flattening into JSON text.
    #[serde(default)]
    pub catalog_items: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_text_or_structured")]
    /// Deliberate beat entrance.
    pub entrance: String,
    #[serde(default, deserialize_with = "deserialize_text_or_structured")]
    /// Deliberate beat exit.
    pub exit: String,
    #[serde(default, deserialize_with = "deserialize_text_or_structured")]
    /// Transition toward the following beat.
    pub transition: String,
    #[serde(default, deserialize_with = "deserialize_text_list_or_structured")]
    /// Visual facts a reviewer can verify in a snapshot.
    pub acceptance: Vec<String>,
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
    #[serde(deserialize_with = "deserialize_text_or_structured")]
    pub content: String,
    /// Visual composition and motion direction.
    #[serde(deserialize_with = "deserialize_text_or_structured")]
    pub visual: String,
    /// Reusable project artifact references selected for the scene.
    #[serde(default)]
    pub artifacts: Vec<String>,
    /// Words spoken over this scene, or empty for a silent beat.
    ///
    /// Optional so every existing silent plan still deserializes. The spoken line
    /// is planned rather than derived from `content`: what a viewer hears is
    /// written for the ear, while `content` states what the beat teaches, and a
    /// synthesiser reading the latter aloud produces prose nobody would say.
    #[serde(default, deserialize_with = "deserialize_text_or_structured")]
    pub narration: String,
    /// Buildable composition, motion, and acceptance direction.
    #[serde(default)]
    pub production: VideoSceneProduction,
}

fn deserialize_text_or_structured<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    Value::deserialize(deserializer).map(structured_text)
}

fn deserialize_text_list_or_structured<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    Ok(match value {
        Value::Array(values) => values.into_iter().map(structured_text).collect(),
        Value::Null => Vec::new(),
        value => vec![structured_text(value)],
    })
}

fn structured_text(value: Value) -> String {
    match value {
        Value::String(value) => value,
        value => value.to_string(),
    }
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
    #[serde(default)]
    workflow: VideoWorkflow,
    #[serde(default)]
    message: String,
    #[serde(default)]
    narrative_arc: String,
    #[serde(default)]
    design_direction: String,
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
            workflow: VideoWorkflow::Auto,
            message: "Unspecified message".into(),
            narrative_arc: "Unspecified narrative arc".into(),
            design_direction: "Use the requested theme".into(),
        };
        document.refresh_revision();
        document.validate_document()?;
        Ok(document)
    }

    /// Creates a plan with the persisted Hyperframe production direction.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_pipeline(
        engine: VideoEngine,
        title: impl Into<String>,
        objective: impl Into<String>,
        duration_seconds: u32,
        scenes: Vec<VideoScene>,
        artifacts: Vec<String>,
        visual_direction: impl Into<String>,
        remote_prompt: impl Into<String>,
        workflow: VideoWorkflow,
        message: impl Into<String>,
        narrative_arc: impl Into<String>,
        design_direction: impl Into<String>,
    ) -> Result<Self, ReviewError> {
        let mut document = Self::new(
            engine,
            title,
            objective,
            duration_seconds,
            scenes,
            artifacts,
            visual_direction,
            remote_prompt,
        )?;
        document.set_pipeline(workflow, message, narrative_arc, design_direction)?;
        Ok(document)
    }

    /// Applies the production metadata drafted by the Hyperframe pipeline.
    pub fn set_pipeline(
        &mut self,
        workflow: VideoWorkflow,
        message: impl Into<String>,
        narrative_arc: impl Into<String>,
        design_direction: impl Into<String>,
    ) -> Result<(), ReviewError> {
        self.workflow = workflow;
        self.message = message.into().trim().to_owned();
        self.narrative_arc = narrative_arc.into().trim().to_owned();
        self.design_direction = design_direction.into().trim().to_owned();
        self.refresh_revision();
        self.validate_document().map(|_| ())
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
    /// Returns the teaching objective.
    pub fn objective(&self) -> &str {
        &self.objective
    }
    /// Returns the requested duration.
    pub const fn duration_seconds(&self) -> u32 {
        self.duration_seconds
    }

    /// Replaces the scene timeline and the film's total length.
    ///
    /// Narration is what makes this necessary: a spoken line runs as long as it
    /// runs, so the planned windows are an estimate until the words exist. The
    /// replacement goes through the document's own validation, which keeps a
    /// retimed plan as trustworthy as a drafted one — scenes that overrun the
    /// total are rejected here rather than at render time.
    pub fn set_timeline(
        &mut self,
        scenes: Vec<VideoScene>,
        duration_seconds: u32,
    ) -> Result<(), ReviewError> {
        self.scenes = scenes;
        self.duration_seconds = duration_seconds;
        self.refresh_revision();
        self.validate_document().map(|_| ())
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
    /// Returns the selected production workflow.
    pub const fn workflow(&self) -> VideoWorkflow {
        self.workflow
    }
    /// Returns the single message the production must communicate.
    pub fn message(&self) -> &str {
        &self.message
    }
    /// Returns the high-level story arc.
    pub fn narrative_arc(&self) -> &str {
        &self.narrative_arc
    }
    /// Returns the normalized design direction.
    pub fn design_direction(&self) -> &str {
        &self.design_direction
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
        if self.engine == VideoEngine::Hyperframe {
            validate_text("message", &self.message, MAX_TEXT_CHARS, true)?;
            validate_text("narrative_arc", &self.narrative_arc, MAX_TEXT_CHARS, true)?;
            validate_text(
                "design_direction",
                &self.design_direction,
                MAX_TEXT_CHARS,
                true,
            )?;
        }
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
            validate_text("scene narration", &scene.narration, MAX_TEXT_CHARS, false)?;
            validate_scene_production(&scene.production)?;
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
        "/workflow",
        "/message",
        "/narrative_arc",
        "/design_direction",
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
        ("workflow", left.workflow != right.workflow),
        ("message", left.message != right.message),
        ("narrative_arc", left.narrative_arc != right.narrative_arc),
        (
            "design_direction",
            left.design_direction != right.design_direction,
        ),
    ]
    .into_iter()
    .filter_map(|(name, changed)| changed.then_some(name.to_owned()))
    .collect()
}

fn validate_scene_production(value: &VideoSceneProduction) -> Result<(), ReviewError> {
    for (field, text) in [
        ("scene narrative_role", &value.narrative_role),
        ("scene focal_element", &value.focal_element),
        ("scene layout", &value.layout),
        ("scene entrance", &value.entrance),
        ("scene exit", &value.exit),
        ("scene transition", &value.transition),
    ] {
        validate_text(field, text, MAX_TEXT_CHARS, false)?;
    }
    if value.motion_rules.len() > 4 {
        return invalid_video("each scene may select at most four motion rules");
    }
    for text in value
        .on_screen_copy
        .iter()
        .chain(value.layers.iter())
        .chain(value.motion_rules.iter())
        .chain(value.acceptance.iter())
    {
        validate_text("scene production value", text, MAX_TEXT_CHARS, false)?;
    }
    Ok(())
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
