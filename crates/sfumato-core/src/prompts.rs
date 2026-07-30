//! Provider-neutral prompt contracts.
//!
//! Workflows select stable prompt identifiers and supply structured values.
//! Loading, overriding, and rendering template text belongs to an adapter.

use std::{
    fmt,
    path::{Path, PathBuf},
    str::FromStr,
};

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
    /// System prompt for repairing an invalid Marp draft.
    SlidesValidationRepairSystem,
    /// User prompt for repairing an invalid Marp draft.
    SlidesValidationRepairUser,
    /// System prompt for semantic review.
    SlidesReviewSystem,
    /// User prompt for semantic review.
    SlidesReviewUser,
    /// System prompt for compact semantic review.
    SlidesCompactReviewSystem,
    /// User prompt for compact semantic review.
    SlidesCompactReviewUser,
    /// System prompt for focused Mermaid syntax repair.
    SlidesMermaidRepairSystem,
    /// User prompt for focused Mermaid syntax repair.
    SlidesMermaidRepairUser,
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
    /// User prompt sent to a page-embedded video-generation model.
    VideoGenerationUser,
    /// User prompt that reconstructs one logical artifact for a new theme.
    ProjectAssetRegenerationUser,
    /// Model-facing descriptions for the generation tool registry.
    ToolsGenerationDescriptions,
    /// System prompt for a standalone page draft.
    PageDraftSystem,
    /// User prompt for a standalone page draft.
    PageDraftUser,
    /// System prompt for a compact standalone page draft.
    PageCompactDraftSystem,
    /// User prompt for a compact standalone page draft.
    PageCompactDraftUser,
    /// System prompt for repairing invalid page fragments.
    PageValidationRepairSystem,
    /// User prompt for repairing invalid page fragments.
    PageValidationRepairUser,
    /// System prompt for semantic page review.
    PageReviewSystem,
    /// User prompt for semantic page review.
    PageReviewUser,
    /// System prompt for browser-detected page repair.
    PageBrowserRepairSystem,
    /// User prompt for browser-detected page repair.
    PageBrowserRepairUser,
    /// Final user message when page tools are exhausted.
    PageToolExhaustedUser,
    /// System prompt for an engine-neutral video plan.
    VideoPlanSystem,
    /// User prompt for an engine-neutral video plan.
    VideoPlanUser,
    /// System prompt for semantic video-plan review.
    VideoReviewSystem,
    /// User prompt for semantic video-plan review.
    VideoReviewUser,
    /// System prompt for authoring a Hyperframe project.
    VideoHyperframeSystem,
    /// User prompt for authoring a Hyperframe project.
    VideoHyperframeUser,
    /// System prompt for authoring a Manim scene.
    VideoManimSystem,
    /// User prompt for authoring a Manim scene.
    VideoManimUser,
    /// System prompt for focused local renderer source repair.
    VideoSourceRepairSystem,
    /// User prompt for focused local renderer source repair.
    VideoSourceRepairUser,
    /// Final user message when video planning tools are exhausted.
    VideoToolExhaustedUser,
    /// System prompt for a full document draft.
    DocumentDraftSystem,
    /// User prompt for a full document draft.
    DocumentDraftUser,
    /// System prompt for a compact document draft retry.
    DocumentCompactDraftSystem,
    /// User prompt for a compact document draft retry.
    DocumentCompactDraftUser,
    /// System prompt for repairing invalid document structure.
    DocumentValidationRepairSystem,
    /// User prompt for repairing invalid document structure.
    DocumentValidationRepairUser,
    /// System prompt for repairing invalid document Mermaid source.
    DocumentMermaidRepairSystem,
    /// User prompt for repairing invalid document Mermaid source.
    DocumentMermaidRepairUser,
    /// System prompt for semantic document review.
    DocumentReviewSystem,
    /// User prompt for semantic document review.
    DocumentReviewUser,
    /// System prompt for compacted semantic document review.
    DocumentCompactReviewSystem,
    /// User prompt for compacted semantic document review.
    DocumentCompactReviewUser,
    /// System prompt for focused page-format repair.
    DocumentFormatRepairSystem,
    /// User prompt for focused page-format repair.
    DocumentFormatRepairUser,
    /// Final user message when document tools are exhausted.
    DocumentToolExhaustedUser,
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
            Self::SlidesValidationRepairSystem,
            Self::SlidesValidationRepairUser,
            Self::SlidesReviewSystem,
            Self::SlidesReviewUser,
            Self::SlidesCompactReviewSystem,
            Self::SlidesCompactReviewUser,
            Self::SlidesMermaidRepairSystem,
            Self::SlidesMermaidRepairUser,
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
            Self::VideoGenerationUser,
            Self::ProjectAssetRegenerationUser,
            Self::ToolsGenerationDescriptions,
            Self::PageDraftSystem,
            Self::PageDraftUser,
            Self::PageCompactDraftSystem,
            Self::PageCompactDraftUser,
            Self::PageValidationRepairSystem,
            Self::PageValidationRepairUser,
            Self::PageReviewSystem,
            Self::PageReviewUser,
            Self::PageBrowserRepairSystem,
            Self::PageBrowserRepairUser,
            Self::PageToolExhaustedUser,
            Self::VideoPlanSystem,
            Self::VideoPlanUser,
            Self::VideoReviewSystem,
            Self::VideoReviewUser,
            Self::VideoHyperframeSystem,
            Self::VideoHyperframeUser,
            Self::VideoManimSystem,
            Self::VideoManimUser,
            Self::VideoSourceRepairSystem,
            Self::VideoSourceRepairUser,
            Self::VideoToolExhaustedUser,
            Self::DocumentDraftSystem,
            Self::DocumentDraftUser,
            Self::DocumentCompactDraftSystem,
            Self::DocumentCompactDraftUser,
            Self::DocumentValidationRepairSystem,
            Self::DocumentValidationRepairUser,
            Self::DocumentMermaidRepairSystem,
            Self::DocumentMermaidRepairUser,
            Self::DocumentReviewSystem,
            Self::DocumentReviewUser,
            Self::DocumentCompactReviewSystem,
            Self::DocumentCompactReviewUser,
            Self::DocumentFormatRepairSystem,
            Self::DocumentFormatRepairUser,
            Self::DocumentToolExhaustedUser,
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
            Self::SlidesValidationRepairSystem => "slides.validation-repair.system",
            Self::SlidesValidationRepairUser => "slides.validation-repair.user",
            Self::SlidesReviewSystem => "slides.review.system",
            Self::SlidesReviewUser => "slides.review.user",
            Self::SlidesCompactReviewSystem => "slides.compact-review.system",
            Self::SlidesCompactReviewUser => "slides.compact-review.user",
            Self::SlidesMermaidRepairSystem => "slides.mermaid-repair.system",
            Self::SlidesMermaidRepairUser => "slides.mermaid-repair.user",
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
            Self::VideoGenerationUser => "video.generation.user",
            Self::ProjectAssetRegenerationUser => "artifact.regeneration.user",
            Self::ToolsGenerationDescriptions => "tools.generation.descriptions",
            Self::PageDraftSystem => "page.draft.system",
            Self::PageDraftUser => "page.draft.user",
            Self::PageCompactDraftSystem => "page.compact-draft.system",
            Self::PageCompactDraftUser => "page.compact-draft.user",
            Self::PageValidationRepairSystem => "page.validation-repair.system",
            Self::PageValidationRepairUser => "page.validation-repair.user",
            Self::PageReviewSystem => "page.review.system",
            Self::PageReviewUser => "page.review.user",
            Self::PageBrowserRepairSystem => "page.browser-repair.system",
            Self::PageBrowserRepairUser => "page.browser-repair.user",
            Self::PageToolExhaustedUser => "page.tool-exhausted.user",
            Self::VideoPlanSystem => "video.plan.system",
            Self::VideoPlanUser => "video.plan.user",
            Self::VideoReviewSystem => "video.review.system",
            Self::VideoReviewUser => "video.review.user",
            Self::VideoHyperframeSystem => "video.hyperframe.system",
            Self::VideoHyperframeUser => "video.hyperframe.user",
            Self::VideoManimSystem => "video.manim.system",
            Self::VideoManimUser => "video.manim.user",
            Self::VideoSourceRepairSystem => "video.source-repair.system",
            Self::VideoSourceRepairUser => "video.source-repair.user",
            Self::VideoToolExhaustedUser => "video.tool-exhausted.user",
            Self::DocumentDraftSystem => "document.draft.system",
            Self::DocumentDraftUser => "document.draft.user",
            Self::DocumentCompactDraftSystem => "document.compact-draft.system",
            Self::DocumentCompactDraftUser => "document.compact-draft.user",
            Self::DocumentValidationRepairSystem => "document.validation-repair.system",
            Self::DocumentValidationRepairUser => "document.validation-repair.user",
            Self::DocumentMermaidRepairSystem => "document.mermaid-repair.system",
            Self::DocumentMermaidRepairUser => "document.mermaid-repair.user",
            Self::DocumentReviewSystem => "document.review.system",
            Self::DocumentReviewUser => "document.review.user",
            Self::DocumentCompactReviewSystem => "document.compact-review.system",
            Self::DocumentCompactReviewUser => "document.compact-review.user",
            Self::DocumentFormatRepairSystem => "document.format-repair.system",
            Self::DocumentFormatRepairUser => "document.format-repair.user",
            Self::DocumentToolExhaustedUser => "document.tool-exhausted.user",
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

