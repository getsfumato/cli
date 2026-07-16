use std::{
    collections::{BTreeMap, btree_map::Entry},
    sync::Arc,
};

use sfumato_domain::SecretRef;

use crate::{
    config::{GlobalConfig, OpenAiCompatibleConnectorConfig},
    errors::{ResultContext as Context, SfumatoResult as Result},
    repositories::GlobalConfigRepository,
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

impl ConnectorService {
    pub fn new(repository: Arc<dyn GlobalConfigRepository>) -> Result<Self> {
        let snapshot = repository.load_snapshot()?;
        Ok(Self {
            config: snapshot.value,
            revision: snapshot.revision,
            repository,
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
        api_key_env: String,
    ) -> Result<ConnectorSummary> {
        let (default_name, connector) = match preset {
            ConnectorPreset::Ollama => (
                "ollama",
                OpenAiCompatibleConnectorConfig {
                    base_url: "http://localhost:11434/v1".to_string(),
                    credential: None,
                    headers: BTreeMap::new(),
                },
            ),
            ConnectorPreset::Openrouter => (
                "openrouter",
                OpenAiCompatibleConnectorConfig {
                    base_url: "https://openrouter.ai/api/v1".to_string(),
                    credential: Some(
                        SecretRef::environment(&api_key_env)
                            .context("Invalid connector credential environment reference")?,
                    ),
                    headers: BTreeMap::new(),
                },
            ),
        };
        let name = name.unwrap_or_else(|| default_name.to_string());
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
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../tests/unit/connectors.rs"]
mod tests;
