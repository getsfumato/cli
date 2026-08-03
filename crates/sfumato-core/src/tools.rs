//! Agent tool contracts used by generation workflows.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use crate::{
    config::{ProjectSecurityConfig, SpeechModelOptions, VideoModelOptions},
    errors::{SfumatoError, SfumatoResult},
    prompts::{PromptCatalog, PromptProvenance},
    providers::{
        ImageGenerationProvider, SpeechGenerationProvider, ToolDefinition, ToolExecutor,
        VideoGenerationProvider,
    },
    python::PythonRuntime,
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
    /// Relative artifact directory returned to the text model.
    pub reference_prefix: String,
    /// Resolved project theme used to style image prompts.
    pub theme: ThemePackage,
    /// Optional project-local instructions.
    pub project_instructions: Option<String>,
}

/// Optional direct video-generation stage exposed only to page drafters.
pub struct VideoToolConfig {
    /// Remote video provider selected for the project.
    pub provider: Arc<dyn VideoGenerationProvider>,
    /// Human-readable model profile name used in results.
    pub profile_name: String,
    /// Transaction staging directory for generated videos.
    pub output_dir: PathBuf,
    /// Relative artifact directory returned to the page drafter.
    pub reference_prefix: String,
    /// Resolved project theme used to style the video prompt.
    pub theme: ThemePackage,
    /// Optional project-local instructions.
    pub project_instructions: Option<String>,
    /// Local reusable image artifacts passed when the model supports references.
    pub references: Vec<PathBuf>,
    /// Typed video defaults from the selected profile.
    pub options: VideoModelOptions,
}

/// Optional speech stage exposed to a text model as a tool.
///
/// Deliberately carries no theme: a voice is not styled by a palette, and the
/// only direction a synthesiser accepts is the profile's own voice settings.
pub struct AudioToolConfig {
    /// Speech model selected for the project.
    pub provider: Arc<dyn SpeechGenerationProvider>,
    /// Human-readable model profile name used in results.
    pub profile_name: String,
    /// Transaction staging directory for generated audio.
    pub output_dir: PathBuf,
    /// Relative artifact directory returned to the model.
    pub reference_prefix: String,
    /// Typed speech defaults from the selected profile.
    pub options: SpeechModelOptions,
}

/// Optional local charting stage exposed to a text model as a tool.
///
/// Unlike the other stages this one has no provider: the drafting model writes
/// the plotting code itself and Sfumato runs it in a managed environment, so a
/// chart is reproducible from the code that made it and costs no remote call.
pub struct ChartToolConfig {
    /// Managed Python environments used to run the generated plotting code.
    pub python: Arc<dyn PythonRuntime>,
    /// Transaction staging directory for rendered charts.
    pub output_dir: PathBuf,
    /// Relative artifact directory returned to the text model.
    pub reference_prefix: String,
    /// Resolved project theme, applied to the figure before the code runs.
    pub theme: ThemePackage,
    /// Optional project-local instructions.
    pub project_instructions: Option<String>,
    /// Project trust decisions gating extra requirements.
    pub security: ProjectSecurityConfig,
}

impl ChartToolConfig {
    /// Builds the charting stage when the project both enables and permits it.
    ///
    /// Both conditions are checked here rather than at each call site: enabling
    /// the tool is a preference, consenting to run generated Python is a trust
    /// decision, and a workflow that remembered only one of them would either
    /// execute code without permission or silently drop a tool the project asked
    /// for. Returning `None` leaves the tool off the model's list entirely, which
    /// is the honest signal that it is unavailable.
    ///
    /// `code_execution_approved` carries a one-time per-run consent such as
    /// `--allow-code-execution`. It is accepted alongside the persisted
    /// `security.allow_python` because they say the same thing with different
    /// lifetimes, and reading only the persisted one made this tool unreachable
    /// for anyone who consented per run — silently, since a tool that is never
    /// offered produces no error. Callers that have no per-run consent to offer
    /// pass `false`; see [`chart_tool_gate_warning`] for what the caller then
    /// tells the user.
    pub fn enable(
        config: &crate::config::EffectiveConfig,
        python: Arc<dyn PythonRuntime>,
        output_dir: PathBuf,
        reference_prefix: &str,
        theme: &ThemePackage,
        project_instructions: Option<String>,
        code_execution_approved: bool,
    ) -> Option<Self> {
        if !chart_tool_requested(config) || !python_permitted(config, code_execution_approved) {
            return None;
        }
        Some(Self {
            python,
            output_dir,
            reference_prefix: reference_prefix.to_string(),
            theme: theme.clone(),
            project_instructions,
            security: config.security.clone(),
        })
    }
}

/// Whether the project asked for the charting tool at all.
fn chart_tool_requested(config: &crate::config::EffectiveConfig) -> bool {
    config.generation_tool_enabled(crate::config::GenerationToolKind::ChartGen)
}

/// Whether running generated Python is permitted, persistently or for this run.
fn python_permitted(
    config: &crate::config::EffectiveConfig,
    code_execution_approved: bool,
) -> bool {
    config.security.allow_python || code_execution_approved
}

/// Explains a charting tool that was enabled but withheld from the model.
///
/// Returning `None` from [`ChartToolConfig::enable`] is right — an unavailable
/// tool must not be offered — but on its own it is invisible: the run finishes
/// without charts and with nothing said, and there is no way to find out why.
/// Workflows push this into the warnings they already surface and record.
pub fn chart_tool_gate_warning(
    config: &crate::config::EffectiveConfig,
    code_execution_approved: bool,
) -> Option<String> {
    (chart_tool_requested(config) && !python_permitted(config, code_execution_approved)).then(|| {
        "The chart-gen tool is enabled but was not offered to the model: it runs generated Python. Pass --allow-code-execution for this run, or set security.allow_python in the project configuration.".to_string()
    })
}

/// Inputs required to construct one operation-scoped tool set.
pub struct GenerationToolsRequest {
    /// Project working directory allowed to filesystem tools.
    pub project_root: PathBuf,
    /// Explicit source paths whose roots are also readable.
    pub sources: Vec<PathBuf>,
    /// Optional image-generation tool configuration.
    pub image: Option<ImageToolConfig>,
    /// Optional page-only generated-video tool.
    pub video: Option<VideoToolConfig>,
    /// Optional speech-synthesis tool.
    pub audio: Option<AudioToolConfig>,
    /// Optional local charting tool.
    pub chart: Option<ChartToolConfig>,
    /// Catalog used for model-visible tool descriptions and image prompts.
    pub prompt_catalog: Arc<dyn PromptCatalog>,
}

/// Builds operation-scoped tools without exposing infrastructure to workflows.
pub trait GenerationToolFactory: Send + Sync {
    /// Creates validated tool definitions and their sandboxed executor.
    fn create(&self, request: GenerationToolsRequest) -> SfumatoResult<ToolSet>;
}

#[cfg(test)]
#[path = "../tests/unit/tools.rs"]
mod tests;
