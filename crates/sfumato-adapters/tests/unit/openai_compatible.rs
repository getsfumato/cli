use super::*;
use serde_json::json;
use sfumato_core::{
    config::{ImageModelOptions, ModelOptions, TextModelOptions},
    providers::{ToolDefinition, ToolFunctionDefinition},
    secrets::{SecretResolver, SecretValue},
};
use std::sync::Arc;

struct TestSecrets;

#[async_trait]
impl SecretResolver for TestSecrets {
    async fn resolve(
        &self,
        _reference: &sfumato_core::config::SecretRef,
    ) -> sfumato_core::errors::SfumatoResult<SecretValue> {
        Ok(SecretValue::new("resolved-secret".to_string()))
    }
}

fn secrets() -> Arc<dyn SecretResolver> {
    Arc::new(TestSecrets)
}

fn connector() -> OpenAiCompatibleConnectorConfig {
    OpenAiCompatibleConnectorConfig {
        base_url: "http://localhost:11434/v1".to_string(),
        credential: None,
        headers: BTreeMap::new(),
    }
}

fn profile() -> ModelProfile {
    ModelProfile {
        connector: "ollama".to_string(),
        model: "gemma3:latest".to_string(),
        capabilities: vec![Capability::Text, Capability::Image],
        options: ModelOptions {
            text: TextModelOptions {
                temperature: Some(0.25),
                max_tokens: Some(8_192),
                max_tool_rounds: Some(6),
                top_p: Some(0.9),
                seed: Some(42),
            },
            image: ImageModelOptions {
                quality: Some("high".to_string()),
                background: Some("transparent".to_string()),
                size: Some("1536x1024".to_string()),
                aspect_ratio: Some("3:2".to_string()),
                output_format: Some("png".to_string()),
            },
            video: Default::default(),
        },
    }
}

#[test]
fn serializes_provider_neutral_transcript_tools_and_typed_text_options() {
    let provider =
        OpenAiCompatibleTextProvider::new("ollama".to_string(), connector(), profile(), secrets())
            .unwrap();
    let request = TextModelRequest {
        messages: vec![
            ModelMessage::System("system".to_string()),
            ModelMessage::User("user".to_string()),
            ModelMessage::Tool {
                tool_call_id: Some("call-1".to_string()),
                name: "sfumato_read_file".to_string(),
                content: "result".to_string(),
                failed: false,
            },
        ],
        tools: vec![ToolDefinition {
            kind: "function".to_string(),
            function: ToolFunctionDefinition {
                name: "sfumato_read_file".to_string(),
                description: "Read one file".to_string(),
                parameters: json!({"type": "object"}),
            },
        }],
    };

    let body = serde_json::to_value(provider.request_body(&request)).unwrap();

    assert_eq!(body["model"], "gemma3:latest");
    assert_eq!(body["temperature"], 0.25);
    assert_eq!(body["max_tokens"], 8_192);
    assert!((body["top_p"].as_f64().unwrap() - 0.9).abs() < 0.000_001);
    assert_eq!(body["seed"], 42);
    assert_eq!(body["messages"][2]["role"], "tool");
    assert_eq!(body["messages"][2]["tool_call_id"], "call-1");
    assert_eq!(body["tools"][0]["function"]["name"], "sfumato_read_file");
    assert!(body.get("credential").is_none());
    assert!(body.get("headers").is_none());
}

#[test]
fn omits_tools_when_the_turn_disables_tool_calling() {
    let provider =
        OpenAiCompatibleTextProvider::new("ollama".to_string(), connector(), profile(), secrets())
            .unwrap();
    let request = TextModelRequest {
        messages: vec![ModelMessage::User("hello".to_string())],
        tools: Vec::new(),
    };

    let body = serde_json::to_value(provider.request_body(&request)).unwrap();

    assert!(body.get("tools").is_none());
}

#[test]
fn serializes_image_options_without_text_or_connector_fields() {
    let provider =
        OpenAiCompatibleImageProvider::new("ollama".to_string(), connector(), profile(), secrets())
            .unwrap();

    let body = serde_json::to_value(
        provider
            .request_body(&ImageGenerationRequest {
                prompt: "A visual Fourier decomposition".to_string(),
            })
            .unwrap(),
    )
    .unwrap();

    assert_eq!(body["model"], "gemma3:latest");
    assert_eq!(body["prompt"], "A visual Fourier decomposition");
    assert_eq!(body["n"], 1);
    assert_eq!(body["quality"], "high");
    assert_eq!(body["background"], "transparent");
    assert_eq!(body["size"], "1536x1024");
    assert_eq!(body["aspect_ratio"], "3:2");
    assert_eq!(body["output_format"], "png");
    assert!(body.get("temperature").is_none());
    assert!(body.get("credential").is_none());
}

#[test]
fn recognizes_structured_and_textual_context_limit_responses() {
    assert!(is_context_limit_response(
        r#"{"error":{"code":"context_length_exceeded"}}"#
    ));
    assert!(is_context_limit_response(
        "prompt is too long for this model"
    ));
    assert!(!is_context_limit_response("temporary upstream failure"));
}

#[test]
fn classifies_http_timeouts_as_retryable_provider_failures() {
    let error = provider_error(
        anyhow::anyhow!("request or response body error: operation timed out"),
        OperationStage::Review,
    );

    assert_eq!(error.class, ErrorClass::Retry);
    assert_eq!(error.stage, Some(OperationStage::Review));
}

#[tokio::test]
async fn connector_resolves_stored_credentials_when_building_a_request() {
    let mut config = connector();
    config.credential =
        Some(sfumato_core::config::SecretRef::stored("connector/openrouter").unwrap());
    let connector =
        OpenAiCompatibleConnector::new("openrouter".to_string(), config, secrets()).unwrap();

    let request = connector
        .post("chat/completions")
        .await
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(
        request.headers()[reqwest::header::AUTHORIZATION],
        "Bearer resolved-secret"
    );
}
