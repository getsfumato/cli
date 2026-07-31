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
        service.config.connectors["ollama"]
            .openai_compatible()
            .unwrap()
            .base_url,
        "http://localhost:11434/v1"
    );
    assert_eq!(
        service
            .config
            .connectors
            .get("openrouter")
            .unwrap()
            .openai_compatible()
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
            .openai_compatible()
            .unwrap()
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
            .openai_compatible()
            .unwrap()
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
            .openai_compatible()
            .unwrap()
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
            .openai_compatible()
            .unwrap()
            .credential
            .as_ref()
            .unwrap()
            .as_str(),
        "env:CI_OPENROUTER_KEY"
    );
}

#[tokio::test]
async fn codex_setup_delegates_authentication_to_codex_app_server() {
    let repository = Arc::new(MemoryGlobal(Mutex::new(GlobalConfig::default_config())));
    let mut service =
        ConnectorService::new(repository, Arc::new(MemorySecrets::default())).unwrap();

    let configured = service.setup(ConnectorPreset::Codex, None, None).unwrap();

    assert_eq!(configured.kind, "codex_app_server");
    assert_eq!(configured.target, "codex");
    let status = service.auth_status("codex").await.unwrap();
    assert!(status.managed_externally);
    assert!(!status.available);
    assert!(
        service
            .login("codex", SecretValue::new("not-used".to_string()))
            .await
            .unwrap_err()
            .to_string()
            .contains("codex login")
    );
}

#[test]
fn preset_metadata_covers_every_variant() {
    for preset in ConnectorPreset::ALL {
        assert!(!preset.as_str().is_empty());
        assert!(!preset.kind().is_empty());
        assert!(!preset.default_connector_name().is_empty());
        assert!(!preset.default_model().is_empty());
        assert!(!preset.default_profile_name().is_empty());
        assert!(!preset.transport_summary().is_empty());
        assert!(!preset.auth_summary().is_empty());

        assert_eq!(
            preset.as_str().parse::<ConnectorPreset>().unwrap(),
            preset,
            "every preset round-trips through its stable identifier"
        );
        assert_eq!(
            preset
                .into_config(preset.default_connector_name(), None)
                .unwrap()
                .kind(),
            preset.kind(),
            "the preset's advertised kind matches the configuration it builds"
        );
    }
}

#[test]
fn anthropic_first_profile_defaults_leave_the_adapter_budget_alone() {
    // A 4000-token cap is shared with thinking on the Messages API, so a fresh
    // profile carrying it returns no text at all on its first generation.
    assert_eq!(ConnectorPreset::Anthropic.default_text_max_tokens(), None);
    // The same connector warns that sampling parameters are inert.
    assert_eq!(ConnectorPreset::Anthropic.default_text_temperature(), None);
    assert!(ConnectorPreset::Anthropic.requires_stored_login());

    for preset in [ConnectorPreset::Ollama, ConnectorPreset::Lmstudio] {
        assert_eq!(preset.default_text_max_tokens(), Some(4000));
        assert_eq!(preset.default_text_temperature(), Some(0.4));
        assert!(!preset.requires_stored_login());
    }
    // Codex owns its own credential, so it never needs `connector login`.
    assert!(!ConnectorPreset::Codex.requires_stored_login());
    assert!(ConnectorPreset::Openrouter.requires_stored_login());
}

#[test]
fn unknown_preset_names_list_the_available_presets() {
    let error = "gemini".parse::<ConnectorPreset>().unwrap_err().to_string();

    assert!(error.contains("gemini"));
    for preset in ConnectorPreset::ALL {
        assert!(error.contains(preset.as_str()));
    }
}

#[test]
fn into_config_rejects_an_environment_credential_for_externally_managed_presets() {
    let error = ConnectorPreset::Codex
        .into_config("codex", Some("CODEX_KEY"))
        .expect_err("Codex owns its credential and cannot read one from the environment");

    assert!(error.to_string().contains("--api-key-env"));
}

#[test]
fn into_config_honours_an_environment_credential_for_local_presets() {
    // Ollama ships without a credential, but LM Studio-style optional server
    // keys are legitimate, so the flag has to be honoured rather than ignored.
    let default = ConnectorPreset::Ollama.into_config("ollama", None).unwrap();
    assert!(matches!(default.auth(), ConnectorAuth::Managed(None)));

    let configured = ConnectorPreset::Ollama
        .into_config("ollama", Some("OLLAMA_KEY"))
        .unwrap();
    let ConnectorAuth::Managed(Some(reference)) = configured.auth() else {
        panic!("an environment credential is Sfumato-managed");
    };
    assert_eq!(reference.as_str(), "env:OLLAMA_KEY");
}

