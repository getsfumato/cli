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

    assert!(error.contains("finish reason: length"));
    assert!(error.contains("reasoning tokens: 4000"));
    assert!(error.contains("Increase the model profile's max_tokens"));
}
