use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    str::FromStr,
};

use serde::{Deserialize, Serialize};
pub use sfumato_domain::{Capability, SecretRef};

use crate::{
    errors::{ResultContext as Context, SfumatoError, SfumatoResult as Result},
    sfumato_bail as bail,
};

#[derive(Clone, Debug, Default)]
pub struct ConfigOverrides {
    pub project: Option<String>,
    pub theme: Option<String>,
    pub model_overrides: BTreeMap<Capability, String>,
    pub reviewer_model: Option<String>,
    pub publish_dir: Option<PathBuf>,
    pub pdf: bool,
    pub tool_overrides: BTreeMap<GenerationToolKind, bool>,
}

#[derive(Clone, Debug, Serialize)]
pub struct GlobalConfig {
    pub user: UserConfig,
    pub connectors: BTreeMap<String, ConnectorConfig>,
    pub models: BTreeMap<String, ModelProfile>,
    pub defaults: ModelDefaults,
    #[serde(default)]
    pub model_roles: BTreeMap<ModelRole, String>,
    pub marp: MarpConfig,
}

#[derive(Clone, Debug, Serialize)]
pub struct UserConfig {
    pub name: Option<String>,
    pub learning_style: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ProjectRegistry {
    pub active: Option<String>,
    pub projects: BTreeMap<String, RegisteredProject>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RegisteredProject {
    pub path: PathBuf,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectConfig {
    pub name: String,
    pub theme: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publish_dir: Option<PathBuf>,
    #[serde(default)]
    pub model_defaults: BTreeMap<Capability, String>,
    #[serde(default)]
    pub model_roles: BTreeMap<ModelRole, String>,
    /// Page-only visual extensions.
    #[serde(default)]
    pub page: PageDefaults,
    /// Model-backed tools made available to resource drafters.
    #[serde(default)]
    pub generation_tools: GenerationToolDefaults,
    /// Explicit trust decisions for local generated-code renderers.
    #[serde(default)]
    pub security: ProjectSecurityConfig,
    #[serde(default)]
    pub marp: Option<MarpConfig>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ModelProfile {
    pub connector: String,
    pub model: String,
    pub capabilities: Vec<Capability>,
    #[serde(default)]
    pub options: ModelOptions,
}

/// Capability-specific options for a model profile.
#[derive(Clone, Debug, Default, Serialize)]
pub struct ModelOptions {
    pub text: TextModelOptions,
    pub image: ImageModelOptions,
    pub video: VideoModelOptions,
}

/// Options used by text and code generation.
#[derive(Clone, Debug, Default, Serialize)]
pub struct TextModelOptions {
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub max_tool_rounds: Option<usize>,
    pub top_p: Option<f32>,
    pub seed: Option<i64>,
}

/// Options used by image generation.
#[derive(Clone, Debug, Default, Serialize)]
pub struct ImageModelOptions {
    pub quality: Option<String>,
    pub background: Option<String>,
    pub size: Option<String>,
    pub aspect_ratio: Option<String>,
    pub output_format: Option<String>,
}

/// Options used by asynchronous video-generation models.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct VideoModelOptions {
    /// Default duration used by the page video-generation tool.
    pub duration_seconds: Option<u32>,
    /// Provider resolution such as `720p` or `1080p`.
    pub resolution: Option<String>,
    /// Provider aspect ratio such as `16:9`.
    pub aspect_ratio: Option<String>,
    /// Native audio preference.
    pub audio: Option<VideoAudioMode>,
    /// Optional deterministic seed.
    pub seed: Option<i64>,
    /// Poll interval for asynchronous providers.
    pub poll_interval_seconds: Option<u64>,
    /// Maximum provider wait time.
    pub timeout_seconds: Option<u64>,
}

/// Native audio policy for remote video models.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VideoAudioMode {
    /// Let the provider use its model default.
    Auto,
    /// Request native generated audio.
    On,
    /// Request a silent clip.
    Off,
}

impl FromStr for VideoAudioMode {
    type Err = SfumatoError;

    fn from_str(value: &str) -> Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "on" | "true" => Ok(Self::On),
            "off" | "false" => Ok(Self::Off),
            _ => bail!("Unknown video audio mode '{value}'. Use auto, on, or off."),
        }
    }
}

