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
            speech: Default::default(),
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

#[test]
fn a_text_only_message_still_serialises_as_a_bare_string() {
    // Several connectors reject the array form for system and tool roles, so text
    // requests have to keep the exact shape they had before images existed.
    let message = ChatMessage::from(ModelMessage::System("be brief".to_string()));

    let json = serde_json::to_value(&message).unwrap();

    assert_eq!(json["content"], json!("be brief"));
}

#[test]
fn a_message_with_images_carries_each_frame_behind_its_own_label() {
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("frame-00.png");
    let second = directory.path().join("frame-01.png");
    std::fs::write(&first, [0x89, 0x50]).unwrap();
    std::fs::write(&second, [0xff]).unwrap();
    let message = ChatMessage::from(ModelMessage::UserWithImages {
        content: "review these".to_string(),
        images: vec![
            ImageAttachment {
                label: "Frame at 0.00s, scene 1".to_string(),
                media_type: "image/png".to_string(),
                path: first,
            },
            ImageAttachment {
                label: "Frame at 4.00s, scene 2".to_string(),
                media_type: "image/png".to_string(),
                path: second,
            },
        ],
    });

    let json = serde_json::to_value(&message).unwrap();

    assert_eq!(json["role"], "user");
    let parts = json["content"]
        .as_array()
        .expect("images need the array form");
    // Prompt, then label and image for each frame: a model that cannot name the
    // frame it is describing produces findings nothing can act on.
    assert_eq!(parts.len(), 5);
    assert_eq!(parts[0], json!({"type": "text", "text": "review these"}));
    assert_eq!(
        parts[1],
        json!({"type": "text", "text": "Frame at 0.00s, scene 1"})
    );
    assert_eq!(
        parts[2],
        json!({"type": "image_url", "image_url": {"url": "data:image/png;base64,iVA="}})
    );
    assert_eq!(
        parts[3],
        json!({"type": "text", "text": "Frame at 4.00s, scene 2"})
    );
}

#[test]
fn an_assistant_reply_in_the_array_form_reads_back_as_text() {
    // A connector may answer in parts even when it was asked in parts. Reading
    // only the string form would report an empty response for a real answer.
    let message: ChatMessage = serde_json::from_value(json!({
        "role": "assistant",
        "content": [
            {"type": "text", "text": "{\"approved\": true"},
            {"type": "text", "text": ", \"findings\": []}"},
        ],
    }))
    .unwrap();

    assert_eq!(
        message.content.as_ref().and_then(MessageContent::text),
        Some("{\"approved\": true\n, \"findings\": []}".to_string())
    );
}

#[test]
fn a_rejected_credential_is_not_reported_as_retryable() {
    // Three native connectors funnelled every non-typed failure into
    // `Unavailable`, which `is_retryable` reports as `true`, so the public DTO
    // told automation to retry a 401 that can never succeed.
    let error = introspection_error(
        "Ollama",
        "endpoint '/api/tags'",
        reqwest::StatusCode::UNAUTHORIZED,
        "{\"error\":{\"message\":\"invalid key\"}}",
    );

    assert_eq!(error.class, ErrorClass::Permanent);
    assert!(!error.retryable);
    // The recovery is named, since a credential is what the caller must change.
    assert!(
        error.message.contains("connector login"),
        "{}",
        error.message
    );
}

#[test]
fn a_permanent_client_error_is_not_reported_as_retryable() {
    for status in [400_u16, 404, 422] {
        let error = introspection_error(
            "LM Studio",
            "endpoint '/v1/models'",
            reqwest::StatusCode::from_u16(status).unwrap(),
            "nope",
        );

        assert!(!error.retryable, "HTTP {status} was reported retryable");
        assert_eq!(error.class, ErrorClass::Permanent);
    }
}

#[test]
fn rate_limits_and_outages_stay_retryable() {
    // The point is not to make everything permanent: transient statuses must
    // keep advertising that a retry can help.
    let throttled = introspection_error(
        "OpenRouter",
        "the model catalog",
        reqwest::StatusCode::TOO_MANY_REQUESTS,
        "slow down",
    );
    assert_eq!(throttled.class, ErrorClass::Retry);
    assert!(throttled.retryable);

    let down = introspection_error(
        "OpenRouter",
        "the model catalog",
        reqwest::StatusCode::BAD_GATEWAY,
        "upstream",
    );
    assert_eq!(down.class, ErrorClass::Unavailable);
    assert!(down.retryable);
}

#[test]
fn an_unclassified_status_defaults_to_permanent() {
    // Defaulting to retryable is what caused this; an unknown status is not
    // evidence that repeating the request will help.
    let error = introspection_error(
        "Ollama",
        "endpoint '/api/ps'",
        reqwest::StatusCode::from_u16(418).unwrap(),
        "teapot",
    );

    assert!(!error.retryable);
}

#[test]
fn the_provider_and_subject_are_named_in_the_message() {
    let error = introspection_error(
        "LM Studio",
        "endpoint '/v1/models'",
        reqwest::StatusCode::SERVICE_UNAVAILABLE,
        "down",
    );

    assert!(error.message.contains("LM Studio"), "{}", error.message);
    assert!(error.message.contains("/v1/models"), "{}", error.message);
    assert_eq!(error.stage, Some(OperationStage::Resolve));
}
