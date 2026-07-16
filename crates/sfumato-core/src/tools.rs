//! Agent tool contracts used by generation workflows.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use crate::{
    errors::{SfumatoError, SfumatoResult},
    prompts::{PromptCatalog, PromptProvenance},
    providers::{ImageGenerationProvider, ToolDefinition, ToolExecutor},
    themes::ThemePackage,
};

/// A set of model-visible tool definitions and their executor.
#[derive(Clone)]
pub struct ToolSet {
    /// JSON-schema tool definitions sent to the text model.
    pub definitions: Vec<ToolDefinition>,
    /// Executes tool calls selected by the model.
    pub executor: Arc<dyn ToolExecutor>,
    /// Registry populated by tools that create artifacts.
    pub artifacts: Arc<Mutex<Vec<PathBuf>>>,
    /// Prompt provenance populated by prompt-backed tools.
    pub prompts: Arc<Mutex<Vec<PromptProvenance>>>,
}

impl ToolSet {
    /// Returns paths created by tools during this operation.
    pub fn generated_artifacts(&self) -> SfumatoResult<Vec<PathBuf>> {
        self.artifacts
            .lock()
            .map(|artifacts| artifacts.clone())
            .map_err(|_| SfumatoError::internal("Generated artifact registry is unavailable"))
    }

    /// Returns prompt provenance recorded by prompt-backed tools.
    pub fn generated_prompts(&self) -> SfumatoResult<Vec<PromptProvenance>> {
        self.prompts
            .lock()
            .map(|prompts| prompts.clone())
            .map_err(|_| SfumatoError::internal("Generated prompt registry is unavailable"))
    }
}

/// Optional image-generation stage exposed to a text model as a tool.
pub struct ImageToolConfig {
    /// Image model selected for the project.
    pub provider: Arc<dyn ImageGenerationProvider>,
    /// Human-readable model profile name used in results.
    pub profile_name: String,
    /// Transaction staging directory for generated images.
    pub output_dir: PathBuf,
    /// Resolved project theme used to style image prompts.
    pub theme: ThemePackage,
    /// Optional project-local instructions.
    pub project_instructions: Option<String>,
}

/// Inputs required to construct one operation-scoped tool set.
pub struct GenerationToolsRequest {
    /// Project working directory allowed to filesystem tools.
    pub project_root: PathBuf,
    /// Explicit source paths whose roots are also readable.
    pub sources: Vec<PathBuf>,
    /// Optional image-generation tool configuration.
    pub image: Option<ImageToolConfig>,
    /// Catalog used for model-visible tool descriptions and image prompts.
    pub prompt_catalog: Arc<dyn PromptCatalog>,
}

/// Builds operation-scoped tools without exposing infrastructure to workflows.
pub trait GenerationToolFactory: Send + Sync {
    /// Creates validated tool definitions and their sandboxed executor.
    fn create(&self, request: GenerationToolsRequest) -> SfumatoResult<ToolSet>;
}
