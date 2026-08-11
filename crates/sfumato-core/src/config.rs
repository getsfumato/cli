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

/// What one run overrides, on top of user and project configuration.
///
/// The third and last layer of resolution: user, then project, then this. Every
/// field is optional because absent means "whatever the layers below say" — the
/// distinction matters, and [`Self::pdf`] is the field that proves it.
#[derive(Clone, Debug, Default)]
pub struct ConfigOverrides {
    /// Which project to act on, instead of the active one.
    pub project: Option<String>,
    /// A theme for this run only.
    pub theme: Option<String>,
    /// Per-capability profile choices, as `--model text=fast` supplies.
    pub model_overrides: BTreeMap<Capability, String>,
    /// A reviewer profile for this run only.
    pub reviewer_model: Option<String>,
    /// Where to publish the finished artifact, overriding the project's setting.
    pub publish_dir: Option<PathBuf>,
    /// PDF override for one run; the project decides when absent.
    ///
    /// `Option` rather than `bool` because an OR cannot express "off": with
    /// `marp.pdf = true` in the config — which is the shipped default — no flag
    /// combination could turn PDF off for a single run.
    pub pdf: Option<bool>,
    /// Per-tool on/off decisions for this run. Enabling and disabling the same tool
    /// is refused rather than resolved to one of them.
    pub tool_overrides: BTreeMap<GenerationToolKind, bool>,
}

/// The user-global document: everything shared by every project.
#[derive(Clone, Debug, Serialize)]
pub struct GlobalConfig {
    /// Who this is, which the prompts use to pitch explanations.
    pub user: UserConfig,
    /// Configured providers, by the name the user gave each one.
    pub connectors: BTreeMap<String, ConnectorConfig>,
    /// Model profiles, by name.
    pub models: BTreeMap<String, ModelProfile>,
    /// Which profile serves each capability when nothing overrides it.
    pub defaults: ModelDefaults,
    /// Which profile serves each named role, such as the reviewer.
    #[serde(default)]
    pub model_roles: BTreeMap<ModelRole, String>,
    /// Slide export settings.
    pub marp: MarpConfig,
    /// The browser the renderers launch.
    #[serde(default)]
    pub browser: BrowserConfig,
}

/// Who the resources are being made for.
///
/// Reaches the prompts: a deck written for someone who said they learn from worked
/// examples should not be a wall of definitions.
#[derive(Clone, Debug, Serialize)]
pub struct UserConfig {
    /// What to call them, when they said.
    pub name: Option<String>,
    /// How they say they learn, in their own words.
    pub learning_style: Vec<String>,
}

/// Every project this user has registered, and which one is current.
#[derive(Clone, Debug, Default, Serialize)]
pub struct ProjectRegistry {
    /// The project used when a command omits `--project`.
    pub active: Option<String>,
    /// Registered projects, by name.
    pub projects: BTreeMap<String, RegisteredProject>,
}

/// A registry entry: a name pointing at a directory.
///
/// The project's own settings live in that directory, not here, which is what lets a
/// project be moved between machines with its configuration intact.
#[derive(Clone, Debug, Serialize)]
pub struct RegisteredProject {
    /// Canonical root of the project.
    pub path: PathBuf,
}

/// One project's own document, portable with its directory.
#[derive(Clone, Debug, Serialize)]
pub struct ProjectConfig {
    /// Registry name; must match the entry pointing here.
    pub name: String,
    /// Installed theme this project renders with.
    pub theme: String,
    /// Where finished artifacts are copied, relative to the project root. The
    /// managed revision under `~/.sfumato` stays authoritative either way.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publish_dir: Option<PathBuf>,
    /// Per-capability profile choices that override the user-global defaults.
    #[serde(default)]
    pub model_defaults: BTreeMap<Capability, String>,
    /// Per-role profile choices that override the user-global ones.
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
    /// Where this project's resources may draw their claims from.
    #[serde(default)]
    pub knowledge: KnowledgeConfig,
    /// Slide export settings for this project. `None` uses the user-global ones;
    /// `Some` replaces them wholesale rather than merging field by field.
    #[serde(default)]
    pub marp: Option<MarpConfig>,
}

