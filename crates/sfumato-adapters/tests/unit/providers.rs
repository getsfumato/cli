use super::*;

use sfumato_core::{
    config::OpenAiCompatibleConnectorConfig,
    secrets::{SecretResolver, SecretValue},
};
use std::collections::BTreeMap;

struct TestSecrets;

#[async_trait]
impl SecretResolver for TestSecrets {
    async fn resolve(
        &self,
        _reference: &sfumato_core::config::SecretRef,
    ) -> SfumatoResult<SecretValue> {
        Ok(SecretValue::new("resolved-secret".to_string()))
    }
}

fn generic(base_url: &str) -> ConnectorConfig {
    ConnectorConfig::OpenAiCompatible(OpenAiCompatibleConnectorConfig {
        base_url: base_url.to_string(),
        credential: None,
        headers: BTreeMap::new(),
    })
}

#[test]
fn upgrades_legacy_lmstudio_urls_to_the_native_connector() {
    for base_url in [
        "http://localhost:1234/v1",
        "http://127.0.0.1:1234/v1",
        "http://localhost:1234/v1/",
    ] {
        let NativeConnector::LmStudio(config) = native_connector(&generic(base_url)) else {
            panic!("'{base_url}' should be recognized as LM Studio");
        };
        assert_eq!(
            config.native_base_url,
            base_url.trim_end_matches('/').trim_end_matches("/v1")
        );
    }
}

#[test]
fn keeps_recognizing_legacy_openrouter_and_ollama_urls() {
    assert!(matches!(
        native_connector(&generic("https://openrouter.ai/api/v1")),
        NativeConnector::OpenRouter(_)
    ));
    assert!(matches!(
        native_connector(&generic("http://localhost:11434/v1")),
        NativeConnector::Ollama(_)
    ));
}

#[test]
fn leaves_unrecognized_openai_compatible_urls_generic() {
    assert!(matches!(
        native_connector(&generic("https://api.example.com/v1")),
        NativeConnector::Generic
    ));
}

#[test]
fn never_promotes_a_neighbouring_local_port_to_a_native_adapter() {
    // ADR-0008 only allows promotion on an unambiguous URL, and a substring test
    // for "localhost:1234" also matches 12340-12349 — 12345 is a common dev port.
    for base_url in [
        "http://localhost:12345/v1",
        "http://127.0.0.1:12345/v1",
        "http://localhost:114340/v1",
        "http://lmstudio.localhost:1234.example.com/v1",
    ] {
        assert!(
            matches!(
                native_connector(&generic(base_url)),
                NativeConnector::Generic
            ),
            "'{base_url}' must stay generic"
        );
    }
}

#[test]
fn recognizes_loopback_hosts_only_on_the_exact_port() {
    for base_url in [
        "http://localhost:1234/v1",
        "http://127.0.0.1:1234",
        "http://[::1]:1234/v1",
    ] {
        assert!(is_local_port(base_url, 1234), "'{base_url}' is local:1234");
    }
    for base_url in [
        "http://localhost:12345/v1",
        "http://localhost/v1",
        "https://example.com:1234/v1",
    ] {
        assert!(!is_local_port(base_url, 1234), "'{base_url}' is not");
    }
}

#[test]
fn advertises_only_capabilities_each_connector_implements() {
    let factory = AdapterProviderFactory::new(Arc::new(TestSecrets));

    let lmstudio = factory.capabilities(&generic("http://localhost:1234/v1"));
    assert_eq!(lmstudio.kind, "lmstudio");
    assert!(
        lmstudio
            .features
            .contains(&ConnectorCapability::ModelCatalog)
    );
    assert!(
        lmstudio
            .features
            .contains(&ConnectorCapability::RuntimeStatus)
    );
    // LM Studio exposes no video API, so it must not advertise one.
    assert!(
        !lmstudio
            .features
            .contains(&ConnectorCapability::VideoGeneration)
    );
    // No port method invokes a per-model detail operation, so advertising
    // `ModelDetails` would promise something no frontend can call.
    assert!(
        !lmstudio
            .features
            .contains(&ConnectorCapability::ModelDetails)
    );

    let generic_connector = factory.capabilities(&generic("https://api.example.com/v1"));
    assert_eq!(generic_connector.kind, "openai_compatible");
    assert!(generic_connector.features.is_empty());
}
