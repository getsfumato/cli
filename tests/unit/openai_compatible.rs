use super::*;
use std::collections::BTreeMap;

fn profile(connector: &str) -> ModelProfile {
    ModelProfile {
        connector: connector.to_string(),
        model: "llama3.2".to_string(),
        capabilities: vec![crate::config::Capability::Text],
        options: BTreeMap::from([("max_tokens".to_string(), toml::Value::Integer(100))]),
    }
}

#[test]
fn ollama_and_openrouter_use_the_same_connector_implementation() {
    let ollama = OpenAiCompatibleConnector::new(
        "ollama".to_string(),
        OpenAiCompatibleConnectorConfig {
            base_url: "http://localhost:11434/v1".to_string(),
            api_key: Some("ollama".to_string()),
            api_key_env: None,
            headers: BTreeMap::new(),
        },
    )
    .unwrap();
    let openrouter = OpenAiCompatibleConnector::new(
        "openrouter".to_string(),
        OpenAiCompatibleConnectorConfig {
            base_url: "https://openrouter.ai/api/v1".to_string(),
            api_key: Some("test-key".to_string()),
            api_key_env: None,
            headers: BTreeMap::from([(
                "X-OpenRouter-Title".to_string(),
                "Sfumato CLI".to_string(),
            )]),
        },
    )
    .unwrap();

    assert_eq!(
        ollama.endpoint("chat/completions"),
        "http://localhost:11434/v1/chat/completions"
    );
    assert_eq!(
        openrouter.endpoint("chat/completions"),
        "https://openrouter.ai/api/v1/chat/completions"
    );
}

#[test]
fn serializes_chat_completion_from_model_profile() {
    let connector = OpenAiCompatibleTextProvider::new(
        "ollama".to_string(),
        OpenAiCompatibleConnectorConfig {
            base_url: "http://localhost:11434/v1".to_string(),
            api_key: Some("ollama".to_string()),
            api_key_env: None,
            headers: BTreeMap::new(),
        },
        profile("ollama"),
    )
    .unwrap();

    let body = connector.request_body(&TextGenerationRequest {
        system_prompt: "system".to_string(),
        user_prompt: "user".to_string(),
    });

    assert_eq!(body.model, "llama3.2");
    assert_eq!(body.max_tokens, 100);
    assert_eq!(body.messages.len(), 2);
}

#[test]
fn requires_an_api_key_source() {
    assert!(
        resolve_api_key(&OpenAiCompatibleConnectorConfig {
            base_url: "https://example.com/v1".to_string(),
            api_key: None,
            api_key_env: None,
            headers: BTreeMap::new(),
        })
        .is_err()
    );
}
