use super::*;
use crate::repositories::GlobalConfigRepository;
use std::sync::{Arc, Mutex};

struct MemoryGlobal(Mutex<GlobalConfig>);

impl GlobalConfigRepository for MemoryGlobal {
    fn exists(&self) -> bool {
        true
    }

    fn load(&self) -> Result<GlobalConfig> {
        Ok(self.0.lock().unwrap().clone())
    }

    fn save(&self, config: &GlobalConfig) -> Result<()> {
        *self.0.lock().unwrap() = config.clone();
        Ok(())
    }
}

#[test]
fn default_config_has_openai_compatible_connector_presets() {
    let repository = Arc::new(MemoryGlobal(Mutex::new(GlobalConfig::default_config())));
    let service = ConnectorService::new(repository).unwrap();

    assert_eq!(
        service.config.connectors.get("ollama").unwrap().base_url,
        "http://localhost:11434/v1"
    );
    assert_eq!(
        service
            .config
            .connectors
            .get("openrouter")
            .unwrap()
            .base_url,
        "https://openrouter.ai/api/v1"
    );
}