impl std::fmt::Display for VideoAudioMode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Auto => "auto",
            Self::On => "on",
            Self::Off => "off",
        })
    }
}

/// Page-specific project defaults.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct PageDefaults {
    /// At most one component-library plugin selected for pages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<String>,
    /// Composable utility plugins such as Motion or Three.js.
    #[serde(default)]
    pub plugins: Vec<String>,
}

/// A model-backed tool that a generation workflow may expose.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum GenerationToolKind {
    /// Generate an image and return a local artifact path.
    ImageGen,
    /// Generate a video and return a local artifact path.
    VideoGen,
}

impl GenerationToolKind {
    /// Stable CLI and configuration identifier.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ImageGen => "image-gen",
            Self::VideoGen => "video-gen",
        }
    }

    /// Model capability required by the tool.
    pub const fn capability(self) -> Capability {
        match self {
            Self::ImageGen => Capability::Image,
            Self::VideoGen => Capability::Video,
        }
    }
}

impl FromStr for GenerationToolKind {
    type Err = SfumatoError;

    fn from_str(value: &str) -> Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "image-gen" | "image_gen" => Ok(Self::ImageGen),
            "video-gen" | "video_gen" => Ok(Self::VideoGen),
            _ => bail!("Unknown generation tool '{value}'. Use image-gen or video-gen."),
        }
    }
}

/// Explicit project defaults for model-backed generation tools.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(transparent)]
pub struct GenerationToolDefaults(pub BTreeMap<GenerationToolKind, bool>);

/// Project trust settings for generated-code renderers.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ProjectSecurityConfig {
    /// Whether generated Manim Python may execute without a command override.
    #[serde(default)]
    pub allow_manim: bool,
}

impl ModelOptions {
    pub fn text_temperature(&self) -> f32 {
        self.text.temperature.unwrap_or(0.4)
    }

    pub fn text_max_tokens(&self) -> u32 {
        self.text.max_tokens.unwrap_or(4000)
    }

    pub fn tool_rounds(&self) -> usize {
        self.text
            .max_tool_rounds
            .filter(|rounds| *rounds > 0)
            .unwrap_or(8)
    }

    pub fn merge(&mut self, changes: Self) {
        macro_rules! replace_some {
            ($($field:ident),+ $(,)?) => {
                $(if changes.text.$field.is_some() { self.text.$field = changes.text.$field; })+
            };
        }
        replace_some!(temperature, max_tokens, max_tool_rounds, top_p, seed,);
        macro_rules! replace_image_some {
            ($($field:ident),+ $(,)?) => {
                $(if changes.image.$field.is_some() { self.image.$field = changes.image.$field; })+
            };
        }
        replace_image_some!(quality, background, size, aspect_ratio, output_format,);
        macro_rules! replace_video_some {
            ($($field:ident),+ $(,)?) => {
                $(if changes.video.$field.is_some() { self.video.$field = changes.video.$field; })+
            };
        }
        replace_video_some!(
            duration_seconds,
            resolution,
            aspect_ratio,
            audio,
            seed,
            poll_interval_seconds,
            timeout_seconds,
        );
    }

    pub fn cli_pairs(&self) -> Vec<String> {
        let mut pairs = Vec::new();
        macro_rules! push_option {
            ($group:ident, $field:ident) => {
                if let Some(value) = &self.$group.$field {
                    pairs.push(format!("{}={value}", stringify!($field)));
                }
            };
        }
        push_option!(text, temperature);
        push_option!(text, max_tokens);
        push_option!(text, max_tool_rounds);
        push_option!(text, top_p);
        push_option!(text, seed);
        push_option!(image, quality);
        push_option!(image, background);
        push_option!(image, size);
        push_option!(image, aspect_ratio);
        push_option!(image, output_format);
        push_option!(video, duration_seconds);
        push_option!(video, resolution);
        push_option!(video, aspect_ratio);
        push_option!(video, audio);
        push_option!(video, seed);
        push_option!(video, poll_interval_seconds);
        push_option!(video, timeout_seconds);
        pairs
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ModelDefaults(pub BTreeMap<Capability, String>);

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum ModelRole {
    Reviewer,
}

impl ModelRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reviewer => "reviewer",
        }
    }

    pub fn required_capability(self) -> Capability {
        match self {
            Self::Reviewer => Capability::Text,
        }
    }
}

