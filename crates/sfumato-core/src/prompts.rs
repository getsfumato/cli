//! Provider-neutral prompt contracts.
//!
//! Workflows select stable prompt identifiers and supply structured values.
//! Loading, overriding, and rendering template text belongs to an adapter.

use std::{fmt, path::PathBuf, str::FromStr};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

/// Stable identifier for a model-facing prompt template.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PromptId {
    /// System prompt for a full slide draft.
    SlidesDraftSystem,
    /// User prompt for a full slide draft.
    SlidesDraftUser,
    /// System prompt for a compact slide draft retry.
    SlidesCompactDraftSystem,
    /// User prompt for a compact slide draft retry.
    SlidesCompactDraftUser,
    /// System prompt for repairing a missing title.
    SlidesTitleRepairSystem,
    /// User prompt for repairing a missing title.
    SlidesTitleRepairUser,
    /// System prompt for semantic review.
    SlidesReviewSystem,
    /// User prompt for semantic review.
    SlidesReviewUser,
    /// System prompt for compact semantic review.
    SlidesCompactReviewSystem,
    /// User prompt for compact semantic review.
    SlidesCompactReviewUser,
    /// System prompt for focused layout repair.
    SlidesLayoutRepairSystem,
    /// User prompt for focused layout repair.
    SlidesLayoutRepairUser,
    /// System prompt for compact focused layout repair.
    SlidesCompactLayoutRepairSystem,
    /// User prompt for compact focused layout repair.
    SlidesCompactLayoutRepairUser,
    /// System prompt for content-only deck editing.
    SlidesEditSystem,
    /// User prompt for content-only deck editing.
    SlidesEditUser,
    /// System prompt for compact content-only editing.
    SlidesCompactEditSystem,
    /// User prompt for compact content-only editing.
    SlidesCompactEditUser,
    /// Final user message when draft tools are exhausted.
    SlidesDraftToolExhaustedUser,
    /// Final user message when review tools are exhausted.
    SlidesReviewToolExhaustedUser,
    /// Final user message when edit tools are exhausted.
    SlidesEditToolExhaustedUser,
    /// Final user message when layout-repair tools are exhausted.
    SlidesLayoutRepairToolExhaustedUser,
    /// User prompt sent to an image-generation model.
    ImageGenerationUser,
    /// Model-facing descriptions for the generation tool registry.
    ToolsGenerationDescriptions,
}

impl PromptId {
    /// Returns every prompt identifier required by this version of Sfumato.
    pub const fn all() -> &'static [Self] {
        &[
            Self::SlidesDraftSystem,
            Self::SlidesDraftUser,
            Self::SlidesCompactDraftSystem,
            Self::SlidesCompactDraftUser,
            Self::SlidesTitleRepairSystem,
            Self::SlidesTitleRepairUser,
            Self::SlidesReviewSystem,
            Self::SlidesReviewUser,
            Self::SlidesCompactReviewSystem,
            Self::SlidesCompactReviewUser,
            Self::SlidesLayoutRepairSystem,
            Self::SlidesLayoutRepairUser,
            Self::SlidesCompactLayoutRepairSystem,
            Self::SlidesCompactLayoutRepairUser,
            Self::SlidesEditSystem,
            Self::SlidesEditUser,
            Self::SlidesCompactEditSystem,
            Self::SlidesCompactEditUser,
            Self::SlidesDraftToolExhaustedUser,
            Self::SlidesReviewToolExhaustedUser,
            Self::SlidesEditToolExhaustedUser,
            Self::SlidesLayoutRepairToolExhaustedUser,
            Self::ImageGenerationUser,
            Self::ToolsGenerationDescriptions,
        ]
    }

    /// Returns the manifest key used by template adapters and CLI commands.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SlidesDraftSystem => "slides.draft.system",
            Self::SlidesDraftUser => "slides.draft.user",
            Self::SlidesCompactDraftSystem => "slides.compact-draft.system",
            Self::SlidesCompactDraftUser => "slides.compact-draft.user",
            Self::SlidesTitleRepairSystem => "slides.title-repair.system",
            Self::SlidesTitleRepairUser => "slides.title-repair.user",
            Self::SlidesReviewSystem => "slides.review.system",
            Self::SlidesReviewUser => "slides.review.user",
            Self::SlidesCompactReviewSystem => "slides.compact-review.system",
            Self::SlidesCompactReviewUser => "slides.compact-review.user",
            Self::SlidesLayoutRepairSystem => "slides.layout-repair.system",
            Self::SlidesLayoutRepairUser => "slides.layout-repair.user",
            Self::SlidesCompactLayoutRepairSystem => "slides.compact-layout-repair.system",
            Self::SlidesCompactLayoutRepairUser => "slides.compact-layout-repair.user",
            Self::SlidesEditSystem => "slides.edit.system",
            Self::SlidesEditUser => "slides.edit.user",
            Self::SlidesCompactEditSystem => "slides.compact-edit.system",
            Self::SlidesCompactEditUser => "slides.compact-edit.user",
            Self::SlidesDraftToolExhaustedUser => "slides.draft.tool-exhausted.user",
            Self::SlidesReviewToolExhaustedUser => "slides.review.tool-exhausted.user",
            Self::SlidesEditToolExhaustedUser => "slides.edit.tool-exhausted.user",
            Self::SlidesLayoutRepairToolExhaustedUser => "slides.layout-repair.tool-exhausted.user",
            Self::ImageGenerationUser => "image.generation.user",
            Self::ToolsGenerationDescriptions => "tools.generation.descriptions",
        }
    }
}