#[tokio::test]
async fn login_stores_a_credential_for_a_connector_without_the_shared_transport() {
    // Anthropic is the first kind whose `openai_compatible()` returns `None`.
    // Before the capability-based auth guard, `login`/`auth_status`/`logout` all
    // reached `.expect("all non-Codex connectors use the shared transport")` and
    // panicked here. This test fails against that previous behavior.
    let repository = Arc::new(MemoryGlobal(Mutex::new(GlobalConfig::default_config())));
    let mut service =
        ConnectorService::new(repository.clone(), Arc::new(MemorySecrets::default())).unwrap();
    service
        .setup(ConnectorPreset::Anthropic, None, None)
        .unwrap();

    assert!(
        service.config.connectors["anthropic"]
            .openai_compatible()
            .is_none(),
        "the Messages API is not an OpenAI-compatible transport"
    );

    let logged_in = service
        .login("anthropic", SecretValue::new("api-key".to_string()))
        .await
        .unwrap();
    assert!(logged_in.available);
    assert!(!logged_in.managed_externally);
    assert_eq!(
        logged_in.credential.as_deref(),
        Some("stored:connector/anthropic")
    );
    assert!(service.auth_status("anthropic").await.unwrap().available);

    service.logout("anthropic").await.unwrap();
    assert!(!service.auth_status("anthropic").await.unwrap().available);
}

#[test]
fn anthropic_setup_targets_the_native_messages_api() {
    let repository = Arc::new(MemoryGlobal(Mutex::new(GlobalConfig::default_config())));
    let mut service =
        ConnectorService::new(repository, Arc::new(MemorySecrets::default())).unwrap();

    let configured = service
        .setup(ConnectorPreset::Anthropic, None, None)
        .unwrap();

    assert_eq!(configured.kind, "anthropic");
    assert_eq!(configured.target, "https://api.anthropic.com/v1");

    let details = service.show("anthropic").unwrap();
    assert_eq!(details.kind, "anthropic");
    assert_eq!(
        details.credential.as_deref(),
        Some("stored:connector/anthropic")
    );
}

#[test]
fn auth_distinguishes_managed_credentials_from_externally_owned_ones() {
    let repository = Arc::new(MemoryGlobal(Mutex::new(GlobalConfig::default_config())));
    let mut service =
        ConnectorService::new(repository, Arc::new(MemorySecrets::default())).unwrap();
    service.setup(ConnectorPreset::Codex, None, None).unwrap();

    assert!(matches!(
        service.config.connectors["ollama"].auth(),
        ConnectorAuth::Managed(None)
    ));

    let ConnectorAuth::Managed(Some(reference)) = service.config.connectors["openrouter"].auth()
    else {
        panic!("Sfumato manages OpenRouter credentials");
    };
    assert_eq!(reference.as_str(), "stored:connector/openrouter");

    let ConnectorAuth::External {
        owner,
        login_command,
        logout_command,
    } = service.config.connectors["codex"].auth()
    else {
        panic!("Codex App Server manages its own authentication");
    };
    assert_eq!(owner, "Codex CLI");
    assert_eq!(login_command, "codex login");
    assert_eq!(logout_command, "codex logout");
}

#[test]
fn set_managed_credential_rejects_externally_authenticated_connectors() {
    let mut connector = ConnectorConfig::CodexAppServer(CodexAppServerConnectorConfig {
        executable: "codex".into(),
    });

    let error = connector
        .set_managed_credential(None)
        .expect_err("Codex App Server connectors expose no Sfumato-managed credential");

    assert!(
        error
            .to_string()
            .contains("manage their own authentication")
    );
}

#[tokio::test]
async fn logout_reports_the_external_logout_command() {
    let repository = Arc::new(MemoryGlobal(Mutex::new(GlobalConfig::default_config())));
    let mut service =
        ConnectorService::new(repository, Arc::new(MemorySecrets::default())).unwrap();
    service.setup(ConnectorPreset::Codex, None, None).unwrap();

    let error = service
        .logout("codex")
        .await
        .expect_err("Sfumato cannot clear a Codex-owned credential");

    assert!(error.to_string().contains("codex logout"));
}

#[tokio::test]
async fn login_establishes_a_credential_for_a_connector_that_ships_without_one() {
    let repository = Arc::new(MemoryGlobal(Mutex::new(GlobalConfig::default_config())));
    let mut service =
        ConnectorService::new(repository.clone(), Arc::new(MemorySecrets::default())).unwrap();

    assert!(!service.auth_status("ollama").await.unwrap().available);

    let logged_in = service
        .login("ollama", SecretValue::new("local-key".to_string()))
        .await
        .unwrap();

    assert!(logged_in.available);
    assert!(!logged_in.managed_externally);
    assert_eq!(
        logged_in.credential.as_deref(),
        Some("stored:connector/ollama")
    );
    assert_eq!(
        repository.load().unwrap().connectors["ollama"]
            .openai_compatible()
            .unwrap()
            .credential
            .as_ref()
            .unwrap()
            .as_str(),
        "stored:connector/ollama"
    );
}
