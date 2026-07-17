use super::*;
use crate::repositories::GlobalConfigRepository;
use crate::secrets::{SecretResolver, SecretStore, SecretValue};
use async_trait::async_trait;
use sfumato_domain::SecretRef;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

struct MemoryGlobal(Mutex<GlobalConfig>);

#[derive(Default)]
struct MemorySecrets(Mutex<BTreeMap<String, String>>);

#[async_trait]
impl SecretResolver for MemorySecrets {
    async fn resolve(&self, reference: &SecretRef) -> crate::errors::SfumatoResult<SecretValue> {
        self.0
            .lock()
            .unwrap()
            .get(reference.as_str())
            .cloned()
            .map(SecretValue::new)
            .ok_or_else(|| crate::errors::SfumatoError::not_found("credential missing"))
    }
}

#[async_trait]
impl SecretStore for MemorySecrets {
    async fn save(
        &self,
        reference: &SecretRef,
        value: SecretValue,
    ) -> crate::errors::SfumatoResult<()> {
        self.0
            .lock()
            .unwrap()
            .insert(reference.to_string(), value.expose().to_string());
        Ok(())
    }

    async fn exists(&self, reference: &SecretRef) -> crate::errors::SfumatoResult<bool> {
        Ok(self.0.lock().unwrap().contains_key(reference.as_str()))
    }

    async fn delete(&self, reference: &SecretRef) -> crate::errors::SfumatoResult<()> {
        self.0.lock().unwrap().remove(reference.as_str());
        Ok(())
    }
}

impl GlobalConfigRepository for MemoryGlobal {
    fn exists(&self) -> bool {
        true
    }

    fn load(&self) -> crate::errors::SfumatoResult<GlobalConfig> {
        Ok(self.0.lock().unwrap().clone())
    }

    fn save(&self, config: &GlobalConfig) -> crate::errors::SfumatoResult<()> {
        *self.0.lock().unwrap() = config.clone();
        Ok(())
    }
}

#[test]
fn default_config_has_openai_compatible_connector_presets() {
    let repository = Arc::new(MemoryGlobal(Mutex::new(GlobalConfig::default_config())));
    let service = ConnectorService::new(repository, Arc::new(MemorySecrets::default())).unwrap();

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

#[tokio::test]
async fn login_selects_secure_storage_and_logout_removes_the_secret() {
    let repository = Arc::new(MemoryGlobal(Mutex::new(GlobalConfig::default_config())));
    let secrets = Arc::new(MemorySecrets::default());
    let mut service = ConnectorService::new(repository.clone(), secrets).unwrap();

    let logged_in = service
        .login("openrouter", SecretValue::new("private-key".to_string()))
        .await
        .unwrap();
    assert!(logged_in.available);
    assert_eq!(
        logged_in.credential.as_deref(),
        Some("stored:connector/openrouter")
    );
    assert!(service.auth_status("openrouter").await.unwrap().available);
    assert_eq!(
        repository.load().unwrap().connectors["openrouter"]
            .credential
            .as_ref()
            .unwrap()
            .as_str(),
        "stored:connector/openrouter"
    );

    service.logout("openrouter").await.unwrap();
    assert!(!service.auth_status("openrouter").await.unwrap().available);
    assert!(
        repository.load().unwrap().connectors["openrouter"]
            .credential
            .is_none()
    );
}

#[test]
fn openrouter_setup_defaults_to_secure_storage_but_allows_explicit_environment_refs() {
    let repository = Arc::new(MemoryGlobal(Mutex::new(GlobalConfig::default_config())));
    let mut service =
        ConnectorService::new(repository, Arc::new(MemorySecrets::default())).unwrap();

    service
        .setup(ConnectorPreset::Openrouter, None, None)
        .unwrap();
    assert_eq!(
        service.config.connectors["openrouter"]
            .credential
            .as_ref()
            .unwrap()
            .as_str(),
        "stored:connector/openrouter"
    );

    service
        .setup(
            ConnectorPreset::Openrouter,
            Some("ci-openrouter".to_string()),
            Some("CI_OPENROUTER_KEY".to_string()),
        )
        .unwrap();
    assert_eq!(
        service.config.connectors["ci-openrouter"]
            .credential
            .as_ref()
            .unwrap()
            .as_str(),
        "env:CI_OPENROUTER_KEY"
    );
}
