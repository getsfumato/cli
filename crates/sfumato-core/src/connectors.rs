use std::{
    collections::{BTreeMap, btree_map::Entry},
    sync::Arc,
};

use anyhow::{Context, Result};
use sfumato_domain::SecretRef;

use crate::{
    config::{GlobalConfig, OpenAiCompatibleConnectorConfig},
    repositories::GlobalConfigRepository,
};

#[derive(Clone, Copy, Debug)]
pub enum ConnectorPreset {
    Ollama,
    Openrouter,
}

pub struct ConnectorService {
    config: GlobalConfig,
    repository: Arc<dyn GlobalConfigRepository>,
}

#[derive(Clone, Debug)]
pub struct ConnectorSummary {
    pub name: String,
    pub base_url: String,
}

impl ConnectorService {
    pub fn new(repository: Arc<dyn GlobalConfigRepository>) -> Result<Self> {
        Ok(Self {
            config: repository.load()?,
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

    pub fn show(&self, name: &str) -> Result<OpenAiCompatibleConnectorConfig> {
        self.config
            .connectors
            .get(name)
            .cloned()
            .with_context(|| format!("Connector '{name}' was not found"))
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
                    credential: Some(SecretRef::environment(&api_key_env)?),
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
        self.repository.save(&self.config)?;
        Ok(ConnectorSummary { name, base_url })
    }
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../tests/unit/connectors.rs"]
mod tests;
