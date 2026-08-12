//! Persisted schema-v5 configuration documents.

use std::{collections::BTreeMap, path::PathBuf};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use sfumato_core::{
    config::{
        AnthropicConnectorConfig, Capability, CodexAppServerConnectorConfig, ConnectorConfig,
        ElevenLabsConnectorConfig, GenerationToolDefaults, GenerationToolKind, GlobalConfig,
        ImageModelOptions, KnowledgeBackend, KnowledgeConfig, LmStudioConnectorConfig, MarpConfig,
        ModelDefaults, ModelOptions, ModelProfile, ModelRole, OllamaConnectorConfig,
        OpenAiCompatibleConnectorConfig, OpenRouterConnectorConfig, PageDefaults, ProjectConfig,
        ProjectRegistry, ProjectSecurityConfig, RegisteredProject, SecretRef, SpeechModelOptions,
        TextModelOptions, UserConfig, VideoAudioMode, VideoModelOptions,
    },
    knowledge::MemoryType,
};

/// Current persisted configuration schema.
pub const CONFIG_SCHEMA_VERSION: u32 = 5;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GlobalConfigDto {
    schema_version: u32,
    user: UserConfigDto,
    connectors: BTreeMap<String, ConnectorDto>,
    models: BTreeMap<String, ModelProfileDto>,
    defaults: BTreeMap<Capability, String>,
    #[serde(default)]
    model_roles: BTreeMap<ModelRole, String>,
    marp: MarpConfigDto,
    /// Optional so a document written before this section existed still parses;
    /// `deny_unknown_fields` means it has to be declared to be accepted at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    browser: Option<BrowserConfigDto>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct UserConfigDto {
    name: Option<String>,
    learning_style: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ConnectorDto {
    #[serde(default)]
    kind: ConnectorKindDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    base_url: Option<String>,
    #[serde(default)]
    credential: Option<SecretRef>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    executable: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    native_base_url: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum ConnectorKindDto {
    #[default]
    OpenaiCompatible,
    Openrouter,
    Ollama,
    // `rename_all = "snake_case"` would emit `lm_studio`; pin the single-word
    // spelling and keep the derived form as a read alias.
    #[serde(rename = "lmstudio", alias = "lm_studio")]
    LmStudio,
    Anthropic,
    Elevenlabs,
    #[serde(alias = "codex_cli")]
    CodexAppServer,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ModelProfileDto {
    connector: String,
    model: String,
    capabilities: Vec<Capability>,
    #[serde(default)]
    options: ModelOptionsDto,
}

/// The file intentionally keeps options flat for human editing. Conversion
/// groups them by capability before they enter the application layer.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ModelOptionsDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_tool_rounds: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    seed: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    quality: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    background: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    aspect_ratio: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    output_format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    video_duration_seconds: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    video_resolution: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    video_aspect_ratio: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    video_audio: Option<VideoAudioMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    video_seed: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    video_poll_interval_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    video_timeout_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    speech_voice: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    speech_output_format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    speech_language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    speech_stability: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    speech_similarity_boost: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    speech_style: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    speech_speed: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    speech_speaker_boost: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    speech_segment_gap_seconds: Option<f32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct MarpConfigDto {
    pdf: bool,
    /// The deprecated spelling of the browser path, still read so that no existing
    /// configuration has to be rewritten. `[browser] path` wins where both appear.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    browser_path: Option<PathBuf>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct BrowserConfigDto {
    #[serde(default)]
    path: Option<PathBuf>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProjectRegistryDto {
    schema_version: u32,
    active: Option<String>,
    projects: BTreeMap<String, RegisteredProjectDto>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RegisteredProjectDto {
    path: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProjectConfigDto {
    schema_version: u32,
    name: String,
    theme: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    publish_dir: Option<PathBuf>,
    #[serde(default)]
    model_defaults: BTreeMap<Capability, String>,
    #[serde(default)]
    model_roles: BTreeMap<ModelRole, String>,
    #[serde(default)]
    page: PageDefaultsDto,
    #[serde(default)]
    generation_tools: BTreeMap<GenerationToolKind, bool>,
    #[serde(default)]
    security: ProjectSecurityDto,
    // Skipped when it matches the shipped defaults, because project files are
    // read back and rewritten by `sfumato config set`: without this, every
    // existing project would grow an empty `[knowledge]` table the first time
    // anything unrelated was edited.
    #[serde(default, skip_serializing_if = "KnowledgeDto::is_default")]
    knowledge: KnowledgeDto,
    #[serde(default)]
    marp: Option<MarpConfigDto>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PageDefaultsDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ui: Option<String>,
    #[serde(default)]
    plugins: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProjectSecurityDto {
    // `allow_manim` is still read so a project that already consented to running
    // generated Python is not asked again now that charts use the same runtime.
    #[serde(default, alias = "allow_manim")]
    allow_python: bool,
    #[serde(default)]
    python_packages: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct KnowledgeDto {
    #[serde(default)]
    backend: KnowledgeBackend,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    brain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    config: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    executable: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    actor: Option<String>,
    #[serde(default)]
    memory_types: Vec<MemoryType>,
    #[serde(default)]
    include_superseded: bool,
    #[serde(default = "default_knowledge_limit")]
    default_limit: usize,
    #[serde(default = "default_knowledge_maximum")]
    max_limit: usize,
    #[serde(default = "default_knowledge_timeout")]
    timeout_seconds: u64,
}

fn default_knowledge_limit() -> usize {
    KnowledgeConfig::default().default_limit
}

fn default_knowledge_maximum() -> usize {
    KnowledgeConfig::default().max_limit
}

fn default_knowledge_timeout() -> u64 {
    KnowledgeConfig::default().timeout_seconds
}

impl Default for KnowledgeDto {
    fn default() -> Self {
        Self::from_domain(&KnowledgeConfig::default())
    }
}

impl KnowledgeDto {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }

    fn into_domain(self) -> KnowledgeConfig {
        KnowledgeConfig {
            backend: self.backend,
            project: self.project,
            brain: self.brain,
            config_file: self.config,
            executable: self.executable,
            actor: self.actor,
            memory_types: self.memory_types,
            include_superseded: self.include_superseded,
            default_limit: self.default_limit,
            max_limit: self.max_limit,
            timeout_seconds: self.timeout_seconds,
        }
    }

    fn from_domain(knowledge: &KnowledgeConfig) -> Self {
        Self {
            backend: knowledge.backend,
            project: knowledge.project.clone(),
            brain: knowledge.brain.clone(),
            config: knowledge.config_file.clone(),
            executable: knowledge.executable.clone(),
            actor: knowledge.actor.clone(),
            memory_types: knowledge.memory_types.clone(),
            include_superseded: knowledge.include_superseded,
            default_limit: knowledge.default_limit,
            max_limit: knowledge.max_limit,
            timeout_seconds: knowledge.timeout_seconds,
        }
    }
}

impl GlobalConfigDto {
    pub(crate) fn into_domain(self) -> Result<GlobalConfig> {
        validate_schema(self.schema_version, "global")?;
        let config = GlobalConfig {
            user: UserConfig {
                name: self.user.name,
                learning_style: self.user.learning_style,
            },
            connectors: self
                .connectors
                .into_iter()
                .map(|(name, connector)| {
                    let connector = match connector.kind {
                        ConnectorKindDto::OpenaiCompatible => {
                            ConnectorConfig::OpenAiCompatible(OpenAiCompatibleConnectorConfig {
                                base_url: connector.base_url.ok_or_else(|| {
                                    anyhow::anyhow!(
                                        "OpenAI-compatible connector '{name}' requires base_url"
                                    )
                                })?,
                                credential: connector.credential,
                                headers: connector.headers,
                            })
                        }
                        ConnectorKindDto::Openrouter => {
                            if connector.executable.is_some() || connector.native_base_url.is_some() {
                                bail!("OpenRouter connector '{name}' cannot define executable or native_base_url");
                            }
                            ConnectorConfig::OpenRouter(OpenRouterConnectorConfig {
                                transport: OpenAiCompatibleConnectorConfig {
                                    base_url: connector.base_url.ok_or_else(|| anyhow::anyhow!(
                                        "OpenRouter connector '{name}' requires base_url"
                                    ))?,
                                    credential: connector.credential,
                                    headers: connector.headers,
                                },
                            })
                        }
                        ConnectorKindDto::Ollama => {
                            if connector.executable.is_some() {
                                bail!("Ollama connector '{name}' cannot define executable");
                            }
                            ConnectorConfig::Ollama(OllamaConnectorConfig {
                                transport: OpenAiCompatibleConnectorConfig {
                                    base_url: connector.base_url.ok_or_else(|| anyhow::anyhow!(
                                        "Ollama connector '{name}' requires base_url"
                                    ))?,
                                    credential: connector.credential,
                                    headers: connector.headers,
                                },
                                native_base_url: connector.native_base_url.ok_or_else(|| anyhow::anyhow!(
                                    "Ollama connector '{name}' requires native_base_url"
                                ))?,
                            })
                        }
                        ConnectorKindDto::LmStudio => {
                            if connector.executable.is_some() {
                                bail!("LM Studio connector '{name}' cannot define executable");
                            }
                            ConnectorConfig::LmStudio(LmStudioConnectorConfig {
                                transport: OpenAiCompatibleConnectorConfig {
                                    base_url: connector.base_url.ok_or_else(|| anyhow::anyhow!(
                                        "LM Studio connector '{name}' requires base_url"
                                    ))?,
                                    credential: connector.credential,
                                    headers: connector.headers,
                                },
                                native_base_url: connector.native_base_url.ok_or_else(|| anyhow::anyhow!(
                                    "LM Studio connector '{name}' requires native_base_url"
                                ))?,
                            })
                        }
                        ConnectorKindDto::Anthropic => {
                            if connector.executable.is_some()
                                || connector.native_base_url.is_some()
                            {
                                bail!("Anthropic connector '{name}' cannot define executable or native_base_url");
                            }
                            ConnectorConfig::Anthropic(AnthropicConnectorConfig {
                                base_url: connector.base_url.ok_or_else(|| anyhow::anyhow!(
                                    "Anthropic connector '{name}' requires base_url"
                                ))?,
                                credential: connector.credential,
                                headers: connector.headers,
                            })
                        }
                        ConnectorKindDto::Elevenlabs => {
                            if connector.executable.is_some()
                                || connector.native_base_url.is_some()
                            {
                                bail!("ElevenLabs connector '{name}' cannot define executable or native_base_url");
                            }
                            ConnectorConfig::ElevenLabs(ElevenLabsConnectorConfig {
                                base_url: connector.base_url.ok_or_else(|| anyhow::anyhow!(
                                    "ElevenLabs connector '{name}' requires base_url"
                                ))?,
                                credential: connector.credential,
                                headers: connector.headers,
                            })
                        }
                        ConnectorKindDto::CodexAppServer => {
                            if connector.base_url.is_some()
                                || connector.credential.is_some()
                                || !connector.headers.is_empty()
                                || connector.native_base_url.is_some()
                            {
                                bail!(
                                    "Codex App Server connector '{name}' cannot define base_url, credential, or headers"
                                );
                            }
                            ConnectorConfig::CodexAppServer(CodexAppServerConnectorConfig {
                                executable: connector
                                    .executable
                                    .unwrap_or_else(|| PathBuf::from("codex")),
                            })
                        }
                    };
                    Ok((name, connector))
                })
                .collect::<Result<_>>()?,
            models: self
                .models
                .into_iter()
                .map(|(name, profile)| {
                    (
                        name,
                        ModelProfile {
                            connector: profile.connector,
                            model: profile.model,
                            capabilities: profile.capabilities,
                            options: profile.options.into(),
                        },
                    )
                })
                .collect(),
            defaults: ModelDefaults(self.defaults),
            model_roles: self.model_roles,
            marp: self.marp.into(),
            browser: self
                .browser
                .map(|browser| sfumato_core::config::BrowserConfig { path: browser.path })
                .unwrap_or_default(),
        };
        config.validate()?;
        Ok(config)
    }

    pub(crate) fn from_domain(config: &GlobalConfig) -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            user: UserConfigDto {
                name: config.user.name.clone(),
                learning_style: config.user.learning_style.clone(),
            },
            connectors: config
                .connectors
                .iter()
                .map(|(name, connector)| {
                    let connector = match connector {
                        ConnectorConfig::OpenAiCompatible(connector) => ConnectorDto {
                            kind: ConnectorKindDto::OpenaiCompatible,
                            base_url: Some(connector.base_url.clone()),
                            credential: connector.credential.clone(),
                            headers: connector.headers.clone(),
                            executable: None,
                            native_base_url: None,
                        },
                        ConnectorConfig::OpenRouter(connector) => ConnectorDto {
                            kind: ConnectorKindDto::Openrouter,
                            base_url: Some(connector.transport.base_url.clone()),
                            credential: connector.transport.credential.clone(),
                            headers: connector.transport.headers.clone(),
                            executable: None,
                            native_base_url: None,
                        },
                        ConnectorConfig::Ollama(connector) => ConnectorDto {
                            kind: ConnectorKindDto::Ollama,
                            base_url: Some(connector.transport.base_url.clone()),
                            credential: connector.transport.credential.clone(),
                            headers: connector.transport.headers.clone(),
                            executable: None,
                            native_base_url: Some(connector.native_base_url.clone()),
                        },
                        ConnectorConfig::LmStudio(connector) => ConnectorDto {
                            kind: ConnectorKindDto::LmStudio,
                            base_url: Some(connector.transport.base_url.clone()),
                            credential: connector.transport.credential.clone(),
                            headers: connector.transport.headers.clone(),
                            executable: None,
                            native_base_url: Some(connector.native_base_url.clone()),
                        },
                        ConnectorConfig::Anthropic(connector) => ConnectorDto {
                            kind: ConnectorKindDto::Anthropic,
                            base_url: Some(connector.base_url.clone()),
                            credential: connector.credential.clone(),
                            headers: connector.headers.clone(),
                            executable: None,
                            native_base_url: None,
                        },
                        ConnectorConfig::ElevenLabs(connector) => ConnectorDto {
                            kind: ConnectorKindDto::Elevenlabs,
                            base_url: Some(connector.base_url.clone()),
                            credential: connector.credential.clone(),
                            headers: connector.headers.clone(),
                            executable: None,
                            native_base_url: None,
                        },
                        ConnectorConfig::CodexAppServer(connector) => ConnectorDto {
                            kind: ConnectorKindDto::CodexAppServer,
                            base_url: None,
                            credential: None,
                            headers: BTreeMap::new(),
                            executable: Some(connector.executable.clone()),
                            native_base_url: None,
                        },
                    };
                    (name.clone(), connector)
                })
                .collect(),
            models: config
                .models
                .iter()
                .map(|(name, profile)| {
                    (
                        name.clone(),
                        ModelProfileDto {
                            connector: profile.connector.clone(),
                            model: profile.model.clone(),
                            capabilities: profile.capabilities.clone(),
                            options: ModelOptionsDto::from(&profile.options),
                        },
                    )
                })
                .collect(),
            defaults: config.defaults.0.clone(),
            model_roles: config.model_roles.clone(),
            marp: MarpConfigDto::from(&config.marp),
            // Written only when set, so a configuration that never used it does not
            // grow an empty section on every save.
            browser: config.browser.path.as_ref().map(|path| BrowserConfigDto {
                path: Some(path.clone()),
            }),
        }
    }
}

impl ProjectRegistryDto {
    pub(crate) fn into_domain(self) -> Result<ProjectRegistry> {
        validate_schema(self.schema_version, "project registry")?;
        Ok(ProjectRegistry {
            active: self.active,
            projects: self
                .projects
                .into_iter()
                .map(|(name, project)| (name, RegisteredProject { path: project.path }))
                .collect(),
        })
    }

    pub(crate) fn from_domain(registry: &ProjectRegistry) -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            active: registry.active.clone(),
            projects: registry
                .projects
                .iter()
                .map(|(name, project)| {
                    (
                        name.clone(),
                        RegisteredProjectDto {
                            path: project.path.clone(),
                        },
                    )
                })
                .collect(),
        }
    }
}

impl ProjectConfigDto {
    pub(crate) fn into_domain(self) -> Result<ProjectConfig> {
        validate_schema(self.schema_version, "project")?;
        let project = ProjectConfig {
            name: self.name,
            theme: self.theme,
            publish_dir: self.publish_dir,
            model_defaults: self.model_defaults,
            model_roles: self.model_roles,
            page: PageDefaults {
                ui: self.page.ui,
                plugins: self.page.plugins,
            },
            generation_tools: GenerationToolDefaults(self.generation_tools),
            security: ProjectSecurityConfig {
                allow_python: self.security.allow_python,
                python_packages: self.security.python_packages,
            },
            knowledge: self.knowledge.into_domain(),
            marp: self.marp.map(Into::into),
        };
        project.validate()?;
        Ok(project)
    }

    pub(crate) fn from_domain(project: &ProjectConfig) -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            name: project.name.clone(),
            theme: project.theme.clone(),
            publish_dir: project.publish_dir.clone(),
            model_defaults: project.model_defaults.clone(),
            model_roles: project.model_roles.clone(),
            page: PageDefaultsDto {
                ui: project.page.ui.clone(),
                plugins: project.page.plugins.clone(),
            },
            generation_tools: project.generation_tools.0.clone(),
            security: ProjectSecurityDto {
                allow_python: project.security.allow_python,
                python_packages: project.security.python_packages.clone(),
            },
            knowledge: KnowledgeDto::from_domain(&project.knowledge),
            marp: project.marp.as_ref().map(MarpConfigDto::from),
        }
    }
}

