use std::{
    collections::{BTreeMap, btree_map::Entry},
    sync::Arc,
};

use sfumato_domain::SecretRef;

use crate::{
    config::{GlobalConfig, OpenAiCompatibleConnectorConfig},
    errors::{ResultContext as Context, SfumatoResult as Result},
    repositories::GlobalConfigRepository,
    secrets::{SecretStore, SecretValue},
};

#[derive(Clone, Copy, Debug)]
pub enum ConnectorPreset {
    Ollama,
    Openrouter,
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
    pub base_url: String,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ConnectorDetails {
    pub base_url: String,
    pub credential: Option<String>,
    pub headers: BTreeMap<String, String>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ConnectorAuthStatus {
    pub name: String,
    pub credential: Option<String>,
    pub available: bool,
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
                base_url: connector.base_url.clone(),
            })
            .collect()
    }

    pub fn show(&self, name: &str) -> Result<ConnectorDetails> {
        let connector = self
            .config
            .connectors
            .get(name)
            .with_context(|| format!("Connector '{name}' was not found"))?;
        Ok(ConnectorDetails {
            base_url: connector.base_url.clone(),
            credential: connector.credential.as_ref().map(ToString::to_string),
            headers: connector
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
                            "[REDACTED]".to_string()
                        } else {
                            value.clone()
                        },
                    )
                })
                .collect(),
        })
    }

    pub fn setup(
        &mut self,
        preset: ConnectorPreset,
        name: Option<String>,
        api_key_env: Option<String>,
    ) -> Result<ConnectorSummary> {
        let default_name = match preset {
            ConnectorPreset::Ollama => "ollama",
            ConnectorPreset::Openrouter => "openrouter",
        };
        let name = name.unwrap_or_else(|| default_name.to_string());
        let connector = match preset {
            ConnectorPreset::Ollama => OpenAiCompatibleConnectorConfig {
                base_url: "http://localhost:11434/v1".to_string(),
                credential: None,
                headers: BTreeMap::new(),
            },
            ConnectorPreset::Openrouter => OpenAiCompatibleConnectorConfig {
                base_url: "https://openrouter.ai/api/v1".to_string(),
                credential: Some(match api_key_env {
                    Some(variable) => SecretRef::environment(&variable)
                        .context("Invalid connector credential environment reference")?,
                    None => SecretRef::stored(&format!("connector/{name}"))
                        .context("Invalid stored connector credential reference")?,
                }),
                headers: BTreeMap::new(),
            },
        };
        let base_url = connector.base_url.clone();
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
        Ok(ConnectorSummary { name, base_url })
    }

    pub async fn login(&mut self, name: &str, secret: SecretValue) -> Result<ConnectorAuthStatus> {
        self.config
            .connectors
            .get(name)
            .with_context(|| format!("Connector '{name}' was not found"))?;
        let reference = SecretRef::stored(&format!("connector/{name}"))
            .context("Invalid stored connector credential reference")?;
        self.secrets.save(&reference, secret).await?;
        self.config
            .connectors
            .get_mut(name)
            .expect("connector existence checked above")
            .credential = Some(reference.clone());
        self.revision = self
            .repository
            .save_if_revision(&self.config, &self.revision)?;
        Ok(ConnectorAuthStatus {
            name: name.to_string(),
            credential: Some(reference.to_string()),
            available: true,
        })
    }

    pub async fn auth_status(&self, name: &str) -> Result<ConnectorAuthStatus> {
        let connector = self
            .config
            .connectors
            .get(name)
            .with_context(|| format!("Connector '{name}' was not found"))?;
        let available = match &connector.credential {
            Some(reference) => self.secrets.exists(reference).await?,
            None => false,
        };
        Ok(ConnectorAuthStatus {
            name: name.to_string(),
            credential: connector.credential.as_ref().map(ToString::to_string),
            available,
        })
    }

    pub async fn logout(&mut self, name: &str) -> Result<ConnectorAuthStatus> {
        let previous = self
            .config
            .connectors
            .get(name)
            .with_context(|| format!("Connector '{name}' was not found"))?
            .credential
            .clone();
        self.config
            .connectors
            .get_mut(name)
            .expect("connector existence checked above")
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
        })
    }
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../tests/unit/connectors.rs"]
mod tests;