/// A named binding from capabilities to one connector's model.
///
/// The indirection is the point: a workflow asks for `text`, and which model that is
/// can change without touching a prompt or a command.
#[derive(Clone, Debug, Serialize)]
pub struct ModelProfile {
    /// Which configured connector to generate through.
    pub connector: String,
    /// Provider-side model identifier, as the provider spells it.
    pub model: String,
    /// What this profile may be selected for. Selecting it for anything else is
    /// refused, because the layer below would reject it anyway.
    pub capabilities: Vec<Capability>,
    /// Per-capability generation options.
    #[serde(default)]
    pub options: ModelOptions,
}

/// Capability-specific options for a model profile.
#[derive(Clone, Debug, Default, Serialize)]
pub struct ModelOptions {
    /// Applied to text and code generation.
    pub text: TextModelOptions,
    /// Applied to image generation.
    pub image: ImageModelOptions,
    /// Applied to video generation.
    pub video: VideoModelOptions,
    /// Applied to speech synthesis.
    pub speech: SpeechModelOptions,
}

/// Options used by text and code generation.
#[derive(Clone, Debug, Default, Serialize)]
pub struct TextModelOptions {
    /// Sampling temperature. Absent leaves the provider's default alone.
    pub temperature: Option<f32>,
    /// Output ceiling. Reaching it fails the generation rather than using a
    /// truncated answer, so raising it is the remedy an output-limit error names.
    pub max_tokens: Option<u32>,
    /// How many rounds of tool calls a generation may take before being asked to
    /// answer with what it has.
    pub max_tool_rounds: Option<usize>,
    /// Nucleus sampling cutoff.
    pub top_p: Option<f32>,
    /// Sampling seed, where the provider honours one.
    pub seed: Option<i64>,
}

/// Options used by image generation.
#[derive(Clone, Debug, Default, Serialize)]
pub struct ImageModelOptions {
    /// Provider-specific quality tier.
    pub quality: Option<String>,
    /// Background treatment, such as transparency where it is supported.
    pub background: Option<String>,
    /// Pixel dimensions, as the provider spells them.
    pub size: Option<String>,
    /// Aspect ratio, for providers that take one instead of a size.
    pub aspect_ratio: Option<String>,
    /// Encoding to ask for.
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

/// Options used by speech-synthesis models.
///
/// Every field is optional so a profile can name only a voice and let the
/// provider decide the rest. Voice and model live here rather than in the
/// profile's `model` field because a speech profile has two identifiers: the
/// synthesis model and the voice it speaks with, and only one of them fits the
/// shared `model` slot.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SpeechModelOptions {
    /// Provider voice identifier used when a request names none.
    pub voice: Option<String>,
    /// Container and sample rate such as `mp3_44100_128`.
    pub output_format: Option<String>,
    /// ISO 639-1 language hint for text normalization.
    pub language: Option<String>,
    /// Voice consistency, 0 to 1.
    pub stability: Option<f32>,
    /// Similarity to the reference voice, 0 to 1.
    pub similarity_boost: Option<f32>,
    /// Expressiveness, 0 to 1.
    pub style: Option<f32>,
    /// Speaking rate multiplier, normally near 1.
    pub speed: Option<f32>,
    /// Whether the provider should boost speaker likeness.
    pub speaker_boost: Option<bool>,
    /// Silence appended after each synthesized segment, in seconds.
    ///
    /// A film reads as rushed when the next beat cuts in on the last syllable, so
    /// this is timeline direction rather than a synthesis parameter; it is kept
    /// with the voice because it is the same decision a narrator makes.
    pub segment_gap_seconds: Option<f32>,
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
    /// Speak text aloud and return a local audio artifact with word timings.
    AudioGen,
    /// Plot data locally with matplotlib and return a local image path.
    ChartGen,
}

impl GenerationToolKind {
    /// Every tool, in presentation order.
    pub const ALL: [Self; 4] = [
        Self::ImageGen,
        Self::VideoGen,
        Self::AudioGen,
        Self::ChartGen,
    ];

    /// Stable CLI and configuration identifier.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ImageGen => "image-gen",
            Self::VideoGen => "video-gen",
            Self::AudioGen => "audio-gen",
            Self::ChartGen => "chart-gen",
        }
    }

    /// Model capability required by the tool, when it needs a model at all.
    ///
    /// Charting has none: the drafting model writes the plotting code itself and
    /// Sfumato runs it locally, so there is no second model to configure and no
    /// capability whose absence should report the tool as unavailable.
    pub const fn capability(self) -> Option<Capability> {
        match self {
            Self::ImageGen => Some(Capability::Image),
            Self::VideoGen => Some(Capability::Video),
            Self::AudioGen => Some(Capability::Speech),
            Self::ChartGen => None,
        }
    }
}