impl FromStr for ModelRole {
    type Err = SfumatoError;

    fn from_str(value: &str) -> Result<Self> {
        match value.to_lowercase().as_str() {
            "reviewer" => Ok(Self::Reviewer),
            _ => bail!("Unknown model role '{value}'. Use reviewer."),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct OpenAiCompatibleConnectorConfig {
    pub base_url: String,
    pub credential: Option<SecretRef>,
    pub headers: BTreeMap<String, String>,
}

/// OpenRouter connector composed from its OpenAI-compatible transport.
#[derive(Clone, Debug, Serialize)]
pub struct OpenRouterConnectorConfig {
    /// Shared chat/image transport configuration.
    pub transport: OpenAiCompatibleConnectorConfig,
}

/// Ollama connector composed from its OpenAI-compatible transport and native API root.
#[derive(Clone, Debug, Serialize)]
pub struct OllamaConnectorConfig {
    /// Shared chat transport configuration.
    pub transport: OpenAiCompatibleConnectorConfig,
    /// Native Ollama API root, normally without `/v1`.
    pub native_base_url: String,
}

/// LM Studio connector composed from its OpenAI-compatible transport and native REST root.
#[derive(Clone, Debug, Serialize)]
pub struct LmStudioConnectorConfig {
    /// Shared chat transport configuration, normally the `/v1` base.
    pub transport: OpenAiCompatibleConnectorConfig,
    /// Native LM Studio REST root, normally without `/v1`.
    pub native_base_url: String,
}

/// Native Anthropic Messages API connector.
///
/// Deliberately not composed from [`OpenAiCompatibleConnectorConfig`]: the
/// Messages API is a different wire format and authenticates with `x-api-key`
/// rather than bearer auth, so typing it as an OpenAI-compatible transport would
/// let a future refactor route it through the wrong provider factory.
#[derive(Clone, Debug, Serialize)]
pub struct AnthropicConnectorConfig {
    /// API root, normally `https://api.anthropic.com/v1`.
    pub base_url: String,
    /// Indirect reference to the `x-api-key` credential.
    pub credential: Option<SecretRef>,
    /// Extra request headers; may override the pinned `anthropic-version`.
    pub headers: BTreeMap<String, String>,
}

/// Configuration for one external model transport.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConnectorConfig {
    /// HTTP connector exposing OpenAI-compatible endpoints.
    OpenAiCompatible(OpenAiCompatibleConnectorConfig),
    /// OpenRouter with provider-native catalog and account operations.
    OpenRouter(OpenRouterConnectorConfig),
    /// Ollama with provider-native local model operations.
    Ollama(OllamaConnectorConfig),
    /// LM Studio with provider-native local model and runtime operations.
    LmStudio(LmStudioConnectorConfig),
    /// Anthropic speaking the native Messages API.
    Anthropic(AnthropicConnectorConfig),
    /// Local Codex App Server using Codex-owned ChatGPT authentication.
    CodexAppServer(CodexAppServerConnectorConfig),
}

/// How Sfumato participates in one connector's authentication.
///
/// Connector kinds differ in whether Sfumato owns the credential or an external
/// tool does. Matching on this instead of on a specific kind keeps
/// authentication a question about capability, so a new connector kind cannot
/// silently fall into the wrong branch.
#[derive(Clone, Copy, Debug)]
pub enum ConnectorAuth<'a> {
    /// Sfumato owns this credential reference and may replace it.
    Managed(Option<&'a SecretRef>),
    /// The connector's own tooling owns authentication.
    External {
        /// Tool that owns the credential, named in guidance messages.
        owner: &'static str,
        /// Command that establishes the credential.
        login_command: &'static str,
        /// Command that clears the credential.
        logout_command: &'static str,
    },
}

impl ConnectorConfig {
    /// Stable connector kind used by presentation and persistence adapters.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::OpenAiCompatible(_) => "openai_compatible",
            Self::OpenRouter(_) => "openrouter",
            Self::Ollama(_) => "ollama",
            Self::LmStudio(_) => "lmstudio",
            Self::Anthropic(_) => "anthropic",
            Self::CodexAppServer(_) => "codex_app_server",
        }
    }

