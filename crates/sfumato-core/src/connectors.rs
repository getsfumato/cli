use std::{
    collections::{BTreeMap, btree_map::Entry},
    sync::Arc,
};

use sfumato_domain::SecretRef;

use crate::{
    config::{
        AnthropicConnectorConfig, CodexAppServerConnectorConfig, ConnectorAuth, ConnectorConfig,
        GlobalConfig, LmStudioConnectorConfig, OllamaConnectorConfig,
        OpenAiCompatibleConnectorConfig, OpenRouterConnectorConfig,
    },
    errors::{ResultContext as Context, SfumatoResult as Result},
    repositories::GlobalConfigRepository,
    secrets::{SecretStore, SecretValue},
    sfumato_bail as bail,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectorPreset {
    Ollama,
    Lmstudio,
    Openrouter,
    Anthropic,
    Codex,
}

impl ConnectorPreset {
    /// Every preset, in presentation order.
    ///
    /// Frontends iterate this instead of hardcoding a subset, so a new preset
    /// reaches the CLI, the interactive installer, and the TUI at once.
    pub const ALL: [Self; 5] = [
        Self::Ollama,
        Self::Lmstudio,
        Self::Openrouter,
        Self::Anthropic,
        Self::Codex,
    ];

    /// Stable preset identifier used by presentation and automation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ollama => "ollama",
            Self::Lmstudio => "lmstudio",
            Self::Openrouter => "openrouter",
            Self::Anthropic => "anthropic",
            Self::Codex => "codex",
        }
    }

    /// Connector kind this preset configures.
    pub const fn kind(self) -> &'static str {
        match self {
            Self::Ollama => "ollama",
            Self::Lmstudio => "lmstudio",
            Self::Openrouter => "openrouter",
            Self::Anthropic => "anthropic",
            Self::Codex => "codex_app_server",
        }
    }

    /// Connector name used when the caller supplies none.
    pub const fn default_connector_name(self) -> &'static str {
        self.as_str()
    }

    /// Provider model identifier suggested for a first text profile.
    pub const fn default_text_model(self) -> &'static str {
        match self {
            Self::Ollama => "llama3.2",
            Self::Lmstudio => "qwen2.5-7b-instruct",
            Self::Openrouter => "openai/gpt-4o-mini",
            Self::Anthropic => "claude-opus-5",
            // Codex resolves `default` against its own authenticated catalog.
            Self::Codex => "default",
        }
    }

    /// Model-profile name suggested when this preset backs the first text profile.
    pub const fn default_text_profile_name(self) -> &'static str {
        match self {
            Self::Ollama => "local-text",
            Self::Lmstudio => "lmstudio-text",
            Self::Openrouter => "cloud-text",
            Self::Anthropic => "anthropic-text",
            Self::Codex => "codex-text",
        }
    }

    /// Whether this preset accepts an environment-variable credential reference.
    pub const fn accepts_api_key_env(self) -> bool {
        match self {
            Self::Ollama | Self::Lmstudio | Self::Openrouter | Self::Anthropic => true,
            Self::Codex => false,
        }
    }

    /// Short transport description for preset listings.
    pub const fn transport_summary(self) -> &'static str {
        match self {
            Self::Ollama => "OpenAI-compatible HTTP on localhost:11434",
            Self::Lmstudio => "OpenAI-compatible HTTP on localhost:1234",
            Self::Openrouter => "OpenAI-compatible HTTP plus a native video API",
            Self::Anthropic => "Native Anthropic Messages API",
            Self::Codex => "Codex App Server JSON-RPC over stdio",
        }
    }

    /// Short authentication description for preset listings.
    pub const fn auth_summary(self) -> &'static str {
        match self {
            Self::Ollama => "None by default",
            Self::Lmstudio => "None by default; optional server key",
            Self::Openrouter => "OS keyring, or an environment variable",
            Self::Anthropic => "OS keyring, or an environment variable",
            Self::Codex => "Owned by `codex login`",
        }
    }

    /// Builds this preset's connector configuration without persisting it.
    ///
    /// Callers that only need a configuration value — the interactive installer
    /// and the TUI setup form — use this instead of [`ConnectorService::setup`],
    /// which also writes the global configuration.
    pub fn into_config(self, name: &str, api_key_env: Option<&str>) -> Result<ConnectorConfig> {
        if api_key_env.is_some() && !self.accepts_api_key_env() {
            bail!(
                "The {} preset manages its own authentication and does not accept --api-key-env",
                self.as_str()
            );
        }
        let environment_credential = |variable: &str| {
            SecretRef::environment(variable)
                .context("Invalid connector credential environment reference")
        };
        Ok(match self {
            Self::Ollama => ConnectorConfig::Ollama(OllamaConnectorConfig {
                transport: OpenAiCompatibleConnectorConfig {
                    base_url: "http://localhost:11434/v1".to_string(),
                    credential: api_key_env.map(environment_credential).transpose()?,
                    headers: BTreeMap::new(),
                },
                native_base_url: "http://localhost:11434".to_string(),
            }),
            Self::Lmstudio => ConnectorConfig::LmStudio(LmStudioConnectorConfig {
                transport: OpenAiCompatibleConnectorConfig {
                    base_url: "http://localhost:1234/v1".to_string(),
                    credential: api_key_env.map(environment_credential).transpose()?,
                    headers: BTreeMap::new(),
                },
                native_base_url: "http://localhost:1234".to_string(),
            }),
            Self::Openrouter => ConnectorConfig::OpenRouter(OpenRouterConnectorConfig {
                transport: OpenAiCompatibleConnectorConfig {
                    base_url: "https://openrouter.ai/api/v1".to_string(),
                    credential: Some(match api_key_env {
                        Some(variable) => environment_credential(variable)?,
                        None => SecretRef::stored(&format!("connector/{name}"))
                            .context("Invalid stored connector credential reference")?,
                    }),
                    headers: BTreeMap::new(),
                },
            }),
            Self::Anthropic => ConnectorConfig::Anthropic(AnthropicConnectorConfig {
                base_url: "https://api.anthropic.com/v1".to_string(),
                credential: Some(match api_key_env {
                    Some(variable) => environment_credential(variable)?,
                    None => SecretRef::stored(&format!("connector/{name}"))
                        .context("Invalid stored connector credential reference")?,
                }),
                headers: BTreeMap::new(),
            }),
            Self::Codex => ConnectorConfig::CodexAppServer(CodexAppServerConnectorConfig {
                executable: "codex".into(),
            }),
        })
    }
}

