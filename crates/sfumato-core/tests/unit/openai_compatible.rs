use super::*;
use crate::providers::{ToolCall, ToolCallFunction, ToolFunctionDefinition};
use std::collections::BTreeMap;

fn profile(connector: &str) -> ModelProfile {
    ModelProfile {
        connector: connector.to_string(),
        model: "llama3.2".to_string(),
        capabilities: vec![crate::config::Capability::Text],
        options: BTreeMap::from([("max_tokens".to_string(), toml::Value::Integer(100))]),
    }
}

fn image_profile(connector: &str) -> ModelProfile {
    ModelProfile {
        connector: connector.to_string(),
        model: "openai/gpt-image-1".to_string(),
        capabilities: vec![crate::config::Capability::Image],
        options: BTreeMap::from([
            (
                "quality".to_string(),
                toml::Value::String("high".to_string()),
            ),
            (
                "background".to_string(),
                toml::Value::String("transparent".to_string()),
            ),
        ]),
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

    let body = connector.request_body(&TextGenerationRequest::new(
        "system".to_string(),
        "user".to_string(),
    ));

    assert_eq!(body.model, "llama3.2");
    assert_eq!(body.max_tokens, 100);
    assert_eq!(body.messages.len(), 2);
}

#[test]
fn serializes_image_generation_from_model_profile() {
    let provider = OpenAiCompatibleImageProvider::new(
        "openrouter".to_string(),
        OpenAiCompatibleConnectorConfig {
            base_url: "https://openrouter.ai/api/v1".to_string(),
            api_key: Some("test-key".to_string()),
            api_key_env: None,
            headers: BTreeMap::new(),
        },
        image_profile("openrouter"),
    )
    .unwrap();

    let body = provider
        .request_body(&ImageGenerationRequest {
            prompt: "A Fourier series diagram".to_string(),
        })
        .unwrap();

    assert_eq!(body.model, "openai/gpt-image-1");
    assert_eq!(body.prompt, "A Fourier series diagram");
    assert_eq!(body.n, 1);
    assert_eq!(body.options["quality"], "high");
    assert_eq!(body.options["background"], "transparent");
}

#[test]
fn image_generation_rejects_reserved_profile_options() {
    let mut profile = image_profile("openrouter");
    profile.options.insert(
        "prompt".to_string(),
        toml::Value::String("override".to_string()),
    );
    let provider = OpenAiCompatibleImageProvider::new(
        "openrouter".to_string(),
        OpenAiCompatibleConnectorConfig {
            base_url: "https://openrouter.ai/api/v1".to_string(),
            api_key: Some("test-key".to_string()),
            api_key_env: None,
            headers: BTreeMap::new(),
        },
        profile,
    )
    .unwrap();

    assert!(
        provider
            .request_body(&ImageGenerationRequest {
                prompt: "Prompt".to_string(),
            })
            .is_err()
    );
}

#[test]
fn serializes_chat_completion_tools() {
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
    let mut request = TextGenerationRequest::new("system".to_string(), "user".to_string());
    request.tools.push(ToolDefinition {
        kind: "function".to_string(),
        function: ToolFunctionDefinition {
            name: "sfumato_read_file".to_string(),
            description: "Read a file".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            }),
        },
    });

    let body = connector.request_body(&request);

    assert_eq!(body.tools.unwrap()[0].function.name, "sfumato_read_file");
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

#[test]
fn formats_tool_execution_errors_as_tool_results() {
    let tool_call = ToolCall {
        id: Some("call-1".to_string()),
        kind: "function".to_string(),
        function: ToolCallFunction {
            name: "sfumato_read_file".to_string(),
            arguments: serde_json::json!({ "path": "missing.md" }),
        },
    };

    let result = tool_error_json(&tool_call, "missing path".to_string());

    assert!(result.contains("missing path"));
    assert!(result.contains("sfumato_read_file"));
}

#[test]
fn preserves_reasoning_details_for_follow_up_tool_rounds() {
    let response: ChatCompletionsResponse = serde_json::from_value(serde_json::json!({
        "choices": [{
            "finish_reason": "tool_calls",
            "message": {
                "role": "assistant",
                "content": null,
                "reasoning": "I should inspect the notes.",
                "reasoning_details": [{
                    "type": "reasoning.text",
                    "text": "Inspect the notes",
                    "format": "unknown"
                }],
                "tool_calls": []
            }
        }]
    }))
    .unwrap();
    let message = &response.choices[0].message;

    assert_eq!(
        message.reasoning.as_deref(),
        Some("I should inspect the notes.")
    );
    assert!(message.reasoning_details.is_some());
    let serialized = serde_json::to_value(message).unwrap();
    assert!(serialized.get("reasoning_details").is_some());
}

#[test]
fn explains_empty_content_caused_by_reasoning_token_limit() {
    let usage = CompletionUsage {
        completion_tokens: Some(4000),
        completion_tokens_details: Some(CompletionTokenDetails {
            reasoning_tokens: Some(4000),
        }),
    };

    let error = empty_content_error(&profile("openrouter"), Some("length"), Some(&usage));
    let message = error.to_string();

    assert!(message.contains("finish reason: length"));
    assert!(message.contains("reasoning tokens: 4000"));
    assert!(message.contains("Increase the model profile's max_tokens"));
    let limit = error.downcast_ref::<TextGenerationLimitError>().unwrap();
    assert_eq!(
        limit.kind,
        crate::providers::TextGenerationLimitKind::Output
    );
    assert_eq!(limit.completion_tokens, Some(4000));
    assert_eq!(limit.reasoning_tokens, Some(4000));
}

#[test]
fn rejects_nonempty_but_truncated_text_response() {
    let usage = CompletionUsage {
        completion_tokens: Some(4000),
        completion_tokens_details: Some(CompletionTokenDetails {
            reasoning_tokens: Some(900),
        }),
    };

    let error = ensure_text_response_complete(&profile("openrouter"), Some("length"), Some(&usage))
        .unwrap_err();

    assert!(error.to_string().contains("refused to use"));
    assert!(error.to_string().contains("max_tokens=100"));
    assert!(error.to_string().contains("completion tokens: 4000"));
    assert!(error.to_string().contains("reasoning tokens: 900"));
    assert!(error.downcast_ref::<TextGenerationLimitError>().is_some());
}

#[test]
fn accepts_completed_text_response() {
    ensure_text_response_complete(&profile("openrouter"), Some("stop"), None).unwrap();
}

#[test]
fn recognizes_structured_and_textual_context_limit_errors() {
    assert!(is_context_limit_response(
        r#"{"error":{"code":"context_length_exceeded","message":"Too large"}}"#
    ));
    assert!(is_context_limit_response(
        r#"{"error":{"message":"This request exceeds the maximum context length"}}"#
    ));
    assert!(!is_context_limit_response(
        r#"{"error":{"code":"invalid_model","message":"Unknown model"}}"#
    ));
}

#[test]
fn compacts_context_error_details() {
    let detail = compact_error_detail(
        r#"{"error":{"message":"The prompt is too long for this context window"}}"#,
    );

    assert_eq!(detail, "The prompt is too long for this context window");
}