impl From<ModelOptionsDto> for ModelOptions {
    fn from(options: ModelOptionsDto) -> Self {
        Self {
            text: TextModelOptions {
                temperature: options.temperature,
                max_tokens: options.max_tokens,
                max_tool_rounds: options.max_tool_rounds,
                top_p: options.top_p,
                seed: options.seed,
            },
            image: ImageModelOptions {
                quality: options.quality,
                background: options.background,
                size: options.size,
                aspect_ratio: options.aspect_ratio,
                output_format: options.output_format,
            },
            video: VideoModelOptions {
                duration_seconds: options.video_duration_seconds,
                resolution: options.video_resolution,
                aspect_ratio: options.video_aspect_ratio,
                audio: options.video_audio,
                seed: options.video_seed,
                poll_interval_seconds: options.video_poll_interval_seconds,
                timeout_seconds: options.video_timeout_seconds,
            },
            speech: SpeechModelOptions {
                voice: options.speech_voice,
                output_format: options.speech_output_format,
                language: options.speech_language,
                stability: options.speech_stability,
                similarity_boost: options.speech_similarity_boost,
                style: options.speech_style,
                speed: options.speech_speed,
                speaker_boost: options.speech_speaker_boost,
                segment_gap_seconds: options.speech_segment_gap_seconds,
            },
        }
    }
}