/// Scope into which a bundled prompt template is customized.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PromptOverrideScope {
    /// User-global override.
    User,
    /// Selected-project override.
    Project,
}

/// Prompt metadata and resolved provenance shown by management frontends.
#[derive(Clone, Debug)]
pub struct PromptTemplateSummary {
    /// Stable identifier.
    pub id: PromptId,
    /// Relative bundled template path.
    pub path: PathBuf,
    /// Variables required by the template manifest.
    pub required: Vec<String>,
    /// Layer selected for this project.
    pub provenance: PromptProvenance,
}

/// Unrendered prompt source selected by layered resolution.
#[derive(Clone, Debug)]
pub struct PromptTemplateSource {
    /// Stable identifier.
    pub id: PromptId,
    /// Resolved template text.
    pub text: String,
    /// Layer and content hash of the source.
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

/// Port for listing, inspecting, validating, and customizing prompt templates.
pub trait PromptManager: Send + Sync {
    /// Lists all templates as resolved for one project root.
    fn list(&self, project_root: &Path) -> Result<Vec<PromptTemplateSummary>, PromptError>;

    /// Loads one unrendered resolved template.
    fn source(
        &self,
        project_root: &Path,
        id: PromptId,
    ) -> Result<PromptTemplateSource, PromptError>;

    /// Copies the bundled template into a user or project override layer.
    fn customize(
        &self,
        project_root: &Path,
        id: PromptId,
        scope: PromptOverrideScope,
    ) -> Result<PathBuf, PromptError>;

    /// Validates every template resolved for one project root.
    fn validate(&self, project_root: &Path) -> Result<Vec<PromptProvenance>, PromptError>;
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