impl FromStr for GenerationToolKind {
    type Err = SfumatoError;

    fn from_str(value: &str) -> Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "image-gen" | "image_gen" => Ok(Self::ImageGen),
            "video-gen" | "video_gen" => Ok(Self::VideoGen),
            "audio-gen" | "audio_gen" => Ok(Self::AudioGen),
            "chart-gen" | "chart_gen" => Ok(Self::ChartGen),
            _ => bail!(
                "Unknown generation tool '{value}'. Use image-gen, video-gen, audio-gen, or chart-gen."
            ),
        }
    }
}

/// Explicit project defaults for model-backed generation tools.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(transparent)]
pub struct GenerationToolDefaults(pub BTreeMap<GenerationToolKind, bool>);

/// Where a project's resources may draw their claims from.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeBackend {
    /// Local files, read by the model through the filesystem tools.
    #[default]
    Filesystem,
    /// A Vitruvio brain, queried through a single search tool.
    Vitruvio,
}

impl KnowledgeBackend {
    /// Every backend, in presentation order.
    pub const ALL: [Self; 2] = [Self::Filesystem, Self::Vitruvio];

    /// Stable CLI and configuration identifier.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Filesystem => "filesystem",
            Self::Vitruvio => "vitruvio",
        }
    }
}

impl std::fmt::Display for KnowledgeBackend {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for KnowledgeBackend {
    type Err = SfumatoError;

    fn from_str(value: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|backend| backend.as_str() == value.trim().to_ascii_lowercase())
            .ok_or_else(|| {
                SfumatoError::validation(format!(
                    "Unknown knowledge backend '{value}'. Use filesystem or vitruvio."
                ))
            })
    }
}

/// Project settings for the knowledge source behind resource generation.
///
/// Project-scoped rather than global: a brain belongs to the work, not to the
/// machine, and two projects on one machine routinely ground differently.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct KnowledgeConfig {
    /// Which knowledge source grounds this project.
    #[serde(default)]
    pub backend: KnowledgeBackend,
    /// Brain name from the project's `vitruvio.toml`, or a path to one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brain: Option<String>,
    /// Optional explicit Vitruvio configuration file.
    #[serde(default, rename = "config", skip_serializing_if = "Option::is_none")]
    pub config_file: Option<PathBuf>,
    /// Optional explicit brain executable, for one not on `PATH`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable: Option<PathBuf>,
    /// Actor identity recorded against each query.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    /// Modules every query is restricted to unless the model says otherwise.
    #[serde(default)]
    pub memory_types: Vec<crate::knowledge::MemoryType>,
    /// Whether replaced blocks are returned by default.
    #[serde(default)]
    pub include_superseded: bool,
    /// Matches a query returns when the model asks for no particular number.
    #[serde(default = "default_query_limit")]
    pub default_limit: usize,
    /// Ceiling the model's own `limit` is clamped to.
    #[serde(default = "default_query_maximum")]
    pub max_limit: usize,
    /// Wall-clock bound for one brain invocation.
    #[serde(default = "default_query_timeout")]
    pub timeout_seconds: u64,
}

const fn default_query_limit() -> usize {
    10
}

const fn default_query_maximum() -> usize {
    50
}

const fn default_query_timeout() -> u64 {
    60
}

impl Default for KnowledgeConfig {
    fn default() -> Self {
        Self {
            backend: KnowledgeBackend::default(),
            brain: None,
            config_file: None,
            executable: None,
            actor: None,
            memory_types: Vec::new(),
            include_superseded: false,
            default_limit: default_query_limit(),
            max_limit: default_query_maximum(),
            timeout_seconds: default_query_timeout(),
        }
    }
}

impl KnowledgeConfig {
    /// Whether resources in this project are grounded in a brain.
    pub const fn uses_brain(&self) -> bool {
        matches!(self.backend, KnowledgeBackend::Vitruvio)
    }

