use std::{
    collections::{BTreeMap, btree_map::Entry},
    sync::Arc,
};

use sfumato_domain::SecretRef;

use crate::{
    config::{
        CodexAppServerConnectorConfig, ConnectorConfig, GlobalConfig, OllamaConnectorConfig,
        OpenAiCompatibleConnectorConfig, OpenRouterConnectorConfig,
    },
    errors::{ResultContext as Context, SfumatoResult as Result},
    repositories::GlobalConfigRepository,
    secrets::{SecretStore, SecretValue},
    sfumato_bail as bail,
};

#[derive(Clone, Copy, Debug)]
pub enum ConnectorPreset {
    Ollama,
    Openrouter,
    Codex,
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
        if matches!(preset, ConnectorPreset::Codex) && api_key_env.is_some() {
            bail!("Codex CLI manages its own authentication and does not accept --api-key-env");
        }
        let default_name = match preset {
            ConnectorPreset::Ollama => "ollama",
            ConnectorPreset::Openrouter => "openrouter",
            ConnectorPreset::Codex => "codex",
        };
        let name = name.unwrap_or_else(|| default_name.to_string());
        let connector = match preset {
            ConnectorPreset::Ollama => ConnectorConfig::Ollama(OllamaConnectorConfig {
                transport: OpenAiCompatibleConnectorConfig {
                    base_url: "http://localhost:11434/v1".to_string(),
                    credential: None,
                    headers: BTreeMap::new(),
                },
                native_base_url: "http://localhost:11434".to_string(),
            }),
            ConnectorPreset::Openrouter => ConnectorConfig::OpenRouter(OpenRouterConnectorConfig {
                transport: OpenAiCompatibleConnectorConfig {
                    base_url: "https://openrouter.ai/api/v1".to_string(),
                    credential: Some(match api_key_env {
                        Some(variable) => SecretRef::environment(&variable)
                            .context("Invalid connector credential environment reference")?,
                        None => SecretRef::stored(&format!("connector/{name}"))
                            .context("Invalid stored connector credential reference")?,
                    }),
                    headers: BTreeMap::new(),
                },
            }),
            ConnectorPreset::Codex => {
                ConnectorConfig::CodexAppServer(CodexAppServerConnectorConfig {
                    executable: "codex".into(),
                })
            }
        };
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
        if matches!(connector, ConnectorConfig::CodexAppServer(_)) {
            bail!("Codex CLI manages its own authentication; run `codex login`");
        }
        let reference = SecretRef::stored(&format!("connector/{name}"))
            .context("Invalid stored connector credential reference")?;
        self.secrets.save(&reference, secret).await?;
        self.config
            .connectors
            .get_mut(name)
            .expect("connector existence checked above")
            .openai_compatible_mut()
            .expect("connector kind checked above")
            .credential = Some(reference.clone());
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
        let (credential, available, managed_externally) =
            if matches!(connector, ConnectorConfig::CodexAppServer(_)) {
                (None, false, true)
            } else {
                let transport = connector
                    .openai_compatible()
                    .expect("all non-Codex connectors use the shared transport");
                let available = match &transport.credential {
                    Some(reference) => self.secrets.exists(reference).await?,
                    None => false,
                };
                (
                    transport.credential.as_ref().map(ToString::to_string),
                    available,
                    false,
                )
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
        if matches!(connector, ConnectorConfig::CodexAppServer(_)) {
            bail!("Codex CLI manages its own authentication; run `codex logout`");
        }
        let previous = connector
            .openai_compatible()
            .expect("all non-Codex connectors use the shared transport")
            .credential
            .clone();
        self.config
            .connectors
            .get_mut(name)
            .expect("connector existence checked above")
            .openai_compatible_mut()
            .expect("connector kind checked above")
            .credential = None;
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