    /// Human-readable endpoint or executable target.
    pub fn target(&self) -> String {
        match self {
            Self::OpenAiCompatible(config) => config.base_url.clone(),
            Self::OpenRouter(config) => config.transport.base_url.clone(),
            Self::Ollama(config) => config.native_base_url.clone(),
            Self::LmStudio(config) => config.native_base_url.clone(),
            Self::Anthropic(config) => config.base_url.clone(),
            Self::CodexAppServer(config) => config.executable.display().to_string(),
        }
    }

    /// Returns the OpenAI-compatible settings when this connector uses HTTP.
    pub fn openai_compatible(&self) -> Option<&OpenAiCompatibleConnectorConfig> {
        match self {
            Self::OpenAiCompatible(config) => Some(config),
            Self::OpenRouter(config) => Some(&config.transport),
            Self::Ollama(config) => Some(&config.transport),
            Self::LmStudio(config) => Some(&config.transport),
            // The Messages API is not OpenAI-compatible.
            Self::Anthropic(_) => None,
            Self::CodexAppServer(_) => None,
        }
    }

    /// Returns mutable OpenAI-compatible settings when this connector uses HTTP.
    pub fn openai_compatible_mut(&mut self) -> Option<&mut OpenAiCompatibleConnectorConfig> {
        match self {
            Self::OpenAiCompatible(config) => Some(config),
            Self::OpenRouter(config) => Some(&mut config.transport),
            Self::Ollama(config) => Some(&mut config.transport),
            Self::LmStudio(config) => Some(&mut config.transport),
            Self::Anthropic(_) => None,
            Self::CodexAppServer(_) => None,
        }
    }

    /// Reports whether Sfumato manages this connector's credential.
    ///
    /// Deliberately exhaustive with no wildcard arm: a new connector kind must
    /// declare its authentication ownership at compile time rather than fall
    /// through to a default that would be wrong for it.
    pub fn auth(&self) -> ConnectorAuth<'_> {
        match self {
            Self::OpenAiCompatible(config) => ConnectorAuth::Managed(config.credential.as_ref()),
            Self::OpenRouter(config) => {
                ConnectorAuth::Managed(config.transport.credential.as_ref())
            }
            Self::Ollama(config) => ConnectorAuth::Managed(config.transport.credential.as_ref()),
            Self::LmStudio(config) => ConnectorAuth::Managed(config.transport.credential.as_ref()),
            Self::Anthropic(config) => ConnectorAuth::Managed(config.credential.as_ref()),
            Self::CodexAppServer(_) => ConnectorAuth::External {
                owner: "Codex CLI",
                login_command: "codex login",
                logout_command: "codex logout",
            },
        }
    }

    /// Replaces the managed credential reference for this connector.
    ///
    /// Returns a typed error for connectors whose authentication is owned by an
    /// external tool. Callers that can offer kind-specific guidance should
    /// inspect [`ConnectorConfig::auth`] first.
    pub fn set_managed_credential(&mut self, reference: Option<SecretRef>) -> Result<()> {
        match self {
            Self::OpenAiCompatible(config) => config.credential = reference,
            Self::OpenRouter(config) => config.transport.credential = reference,
            Self::Ollama(config) => config.transport.credential = reference,
            Self::LmStudio(config) => config.transport.credential = reference,
            Self::Anthropic(config) => config.credential = reference,
            Self::CodexAppServer(_) => {
                bail!("Codex App Server connectors manage their own authentication")
            }
        }
        Ok(())
    }
}

/// Process settings for a local Codex App Server connector.
#[derive(Clone, Debug, Serialize)]
pub struct CodexAppServerConnectorConfig {
    /// Executable name or path. Authentication remains owned by Codex.
    pub executable: PathBuf,
}

#[derive(Clone, Debug, Serialize)]
pub struct MarpConfig {
    pub pdf: bool,
    #[serde(default)]
    pub browser_path: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize)]