    /// Rejects a knowledge table that cannot describe a reachable brain.
    pub fn validate(&self) -> Result<()> {
        if self.default_limit == 0 {
            bail!("knowledge.default_limit must be at least 1");
        }
        if self.max_limit < self.default_limit {
            bail!(
                "knowledge.max_limit ({}) cannot be below knowledge.default_limit ({})",
                self.max_limit,
                self.default_limit
            );
        }
        if self.max_limit > 200 {
            bail!("knowledge.max_limit cannot exceed 200");
        }
        if self.timeout_seconds == 0 {
            bail!("knowledge.timeout_seconds must be at least 1");
        }
        if !self.uses_brain() {
            return Ok(());
        }
        match self.brain.as_deref().map(str::trim) {
            Some(brain) if !brain.is_empty() => Ok(()),
            // Checked here rather than mid-generation: a brain-backed project
            // that cannot name its brain has nothing to draft from, and finding
            // that out after a model call has already been paid for is worse
            // than finding it out while editing the config.
            _ => bail!(
                "knowledge.backend is \"vitruvio\" but no brain is named. \
                 Set knowledge.brain in .sfumato/project.toml."
            ),
        }
    }
}

/// Project trust settings for generated-code renderers.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ProjectSecurityConfig {
    /// Whether generated Python may execute without a command override.
    ///
    /// Reads `allow_manim` too: Manim was the only generated-Python workflow when
    /// the setting was named, and a project that already consented to executing
    /// generated Python should not be asked again because a second workflow
    /// started using the same runtime.
    #[serde(default, alias = "allow_manim")]
    pub allow_python: bool,
    /// Requirements a project permits on top of the pinned base environments.
    ///
    /// Empty by default: layering a package means installing it from an index at
    /// generation time, which is a decision a project makes explicitly rather
    /// than one a model makes for it mid-draft.
    #[serde(default)]
    pub python_packages: Vec<String>,
}

impl ProjectSecurityConfig {
    /// Rejects an extra requirement the project has not permitted.
    ///
    /// Matching is on the package name, so a project that allows `scipy` accepts
    /// whichever pin the caller asks for rather than having to enumerate every
    /// version it might want.
    pub fn authorize_python_package(&self, requirement: &str) -> Result<()> {
        crate::python::validate_requirement(requirement)?;
        let name = |value: &str| {
            value
                .split_once("==")
                .map_or(value, |(name, _)| name)
                .trim()
                .to_ascii_lowercase()
        };
        let requested = name(requirement);
        if self
            .python_packages
            .iter()
            .any(|allowed| name(allowed) == requested)
        {
            return Ok(());
        }
        Err(SfumatoError::validation(format!(
            "Python package '{requested}' is not permitted. Add it to security.python_packages to allow it."
        )))
    }
}

impl ModelOptions {
    /// Configured temperature, or the shipped default.
    pub fn text_temperature(&self) -> f32 {
        self.text.temperature.unwrap_or(0.4)
    }

    /// Configured output ceiling, or the shipped default.
    pub fn text_max_tokens(&self) -> u32 {
        self.text.max_tokens.unwrap_or(4000)
    }

    /// Configured tool-round limit, or the shipped default.
    pub fn tool_rounds(&self) -> usize {
        self.text
            .max_tool_rounds
            .filter(|rounds| *rounds > 0)
            .unwrap_or(8)
    }

    /// Applies only the fields `changes` actually sets.
    ///
    /// Field-by-field so editing one option does not silently clear the rest.
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
        macro_rules! replace_speech_some {
            ($($field:ident),+ $(,)?) => {
                $(if changes.speech.$field.is_some() { self.speech.$field = changes.speech.$field; })+
            };
        }
        replace_speech_some!(
            voice,
            output_format,
            language,
            stability,
            similarity_boost,
            style,
            speed,
            speaker_boost,
            segment_gap_seconds,
        );
    }

    /// The options as `key=value` strings, for echoing a profile back to a user.
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
        push_option!(speech, voice);
        push_option!(speech, output_format);
        push_option!(speech, language);
        push_option!(speech, stability);
        push_option!(speech, similarity_boost);
        push_option!(speech, style);
        push_option!(speech, speed);
        push_option!(speech, speaker_boost);
        push_option!(speech, segment_gap_seconds);
        pairs
    }
}

#[derive(Clone, Debug, Default, Serialize)]
/// Which profile serves each capability.
pub struct ModelDefaults(pub BTreeMap<Capability, String>);