impl std::fmt::Display for ConnectorPreset {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::str::FromStr for ConnectorPreset {
    type Err = crate::errors::SfumatoError;

    fn from_str(value: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|preset| preset.as_str() == value)
            .ok_or_else(|| {
                let available = Self::ALL
                    .into_iter()
                    .map(Self::as_str)
                    .collect::<Vec<_>>()
                    .join(", ");
                crate::errors::SfumatoError::config(format_args!(
                    "Unknown connector preset '{value}'. Available presets: {available}"
                ))
            })
    }
}

pub struct ConnectorService {
    config: GlobalConfig,
    revision: String,
    repository: Arc<dyn GlobalConfigRepository>,
    secrets: Arc<dyn SecretStore>,
}

#[derive(Clone, Debug)]
pub struct ConnectorSummary {
    pub name: String,
    pub kind: String,
    pub target: String,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ConnectorDetails {
    pub kind: String,
    pub base_url: Option<String>,
    pub executable: Option<String>,
    pub credential: Option<String>,
    pub headers: BTreeMap<String, String>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ConnectorAuthStatus {
    pub name: String,
    pub credential: Option<String>,
    pub available: bool,
    pub managed_externally: bool,
}

impl ConnectorService {
    pub fn new(
        repository: Arc<dyn GlobalConfigRepository>,
        secrets: Arc<dyn SecretStore>,
    ) -> Result<Self> {
        let snapshot = repository.load_snapshot()?;
        Ok(Self {
            config: snapshot.value,
            revision: snapshot.revision,
            repository,
            secrets,
        })
    }

    pub fn list(&self) -> Vec<ConnectorSummary> {
        self.config
            .connectors
            .iter()
            .map(|(name, connector)| ConnectorSummary {
                name: name.clone(),
                kind: connector.kind().to_string(),
                target: connector.target(),
            })
            .collect()
    }

    pub fn show(&self, name: &str) -> Result<ConnectorDetails> {
        let connector = self
            .config
            .connectors
            .get(name)
            .with_context(|| format!("Connector '{name}' was not found"))?;
        let (base_url, executable, credential, headers) = match connector {
            ConnectorConfig::OpenAiCompatible(connector) => connector_details(connector),
            ConnectorConfig::OpenRouter(connector) => connector_details(&connector.transport),
            ConnectorConfig::Ollama(connector) => connector_details(&connector.transport),
            ConnectorConfig::LmStudio(connector) => connector_details(&connector.transport),
            ConnectorConfig::Anthropic(connector) => {
                connector_details(&OpenAiCompatibleConnectorConfig {
                    base_url: connector.base_url.clone(),
                    credential: connector.credential.clone(),
                    headers: connector.headers.clone(),
                })
            }
            ConnectorConfig::CodexAppServer(connector) => (
                None,
                Some(connector.executable.display().to_string()),
                None,
                BTreeMap::new(),
            ),
        };
        Ok(ConnectorDetails {
            kind: connector.kind().to_string(),
            base_url,
            executable,
            credential,
            headers,
        })
    }

    pub fn setup(
        &mut self,
        preset: ConnectorPreset,
        name: Option<String>,
        api_key_env: Option<String>,
    ) -> Result<ConnectorSummary> {
        let name = name.unwrap_or_else(|| preset.default_connector_name().to_string());
        let connector = preset.into_config(&name, api_key_env.as_deref())?;
        let kind = connector.kind().to_string();
        let target = connector.target();
        match self.config.connectors.entry(name.clone()) {
            Entry::Vacant(entry) => {
                entry.insert(connector);
            }
            Entry::Occupied(mut entry) => {
                entry.insert(connector);
            }
        }
        self.revision = self
            .repository
            .save_if_revision(&self.config, &self.revision)?;
        Ok(ConnectorSummary { name, kind, target })
    }

    pub async fn login(&mut self, name: &str, secret: SecretValue) -> Result<ConnectorAuthStatus> {
        let connector = self
            .config
            .connectors
            .get(name)
            .with_context(|| format!("Connector '{name}' was not found"))?;
        if let ConnectorAuth::External {
            owner,
            login_command,
            ..
        } = connector.auth()
        {
            bail!("{owner} manages its own authentication; run `{login_command}`");
        }
        let reference = SecretRef::stored(&format!("connector/{name}"))
            .context("Invalid stored connector credential reference")?;
        self.secrets.save(&reference, secret).await?;
        self.config
            .connectors
            .get_mut(name)
            .expect("connector existence checked above")
            .set_managed_credential(Some(reference.clone()))?;
        self.revision = self
            .repository
            .save_if_revision(&self.config, &self.revision)?;
        Ok(ConnectorAuthStatus {
            name: name.to_string(),
            credential: Some(reference.to_string()),
            available: true,
            managed_externally: false,
        })
    }

    pub async fn auth_status(&self, name: &str) -> Result<ConnectorAuthStatus> {
        let connector = self
            .config
            .connectors
            .get(name)
            .with_context(|| format!("Connector '{name}' was not found"))?;
        let (credential, available, managed_externally) = match connector.auth() {
            ConnectorAuth::External { .. } => (None, false, true),
            ConnectorAuth::Managed(credential) => {
                let available = match credential {
                    Some(reference) => self.secrets.exists(reference).await?,
                    None => false,
                };
                (credential.map(ToString::to_string), available, false)
            }
        };
        Ok(ConnectorAuthStatus {
            name: name.to_string(),
            credential,
            available,
            managed_externally,
        })
    }

    pub async fn logout(&mut self, name: &str) -> Result<ConnectorAuthStatus> {
        let connector = self
            .config
            .connectors
            .get(name)
            .with_context(|| format!("Connector '{name}' was not found"))?;
        let previous = match connector.auth() {
            ConnectorAuth::External {
                owner,
                logout_command,
                ..
            } => bail!("{owner} manages its own authentication; run `{logout_command}`"),
            ConnectorAuth::Managed(credential) => credential.cloned(),
        };
        self.config
            .connectors
            .get_mut(name)
            .expect("connector existence checked above")
            .set_managed_credential(None)?;
        self.revision = self
            .repository
            .save_if_revision(&self.config, &self.revision)?;
        if let Some(reference) = previous.filter(|reference| reference.scheme() == "stored") {
            self.secrets.delete(&reference).await?;
        }
        Ok(ConnectorAuthStatus {
            name: name.to_string(),
            credential: None,
            available: false,
            managed_externally: false,
        })
    }
}

fn connector_details(
    connector: &OpenAiCompatibleConnectorConfig,
) -> (
    Option<String>,
    Option<String>,
    Option<String>,
    BTreeMap<String, String>,
) {
    (
        Some(connector.base_url.clone()),
        None,
        connector.credential.as_ref().map(ToString::to_string),
        connector
            .headers
            .iter()
            .map(|(name, value)| {
                let normalized = name.to_ascii_lowercase();
                let sensitive = ["authorization", "key", "token", "secret", "cookie"]
                    .iter()
                    .any(|fragment| normalized.contains(fragment));
                (
                    name.clone(),
                    if sensitive {
                        "[REDACTED]".into()
                    } else {
                        value.clone()
                    },
                )
            })
            .collect(),
    )
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../tests/unit/connectors.rs"]
mod tests;