pub struct EffectiveConfig {
    pub user: UserConfig,
    pub project_name: String,
    pub project_root: PathBuf,
    pub publish_dir: Option<PathBuf>,
    pub theme: String,
    pub connectors: BTreeMap<String, ConnectorConfig>,
    pub models: BTreeMap<String, ModelProfile>,
    pub model_defaults: BTreeMap<Capability, String>,
    pub model_roles: BTreeMap<ModelRole, String>,
    /// Page-only visual extension defaults.
    pub page: PageDefaults,
    /// Effective model-backed generation-tool policy.
    pub generation_tools: GenerationToolDefaults,
    /// Project trust decisions for generated code.
    pub security: ProjectSecurityConfig,
    pub marp: MarpConfig,
}

impl GlobalConfig {
    pub fn default_config() -> Self {
        let mut models = BTreeMap::new();
        models.insert(
            "local-text".to_string(),
            ModelProfile {
                connector: "ollama".to_string(),
                model: "llama3.2".to_string(),
                capabilities: vec![Capability::Text, Capability::Code],
                options: ModelOptions {
                    text: TextModelOptions {
                        temperature: Some(0.4),
                        max_tokens: Some(4000),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            },
        );
        models.insert(
            "cloud-text".to_string(),
            ModelProfile {
                connector: "openrouter".to_string(),
                model: "openai/gpt-4o-mini".to_string(),
                capabilities: vec![Capability::Text, Capability::Code],
                options: ModelOptions {
                    text: TextModelOptions {
                        temperature: Some(0.4),
                        max_tokens: Some(4000),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            },
        );

        Self {
            user: UserConfig {
                name: None,
                learning_style: vec!["visual".to_string(), "step-by-step".to_string()],
            },
            connectors: BTreeMap::from([
                (
                    "ollama".to_string(),
                    ConnectorConfig::Ollama(OllamaConnectorConfig {
                        transport: OpenAiCompatibleConnectorConfig {
                            base_url: "http://localhost:11434/v1".to_string(),
                            credential: None,
                            headers: BTreeMap::new(),
                        },
                        native_base_url: "http://localhost:11434".to_string(),
                    }),
                ),
                (
                    "openrouter".to_string(),
                    ConnectorConfig::OpenRouter(OpenRouterConnectorConfig {
                        transport: OpenAiCompatibleConnectorConfig {
                            base_url: "https://openrouter.ai/api/v1".to_string(),
                            credential: Some(
                                SecretRef::stored("connector/openrouter")
                                    .expect("the bundled stored reference is valid"),
                            ),
                            headers: BTreeMap::new(),
                        },
                    }),
                ),
            ]),
            models,
            defaults: ModelDefaults(BTreeMap::from([(
                Capability::Text,
                "local-text".to_string(),
            )])),
            model_roles: BTreeMap::new(),
            marp: MarpConfig {
                pdf: true,
                browser_path: None,
            },
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self
            .user
            .learning_style
            .iter()
            .any(|style| style.trim().is_empty())
        {
            bail!("Learning styles cannot contain empty values");
        }
        for (name, connector) in &self.connectors {
            if name.trim().is_empty() {
                bail!("Connector names cannot be empty");
            }
            match connector {
                ConnectorConfig::OpenAiCompatible(connector)
                    if connector.base_url.trim().is_empty() =>
                {
                    bail!("OpenAI-compatible connector base URLs cannot be empty");
                }
                ConnectorConfig::OpenRouter(connector)
                    if connector.transport.base_url.trim().is_empty() =>
                {
                    bail!("OpenRouter connector base URLs cannot be empty");
                }
                ConnectorConfig::Ollama(connector)
                    if connector.transport.base_url.trim().is_empty()
                        || connector.native_base_url.trim().is_empty() =>
                {
                    bail!("Ollama connector base URLs cannot be empty");
                }
                ConnectorConfig::LmStudio(connector)
                    if connector.transport.base_url.trim().is_empty()
                        || connector.native_base_url.trim().is_empty() =>
                {
                    bail!("LM Studio connector base URLs cannot be empty");
                }
                ConnectorConfig::Anthropic(connector) if connector.base_url.trim().is_empty() => {
                    bail!("Anthropic connector base URLs cannot be empty");
                }
                ConnectorConfig::CodexAppServer(connector)
                    if connector.executable.as_os_str().is_empty() =>
                {
                    bail!("Codex App Server connector executables cannot be empty");
                }
                _ => {}
            }
        }
        for (name, profile) in &self.models {
            if !self.connectors.contains_key(&profile.connector) {
                bail!(
                    "Model profile '{name}' references unknown connector '{}'",
                    profile.connector
                );
            }
            if profile.model.trim().is_empty() || profile.capabilities.is_empty() {
                bail!("Model profile '{name}' requires a model ID and capabilities");
            }
            if profile
                .options
                .text
                .temperature
                .is_some_and(|value| !(0.0..=2.0).contains(&value))
            {
                bail!("Model profile '{name}' temperature must be between 0 and 2");
            }
            if profile
                .options
                .text
                .top_p
                .is_some_and(|value| !(0.0..=1.0).contains(&value))
            {
                bail!("Model profile '{name}' top_p must be between 0 and 1");
            }
            if profile.options.text.max_tokens == Some(0)
                || profile.options.text.max_tool_rounds == Some(0)
            {
                bail!("Model profile '{name}' token and tool limits must be positive");
            }
            if profile.options.video.duration_seconds == Some(0)
                || profile.options.video.poll_interval_seconds == Some(0)
                || profile.options.video.timeout_seconds == Some(0)
            {
                bail!("Model profile '{name}' video duration and time limits must be positive");
            }
        }
        for (capability, profile_name) in &self.defaults.0 {
            let profile = self.models.get(profile_name).with_context(|| {
                format!(
                    "Default '{}' references unknown model profile '{profile_name}'",
                    capability.as_str()
                )
            })?;
            if !profile.capabilities.contains(capability) {
                bail!(
                    "Default '{}' uses model profile '{profile_name}', which lacks that capability",
                    capability.as_str()
                );
            }
        }
        for (role, profile_name) in &self.model_roles {
            let profile = self.models.get(profile_name).with_context(|| {
                format!(
                    "Model role '{}' references unknown profile '{profile_name}'",
                    role.as_str()
                )
            })?;
            if !profile.capabilities.contains(&role.required_capability()) {
                bail!(
                    "Model role '{}' requires a text-capable profile",
                    role.as_str()
                );
            }
        }
        Ok(())
    }
}

impl ProjectConfig {
    pub fn validate(&self) -> Result<()> {
        validate_project_name(&self.name)?;
        if self.theme.trim().is_empty() {
            bail!("Project theme cannot be empty");
        }
        let mut plugins = std::collections::BTreeSet::new();
        if let Some(ui) = &self.page.ui {
            crate::page_plugins::validate_plugin_id(ui)?;
            plugins.insert(ui);
        }
        for plugin in &self.page.plugins {
            crate::page_plugins::validate_plugin_id(plugin)?;
            if !plugins.insert(plugin) {
                bail!("Project contains duplicate page plugin '{plugin}'");
            }
        }
        for tool in self.generation_tools.0.keys() {
            let _ = tool.capability();
        }
        Ok(())
    }
}

impl ProjectRegistry {
    pub fn selected(&self, requested: Option<&str>) -> Result<(String, PathBuf)> {
        let name = requested
            .map(ToOwned::to_owned)
            .or_else(|| self.active.clone())
            .context("No active project. Run `sfumato init project <name>` or `sfumato project use <name>`.")?;
        let project = self
            .projects
            .get(&name)
            .with_context(|| format!("Project '{name}' is not registered"))?;
        Ok((name, project.path.clone()))
    }
}

impl EffectiveConfig {
    pub fn from_parts(
        global: GlobalConfig,
        selected_name: String,
        project_root: PathBuf,
        project: ProjectConfig,
        overrides: ConfigOverrides,
    ) -> Result<Self> {
        if project.name != selected_name {
            bail!(
                "Registered project name '{selected_name}' does not match project config name '{}'",
                project.name
            );
        }

        let model_defaults = merge_model_defaults(
            global.defaults.0.clone(),
            project.model_defaults.clone(),
            overrides.model_overrides,
        );
        let model_roles = merge_model_roles(
            global.model_roles.clone(),
            project.model_roles.clone(),
            overrides.reviewer_model,
        );

        let publish_dir = overrides.publish_dir.or(project.publish_dir);
        let theme = resolve_theme_name(&project.theme, overrides.theme);
        let marp = project.marp.unwrap_or_else(|| global.marp.clone());
        let marp = MarpConfig {
            pdf: marp.pdf || overrides.pdf,
            browser_path: marp.browser_path,
        };

        Ok(Self {
            user: global.user,
            project_name: project.name,
            project_root,
            publish_dir,
            theme,
            connectors: global.connectors,
            models: global.models,
            model_defaults,
            model_roles,
            page: project.page,
            generation_tools: GenerationToolDefaults(merge_tool_defaults(
                project.generation_tools.0,
                overrides.tool_overrides,
            )),
            security: project.security,
            marp,
        })
    }

    pub fn resolve_model(&self, capability: Capability) -> Result<(&str, &ModelProfile)> {
        let profile_name = self.model_defaults.get(&capability).with_context(|| {
            format!(
                "No model profile configured for '{}' capability",
                capability.as_str()
            )
        })?;
        let profile = self
            .models
            .get(profile_name)
            .with_context(|| format!("Model profile '{profile_name}' was not found"))?;
        if !profile.capabilities.contains(&capability) {
            bail!(
                "Model profile '{profile_name}' does not support '{}' capability",
                capability.as_str()
            );
        }
        Ok((profile_name, profile))
    }

    pub fn resolve_model_role(&self, role: ModelRole) -> Result<(&str, &ModelProfile)> {
        let fallback;
        let profile_name = if let Some(profile_name) = self.model_roles.get(&role) {
            profile_name.as_str()
        } else {
            fallback = self.resolve_model(role.required_capability())?.0;
            fallback
        };
        let profile = self
            .models
            .get(profile_name)
            .with_context(|| format!("Model profile '{profile_name}' was not found"))?;
        if !self.connectors.contains_key(&profile.connector) {
            bail!(
                "Model profile '{profile_name}' selected for '{}' references missing connector '{}'",
                role.as_str(),
                profile.connector
            );
        }
        let required = role.required_capability();
        if !profile.capabilities.contains(&required) {
            bail!(
                "Model profile '{profile_name}' selected for '{}' does not support '{}' capability",
                role.as_str(),
                required.as_str()
            );
        }
        Ok((profile_name, profile))
    }

    pub fn publish_root(&self) -> Result<Option<PathBuf>> {
        self.publish_dir
            .as_ref()
            .map(|publish_dir| {
                if publish_dir.is_absolute() {
                    Ok(publish_dir.clone())
                } else {
                    Ok(self.project_root.join(publish_dir))
                }
            })
            .transpose()
    }

    /// Resolves whether a generation tool is enabled for this operation.
    pub fn generation_tool_enabled(&self, tool: GenerationToolKind) -> bool {
        self.generation_tools
            .0
            .get(&tool)
            .copied()
            .unwrap_or_else(|| {
                tool == GenerationToolKind::ImageGen
                    && self.model_defaults.contains_key(&Capability::Image)
            })
    }
}

pub fn validate_project_name(name: &str) -> Result<()> {
    let path = Path::new(name);
    if name.trim().is_empty() {
        bail!("Project name cannot be empty");
    }
    if path.components().count() != 1
        || !matches!(
            path.components().next(),
            Some(std::path::Component::Normal(_))
        )
    {
        bail!("Project name '{name}' cannot contain path separators or traversal");
    }
    Ok(())
}

fn merge_model_defaults(
    mut user: BTreeMap<Capability, String>,
    project: BTreeMap<Capability, String>,
    command: BTreeMap<Capability, String>,
) -> BTreeMap<Capability, String> {
    user.extend(project);
    user.extend(command);
    user
}

fn merge_model_roles(
    mut user: BTreeMap<ModelRole, String>,
    project: BTreeMap<ModelRole, String>,
    reviewer: Option<String>,
) -> BTreeMap<ModelRole, String> {
    user.extend(project);
    if let Some(profile) = reviewer {
        user.insert(ModelRole::Reviewer, profile);
    }
    user
}

fn resolve_theme_name(project_theme: &str, command_theme: Option<String>) -> String {
    command_theme.unwrap_or_else(|| project_theme.to_string())
}

fn merge_tool_defaults(
    mut project: BTreeMap<GenerationToolKind, bool>,
    command: BTreeMap<GenerationToolKind, bool>,
) -> BTreeMap<GenerationToolKind, bool> {
    project.extend(command);
    project
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../tests/unit/config.rs"]
mod tests;