/// A job a model does in a run, as distinct from what it must be able to do.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum ModelRole {
    /// The model that reviews generated content against the instruction and the
    /// sources. A role rather than a capability because it needs text like the
    /// drafter does, so the capability alone could not tell them apart.
    Reviewer,
}

impl ModelRole {
    /// The stable identifier a config stores and a command accepts.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reviewer => "reviewer",
        }
    }

    /// What a profile must declare to serve this role.
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
/// One OpenAI-compatible endpoint, which is what Ollama, LM Studio and
/// OpenRouter all speak.

#[derive(Clone, Debug, Serialize)]
pub struct OpenAiCompatibleConnectorConfig {
    /// Endpoint serving `/v1`.
    pub base_url: String,
    /// Reference to the credential, never the credential.
    pub credential: Option<SecretRef>,
    /// Extra headers sent with every request.
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

/// Native ElevenLabs speech connector.
///
/// Not composed from [`OpenAiCompatibleConnectorConfig`] for the same reason
/// Anthropic is not: it authenticates with `xi-api-key` rather than bearer auth
/// and speaks its own wire format, so typing it as an OpenAI-compatible
/// transport would let a refactor route it through the wrong provider factory.
#[derive(Clone, Debug, Serialize)]
pub struct ElevenLabsConnectorConfig {
    /// API root, normally `https://api.elevenlabs.io`.
    pub base_url: String,
    /// Indirect reference to the `xi-api-key` credential.
    pub credential: Option<SecretRef>,
    /// Extra request headers.
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
    /// ElevenLabs speaking its native speech-synthesis API.
    ElevenLabs(ElevenLabsConnectorConfig),
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
            Self::ElevenLabs(_) => "elevenlabs",
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
            Self::ElevenLabs(config) => config.base_url.clone(),
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
            // Neither is the speech API.
            Self::ElevenLabs(_) => None,
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
            Self::ElevenLabs(_) => None,
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
            Self::ElevenLabs(config) => ConnectorAuth::Managed(config.credential.as_ref()),
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
            Self::ElevenLabs(config) => config.credential = reference,
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
/// How decks are exported.

#[derive(Clone, Debug, Serialize)]
pub struct MarpConfig {
    /// Whether a deck is exported to PDF. On by default; `--no-pdf` is what turns it
    /// off for one run, because configuration alone could only turn it on.
    pub pdf: bool,
    /// Deprecated location for the browser path. Read for compatibility; new
    /// configuration writes `[browser] path`. See [`BrowserConfig`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser_path: Option<PathBuf>,
}

/// Which browser the renderers launch.
///
/// Its own section rather than a Marp setting, which is what it used to be: slides,
/// pages, documents and Mermaid diagrams all launch the same browser, so
/// `marp.browser_path` named one caller of four. The old key is still read, so no
/// existing configuration has to change and there is no schema migration —
/// `[browser] path` simply wins where both are present.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserConfig {
    /// Executable to launch. Left unset, discovery finds one: `SFUMATO_BROWSER`,
    /// then `PUPPETEER_EXECUTABLE_PATH` and `CHROME_PATH`, then `PATH`, then the
    /// platform's usual locations. A path that does not exist is an error rather
    /// than a silent fall back to discovery.
    #[serde(default)]
    pub path: Option<PathBuf>,
}
/// Everything one run needs, with all three layers already resolved.

#[derive(Clone, Debug, Serialize)]
pub struct EffectiveConfig {
    /// Who the resource is for.
    pub user: UserConfig,
    /// The project this run acts on.
    pub project_name: String,
    /// Its canonical root, which bounds what the model may read.
    pub project_root: PathBuf,
    /// Where the finished artifact is copied, if anywhere.
    pub publish_dir: Option<PathBuf>,
    /// The theme to render with.
    pub theme: String,
    /// Configured connectors.
    pub connectors: BTreeMap<String, ConnectorConfig>,
    /// Configured profiles.
    pub models: BTreeMap<String, ModelProfile>,
    /// The resolved per-capability choices, after all three layers.
    pub model_defaults: BTreeMap<Capability, String>,
    /// The resolved per-role choices.
    pub model_roles: BTreeMap<ModelRole, String>,
    /// Page-only visual extension defaults.
    pub page: PageDefaults,
    /// Effective model-backed generation-tool policy.
    pub generation_tools: GenerationToolDefaults,
    /// Project trust decisions for generated code.
    pub security: ProjectSecurityConfig,
    /// Where this project's resources may draw their claims from.
    pub knowledge: KnowledgeConfig,
    /// Slide export settings.
    pub marp: MarpConfig,
    /// The browser every renderer launches.
    pub browser: BrowserConfig,
}

impl GlobalConfig {
    /// The configuration `sfumato init user --yes` writes.
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
            browser: BrowserConfig::default(),
        }
    }

    /// Checks the whole document, not one field.
    ///
    /// Every write validates the result of the edit, so a single-key change cannot
    /// leave configuration in a state a later command would refuse.
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
                ConnectorConfig::ElevenLabs(connector) if connector.base_url.trim().is_empty() => {
                    bail!("ElevenLabs connector base URLs cannot be empty");
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
    /// Checks the whole project document.
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
        // No tool loop here on purpose. One existed and discarded its own result,
        // validating nothing. The check it was reaching for — that each enabled
        // tool has a model configured for its capability — cannot be made from a
        // project config, which does not know the global model profiles; it belongs
        // where capabilities are resolved. And the keys are a typed enum, so an
        // unknown tool cannot reach this map: deserialization rejects it first.
        // Caught here rather than at install time: a malformed requirement in the
        // allowlist is a typo in the project's own trust decision, and reporting
        // it while editing the config beats reporting it mid-generation.
        for requirement in &self.security.python_packages {
            crate::python::validate_requirement(requirement)?;
        }
        self.knowledge.validate()?;
        Ok(())
    }
}