impl From<&ModelOptions> for ModelOptionsDto {
    fn from(options: &ModelOptions) -> Self {
        Self {
            temperature: options.text.temperature,
            max_tokens: options.text.max_tokens,
            max_tool_rounds: options.text.max_tool_rounds,
            top_p: options.text.top_p,
            seed: options.text.seed,
            quality: options.image.quality.clone(),
            background: options.image.background.clone(),
            size: options.image.size.clone(),
            aspect_ratio: options.image.aspect_ratio.clone(),
            output_format: options.image.output_format.clone(),
            video_duration_seconds: options.video.duration_seconds,
            video_resolution: options.video.resolution.clone(),
            video_aspect_ratio: options.video.aspect_ratio.clone(),
            video_audio: options.video.audio,
            video_seed: options.video.seed,
            video_poll_interval_seconds: options.video.poll_interval_seconds,
            video_timeout_seconds: options.video.timeout_seconds,
            speech_voice: options.speech.voice.clone(),
            speech_output_format: options.speech.output_format.clone(),
            speech_language: options.speech.language.clone(),
            speech_stability: options.speech.stability,
            speech_similarity_boost: options.speech.similarity_boost,
            speech_style: options.speech.style,
            speech_speed: options.speech.speed,
            speech_speaker_boost: options.speech.speaker_boost,
            speech_segment_gap_seconds: options.speech.segment_gap_seconds,
        }
    }
}

impl From<MarpConfigDto> for MarpConfig {
    fn from(config: MarpConfigDto) -> Self {
        Self {
            pdf: config.pdf,
            browser_path: config.browser_path,
        }
    }
}

impl From<&MarpConfig> for MarpConfigDto {
    fn from(config: &MarpConfig) -> Self {
        Self {
            pdf: config.pdf,
            browser_path: config.browser_path.clone(),
        }
    }
}

fn validate_schema(actual: u32, kind: &str) -> Result<()> {
    if !matches!(actual, 4 | CONFIG_SCHEMA_VERSION) {
        bail!(
            "Unsupported {kind} config schema {actual}; Sfumato v0.2 requires schema {CONFIG_SCHEMA_VERSION}"
        );
    }
    Ok(())
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private persistence DTOs.
#[path = "../tests/unit/config_dto.rs"]
mod tests;