impl fmt::Display for PromptId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for PromptId {
    type Err = PromptError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::all()
            .iter()
            .copied()
            .find(|id| id.as_str() == value)
            .ok_or_else(|| PromptError::UnknownId(value.to_string()))
    }
}

/// Structured values supplied to a prompt template.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(transparent)]
pub struct PromptVariables(pub Map<String, Value>);

impl PromptVariables {
    /// Converts a serializable typed context into template variables.
    pub fn from_serializable<T: Serialize>(context: &T) -> Result<Self, PromptError> {
        let value = serde_json::to_value(context).map_err(PromptError::Variables)?;
        let values = value
            .as_object()
            .cloned()
            .ok_or(PromptError::NonObjectContext)?;
        Ok(Self(values))
    }
}

/// A request to resolve and render one prompt template.
#[derive(Clone, Debug)]
pub struct PromptRenderRequest {
    /// Stable prompt identifier.
    pub id: PromptId,
    /// Structured template values.
    pub variables: PromptVariables,
}

/// Location from which a rendered template was resolved.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptOrigin {
    /// Template embedded in the Sfumato binary.
    Bundled,
    /// User-level filesystem override.
    User(PathBuf),
    /// Project-level filesystem override.
    Project(PathBuf),
}

/// Provenance for one rendered prompt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PromptProvenance {
    /// Stable template identifier.
    pub id: PromptId,
    /// Template source.
    pub origin: PromptOrigin,
    /// Prompt schema version from the bundled manifest.
    pub version: u32,
    /// SHA-256 hash of the resolved template source.
    pub content_hash: String,
}

/// Fully rendered model-facing prompt text and its provenance.
#[derive(Clone, Debug)]
pub struct RenderedPrompt {
    /// Rendered template text.
    pub text: String,
    /// Resolved template provenance.
    pub provenance: PromptProvenance,
}

/// Prompt loading or rendering failure.
#[derive(Debug, Error)]
pub enum PromptError {
    /// A string is not a known prompt identifier.
    #[error("unknown prompt template '{0}'")]
    UnknownId(String),
    /// A required template does not exist.
    #[error("prompt template '{0}' was not found")]
    Missing(PromptId),
    /// A template path or include violates containment rules.
    #[error("unsafe prompt template path: {0}")]
    UnsafePath(String),
    /// A template could not be loaded.
    #[error("could not load prompt template '{id}': {message}")]
    Load {
        /// Prompt identifier.
        id: PromptId,
        /// Adapter detail.
        message: String,
    },
    /// A template could not be rendered.
    #[error("could not render prompt template '{id}': {message}")]
    Render {
        /// Prompt identifier.
        id: PromptId,
        /// Strict template-engine diagnostic.
        message: String,
    },
    /// Typed context serialization failed.
    #[error("could not serialize prompt variables: {0}")]
    Variables(serde_json::Error),
    /// Template contexts must serialize to an object.
    #[error("prompt context must serialize to an object")]
    NonObjectContext,
}

/// Port for layered prompt resolution and strict rendering.
pub trait PromptCatalog: Send + Sync {
    /// Resolves and renders one prompt.
    fn render(&self, request: PromptRenderRequest) -> Result<RenderedPrompt, PromptError>;

    /// Validates that every required template resolves and compiles.
    fn validate(&self) -> Result<Vec<PromptProvenance>, PromptError>;
}

/// Prompt identifiers that define one text-model request.
#[derive(Clone, Copy, Debug)]
pub struct PromptPair {
    /// System-message prompt identifier.
    pub system: PromptId,
    /// User-message prompt identifier.
    pub user: PromptId,
    /// User-message prompt sent after tool exhaustion.
    pub tool_exhausted: PromptId,
}