impl ProjectRegistry {
    /// The requested project, or the active one, with its root.
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
    /// Resolves user, project and per-run layers into one answer.
    ///
    /// The single place precedence is decided, so nothing downstream has to know
    /// that there were three documents.
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
        // `[browser] path` wins; `marp.browser_path` is the deprecated spelling and
        // is read so that no existing configuration has to be rewritten. Resolved
        // once, here, so every renderer downstream reads one field.
        let browser = BrowserConfig {
            path: global.browser.path.clone().or(marp.browser_path.clone()),
        };
        let marp = MarpConfig {
            pdf: overrides.pdf.unwrap_or(marp.pdf),
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
            knowledge: project.knowledge,
            marp,
            browser,
        })
    }

    /// Builds the binding one brain invocation needs, or says why it cannot.
    ///
    /// The single place configuration becomes a value the knowledge port
    /// accepts, so a project that is not brain-backed cannot silently produce a
    /// binding that names no brain.
    pub fn brain_binding(&self) -> Result<crate::knowledge::BrainBinding> {
        if !self.knowledge.uses_brain() {
            bail!(
                "Project '{}' is not grounded in a brain. \
                 Set knowledge.backend = \"vitruvio\" in .sfumato/project.toml.",
                self.project_name
            );
        }
        let brain = self
            .knowledge
            .brain
            .as_deref()
            .map(str::trim)
            .filter(|brain| !brain.is_empty())
            .context(
                "knowledge.backend is \"vitruvio\" but no brain is named. \
                 Set knowledge.brain in .sfumato/project.toml.",
            )?;
        Ok(crate::knowledge::BrainBinding {
            brain: brain.to_string(),
            config_file: self.knowledge.config_file.clone(),
            executable: self.knowledge.executable.clone(),
            actor: self.knowledge.actor.clone(),
            timeout_seconds: self.knowledge.timeout_seconds,
        })
    }

    /// The profile serving a capability, or why none does.
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

    /// The profile serving a role, falling back to the drafter where that is the
    /// defined behaviour.
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

    /// Where artifacts are published, resolved against the project root.
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
                // A tool whose capability has no configured default cannot run, so
                // the implicit answer is whether the project can actually back it.
                // Video generation stays opt-in because it spends a remote render
                // on a page that rarely asks for one.
                match tool {
                    GenerationToolKind::ImageGen => {
                        self.model_defaults.contains_key(&Capability::Image)
                    }
                    GenerationToolKind::AudioGen => {
                        self.model_defaults.contains_key(&Capability::Speech)
                    }
                    GenerationToolKind::VideoGen => false,
                    // Charting needs no model profile, only the project's consent
                    // to run generated Python, which the workflows check
                    // separately. Defaulting it on would run code a project never
                    // agreed to execute.
                    GenerationToolKind::ChartGen => false,
                }
            })
    }
}

/// Validates the stable project identifier grammar.
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
