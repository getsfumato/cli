use std::collections::{BTreeMap, btree_map::Entry};

use anyhow::{Context, Result};

use crate::{
    cli::ConnectorPreset,
    config::{GlobalConfig, OpenAiCompatibleConnectorConfig, user_config_path, write_toml},
};

#[derive(Debug)]
pub struct ConnectorService {
    config: GlobalConfig,
}

impl ConnectorService {
    pub fn load() -> Result<Self> {
        Ok(Self {
            config: GlobalConfig::load()?,
        })
    }

    pub fn list(&self) {
        for (name, connector) in &self.config.connectors {
            println!("{name}\t{}", connector.base_url);
        }
    }

    pub fn show(&self, name: &str) -> Result<String> {
        let connector = self
            .config
            .connectors
            .get(name)
            .with_context(|| format!("Connector '{name}' was not found"))?;
        toml::to_string_pretty(connector).context("Could not render connector config")
    }

    pub fn setup(
        &mut self,
        preset: ConnectorPreset,
        name: Option<String>,
        api_key_env: String,
    ) -> Result<()> {
        let (default_name, connector) = match preset {
            ConnectorPreset::Ollama => (
                "ollama",
                OpenAiCompatibleConnectorConfig {
                    base_url: "http://localhost:11434/v1".to_string(),
                    api_key: Some("ollama".to_string()),
                    api_key_env: None,
                    headers: BTreeMap::new(),
                },
            ),
            ConnectorPreset::Openrouter => (
                "openrouter",
                OpenAiCompatibleConnectorConfig {
                    base_url: "https://openrouter.ai/api/v1".to_string(),
                    api_key: None,
                    api_key_env: Some(api_key_env),
                    headers: BTreeMap::new(),
                },
            ),
        };
        let name = name.unwrap_or_else(|| default_name.to_string());
        match self.config.connectors.entry(name.clone()) {
            Entry::Vacant(entry) => {
                entry.insert(connector);
            }
            Entry::Occupied(mut entry) => {
                entry.insert(connector);
            }
        }
        let path = user_config_path().context("Could not find user configuration path")?;
        write_toml(&path, &self.config)?;
        println!("Configured OpenAI-compatible connector '{name}'");
        Ok(())
    }
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../tests/unit/connectors.rs"]
mod tests;
