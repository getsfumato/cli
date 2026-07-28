use super::*;

use serde_json::json;
use sfumato_core::{
    config::{ModelOptions, TextModelOptions},
    errors::ErrorCode,
    operation::DiscardEvents,
    providers::{ModelMessage, ToolDefinition, ToolFunctionDefinition},
    secrets::{SecretResolver, SecretValue},
};

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

fn secrets() -> Arc<dyn SecretResolver> {
    Arc::new(TestSecrets)
}

fn config() -> AnthropicConnectorConfig {
    AnthropicConnectorConfig {
        base_url: "https://api.anthropic.com/v1".to_string(),
        credential: Some(sfumato_core::config::SecretRef::stored("connector/anthropic").unwrap()),
        headers: BTreeMap::new(),
    }
}

fn profile(options: TextModelOptions) -> ModelProfile {
    ModelProfile {
        connector: "anthropic".to_string(),
        model: "claude-opus-5".to_string(),
        capabilities: vec![sfumato_core::config::Capability::Text],
        options: ModelOptions {
            text: options,
            ..Default::default()
        },
    }
}

fn model(options: TextModelOptions) -> AnthropicTextModel {
    AnthropicConnector::new(config(), secrets())
        .unwrap()
        .text_model(profile(options))
}

fn tool() -> ToolDefinition {
    ToolDefinition {
        kind: "function".to_string(),
        function: ToolFunctionDefinition {
            name: "sfumato_read_file".to_string(),
            description: "Read a project file".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"]
            }),
        },
    }
}

fn body(model: &AnthropicTextModel, request: &TextModelRequest) -> serde_json::Value {
    serde_json::to_value(model.request_body(request).unwrap()).unwrap()
}

#[test]
fn serializes_the_system_prompt_outside_the_message_array() {
    let request = TextModelRequest {
        messages: vec![
            ModelMessage::System("be terse".into()),
            ModelMessage::User("draft a deck".into()),
        ],
        tools: Vec::new(),
    };

    let body = body(&model(TextModelOptions::default()), &request);

    assert_eq!(body["system"], "be terse");
    assert_eq!(body["messages"].as_array().unwrap().len(), 1);
    assert_eq!(body["messages"][0]["role"], "user");
}

#[test]
fn defaults_max_tokens_to_the_non_streaming_budget_and_clamps_it() {
    let cases = [
        (None, 16_000),
        (Some(200_000), 32_000),
        (Some(8_192), 8_192),
        (Some(1), 1_024),
    ];

    for (configured, expected) in cases {
        let request = TextModelRequest {
            messages: vec![ModelMessage::User("hi".into())],
            tools: Vec::new(),
        };
        let body = body(
            &model(TextModelOptions {
                max_tokens: configured,
                ..Default::default()
            }),
            &request,
        );
        assert_eq!(body["max_tokens"], expected, "for {configured:?}");
    }
}

#[test]
fn never_serializes_sampling_parameters_stream_or_thinking() {
    // Current Claude models reject these outright, so absence is enforced by the
    // request type rather than by a runtime check.
    let request = TextModelRequest {
        messages: vec![ModelMessage::User("hi".into())],
        tools: Vec::new(),
    };

    let body = body(
        &model(TextModelOptions {
            temperature: Some(0.4),
            top_p: Some(0.9),
            seed: Some(42),
            ..Default::default()
        }),
        &request,
    );

    for absent in [
        "temperature",
        "top_p",
        "top_k",
        "stream",
        "thinking",
        "tool_choice",
        "output_config",
    ] {
        assert!(body.get(absent).is_none(), "{absent} must not be sent");
    }
}

#[test]
fn translates_tools_into_a_flat_input_schema() {
    let request = TextModelRequest {
        messages: vec![ModelMessage::User("hi".into())],
        tools: vec![tool()],
    };

    let body = body(&model(TextModelOptions::default()), &request);

    assert_eq!(body["tools"][0]["name"], "sfumato_read_file");
    assert_eq!(body["tools"][0]["input_schema"]["type"], "object");
    assert!(body["tools"][0].get("function").is_none());
    assert!(body.get("tool_choice").is_none());
}

#[test]
fn omits_tools_when_the_turn_disables_tool_calling() {
    let request = TextModelRequest {
        messages: vec![ModelMessage::User("hi".into())],
        tools: Vec::new(),
    };

    assert!(
        body(&model(TextModelOptions::default()), &request)
            .get("tools")
            .is_none()
    );
}

#[test]
fn coalesces_every_tool_result_into_one_user_message() {
    // Splitting tool_result blocks across messages silently degrades parallel
    // tool use, so this is the regression guard for the translation fold.
    let request = TextModelRequest {
        messages: vec![
            ModelMessage::System("sys".into()),
            ModelMessage::User("go".into()),
            ModelMessage::Assistant {
                content: Some("plan".into()),
                tool_calls: vec![
                    ToolCall {
                        id: Some("toolu_1".into()),
                        kind: "function".into(),
                        function: ToolCallFunction {
                            name: "a".into(),
                            arguments: json!({}),
                        },
                    },
                    ToolCall {
                        id: Some("toolu_2".into()),
                        kind: "function".into(),
                        function: ToolCallFunction {
                            name: "b".into(),
                            arguments: json!({}),
                        },
                    },
                ],
            },
            ModelMessage::Tool {
                tool_call_id: Some("toolu_1".into()),
                name: "a".into(),
                content: "first".into(),
            },
            ModelMessage::Tool {
                tool_call_id: Some("toolu_2".into()),
                name: "b".into(),
                content: "second".into(),
            },
        ],
        tools: Vec::new(),
    };

    let body = body(&model(TextModelOptions::default()), &request);
    let messages = body["messages"].as_array().unwrap();

    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0]["role"], "user");

    assert_eq!(messages[1]["role"], "assistant");
    let assistant = messages[1]["content"].as_array().unwrap();
    assert_eq!(assistant.len(), 3);
    assert_eq!(assistant[0]["type"], "text");
    assert_eq!(assistant[1]["type"], "tool_use");
    assert_eq!(assistant[2]["type"], "tool_use");

    // Both results land in one user turn rather than two.
    assert_eq!(messages[2]["role"], "user");
    let results = messages[2]["content"].as_array().unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["tool_use_id"], "toolu_1");
    assert_eq!(results[1]["tool_use_id"], "toolu_2");
}

#[test]
fn rejects_a_trailing_assistant_turn() {
    let request = TextModelRequest {
        messages: vec![
            ModelMessage::User("go".into()),
            ModelMessage::Assistant {
                content: Some("half an answer".into()),
                tool_calls: Vec::new(),
            },
        ],
        tools: Vec::new(),
    };

    let error = model(TextModelOptions::default())
        .request_body(&request)
        .expect_err("assistant prefill is unsupported on current models");

    assert!(error.to_string().contains("prefill"));
}

#[test]
fn rejects_a_tool_result_without_a_provider_tool_call_id() {
    let request = TextModelRequest {
        messages: vec![
            ModelMessage::User("go".into()),
            ModelMessage::Tool {
                tool_call_id: None,
                name: "a".into(),
                content: "result".into(),
            },
        ],
        tools: Vec::new(),
    };

    let error = model(TextModelOptions::default())
        .request_body(&request)
        .expect_err("Anthropic requires tool_use_id on every tool_result");

    assert!(error.to_string().contains("tool-call id"));
}

#[test]
fn credentials_never_appear_in_the_request_body() {
    let request = TextModelRequest {
        messages: vec![ModelMessage::User("hi".into())],
        tools: Vec::new(),
    };

    let body = body(&model(TextModelOptions::default()), &request);

    assert!(body.get("credential").is_none());
    assert!(body.get("headers").is_none());
    assert!(!body.to_string().contains("resolved-secret"));
}

#[tokio::test]
async fn sends_x_api_key_and_a_pinned_version_without_bearer_auth() {
    let request = AnthropicConnector::new(config(), secrets())
        .unwrap()
        .post(
            "messages",
            &OperationContext::detached(),
            OperationStage::Draft,
        )
        .await
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(
        request.url().as_str(),
        "https://api.anthropic.com/v1/messages"
    );
    assert_eq!(request.headers()["x-api-key"], "resolved-secret");
    assert_eq!(request.headers()["anthropic-version"], "2023-06-01");
    assert!(
        request
            .headers()
            .get(reqwest::header::AUTHORIZATION)
            .is_none()
    );
}

#[tokio::test]
async fn configured_headers_override_the_pinned_api_version() {
    let mut config = config();
    config
        .headers
        .insert("anthropic-version".to_string(), "2099-01-01".to_string());

    let request = AnthropicConnector::new(config, secrets())
        .unwrap()
        .post(
            "messages",
            &OperationContext::detached(),
            OperationStage::Draft,
        )
        .await
        .unwrap()
        .build()
        .unwrap();

    let versions = request
        .headers()
        .get_all("anthropic-version")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .collect::<Vec<_>>();
    assert_eq!(versions.last(), Some(&"2099-01-01"));
}

#[test]
fn maps_tool_use_blocks_into_provider_neutral_tool_calls() {
    let content = vec![
        json!({"type": "text", "text": "reading"}),
        json!({"type": "tool_use", "id": "toolu_01", "name": "sfumato_read_file", "input": {"path": "/tmp/a.md"}}),
    ];

    let calls = response_tool_calls(&content).unwrap();

    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].id.as_deref(), Some("toolu_01"));
    // Anthropic sends an object, not OpenAI's stringified arguments.
    assert_eq!(calls[0].function.arguments, json!({"path": "/tmp/a.md"}));
    assert_eq!(response_text(&content).as_deref(), Some("reading"));
}

#[test]
fn joins_text_blocks_and_ignores_thinking_and_unknown_blocks() {
    let content = vec![
        json!({"type": "thinking", "thinking": ""}),
        json!({"type": "text", "text": "one"}),
        json!({"type": "text", "text": "two"}),
        json!({"type": "future_block", "payload": 1}),
    ];

    assert_eq!(response_text(&content).as_deref(), Some("one\ntwo"));
    assert!(response_tool_calls(&content).unwrap().is_empty());
}

fn interpret(stop_reason: &str, content: Vec<serde_json::Value>) -> SfumatoError {
    let parsed: MessagesResponse = serde_json::from_value(json!({
        "content": content,
        "stop_reason": stop_reason,
        "usage": {"output_tokens": 12}
    }))
    .unwrap();
    let calls = response_tool_calls(&parsed.content).unwrap();
    let error = interpret_response("claude-opus-5", 16_000, parsed, calls)
        .map(|_| ())
        .expect_err("expected a typed error");
    provider_error(error, OperationStage::Draft)
}

#[test]
fn classifies_truncated_and_overflowing_responses_as_typed_limits() {
    let truncated = interpret("max_tokens", vec![json!({"type": "text", "text": "cut"})]);
    assert_eq!(truncated.class, ErrorClass::InvalidOutput);

    let overflow = interpret("model_context_window_exceeded", Vec::new());
    assert_eq!(overflow.class, ErrorClass::ContextLimit);
}

#[test]
fn classifies_pause_turn_as_retryable_and_refusals_as_permanent() {
    assert_eq!(interpret("pause_turn", Vec::new()).class, ErrorClass::Retry);

    let parsed: MessagesResponse = serde_json::from_value(json!({
        "content": [],
        "stop_reason": "refusal",
        "stop_details": {"type": "refusal", "category": "cyber", "explanation": "declined"}
    }))
    .unwrap();
    let error = provider_error(
        interpret_response("claude-opus-5", 16_000, parsed, Vec::new())
            .map(|_| ())
            .expect_err("a refusal is an error"),
        OperationStage::Draft,
    );

    assert_eq!(error.class, ErrorClass::Permanent);
    assert_eq!(
        error.details.get("refusal_category").map(String::as_str),
        Some("cyber")
    );
}

#[test]
fn treats_an_empty_end_turn_response_as_invalid_output() {
    assert_eq!(
        interpret("end_turn", Vec::new()).class,
        ErrorClass::InvalidOutput
    );
}

#[test]
fn maps_anthropic_error_codes_to_recovery_classes() {
    let cases: [(u16, &str, ErrorClass); 7] = [
        (
            400,
            r#"{"error":{"type":"invalid_request_error","message":"bad"}}"#,
            ErrorClass::Permanent,
        ),
        (
            400,
            r#"{"error":{"message":"prompt is too long: 1200000 tokens > 1000000 maximum"}}"#,
            ErrorClass::ContextLimit,
        ),
        (
            401,
            r#"{"error":{"type":"authentication_error","message":"bad key"}}"#,
            ErrorClass::Permanent,
        ),
        (
            413,
            r#"{"error":{"message":"request too large"}}"#,
            ErrorClass::ContextLimit,
        ),
        (
            429,
            r#"{"error":{"type":"rate_limit_error","message":"slow down"}}"#,
            ErrorClass::Retry,
        ),
        (
            500,
            r#"{"error":{"type":"api_error","message":"boom"}}"#,
            ErrorClass::Retry,
        ),
        (
            529,
            r#"{"error":{"type":"overloaded_error","message":"busy"}}"#,
            ErrorClass::Unavailable,
        ),
    ];

    for (status, body, expected) in cases {
        let error = classify_response(
            reqwest::StatusCode::from_u16(status).unwrap(),
            body,
            Some("30"),
            "claude-opus-5",
        );
        assert_eq!(error.class, expected, "HTTP {status}");
        assert_eq!(
            error.details.get("retry_after_seconds").map(String::as_str),
            Some("30")
        );
    }
}

#[test]
fn surfaces_the_request_id_and_never_the_api_key() {
    let error = classify_response(
        reqwest::StatusCode::UNAUTHORIZED,
        r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key sk-ant-api03-AAAA"},"request_id":"req_1"}"#,
        None,
        "claude-opus-5",
    );

    assert_eq!(
        error.details.get("request_id").map(String::as_str),
        Some("req_1")
    );
    assert!(!error.to_string().contains("sk-ant"));
}

#[test]
fn maps_the_models_capability_tree_onto_connector_summaries() {
    let response: ModelsResponse = serde_json::from_value(json!({
        "data": [{
            "id": "claude-opus-5",
            "display_name": "Claude Opus 5",
            "created_at": "2026-06-01T00:00:00Z",
            "max_input_tokens": 1000000,
            "max_tokens": 128000,
            "capabilities": {
                "image_input": {"supported": true},
                "structured_outputs": {"supported": true},
                "thinking": {"supported": true, "types": {"adaptive": {"supported": true}}},
                "effort": {"supported": true, "low": {"supported": true}, "medium": {"supported": true},
                           "high": {"supported": true}, "xhigh": {"supported": true}, "max": {"supported": true}}
            }
        }],
        "has_more": false
    }))
    .unwrap();

    let model = map_model(response.data.into_iter().next().unwrap());

    // The context window is `max_input_tokens`; `max_tokens` is the output cap.
    assert_eq!(model.context_length, Some(1_000_000));
    assert_eq!(model.metadata["max_output_tokens"], "128000");
    assert_eq!(model.input_modalities, vec!["text", "image"]);
    assert_eq!(model.output_modalities, vec!["text"]);
    assert!(model.is_default);
    assert_eq!(model.metadata["thinking"], "adaptive");
    assert_eq!(
        model.metadata["effort_levels"],
        "low, medium, high, xhigh, max"
    );
}

#[test]
fn defaults_missing_capability_subtrees_to_text_only() {
    let response: ModelsResponse =
        serde_json::from_value(json!({"data": [{"id": "claude-x"}]})).unwrap();

    let model = map_model(response.data.into_iter().next().unwrap());

    assert_eq!(model.input_modalities, vec!["text"]);
    assert_eq!(model.context_length, None);
    assert_eq!(model.display_name, "claude-x");
    assert!(!model.is_default);
}

#[test]
fn paginates_with_after_id_cursors() {
    let page = |has_more: bool, last_id: Option<&str>| ModelsResponse {
        data: Vec::new(),
        has_more,
        last_id: last_id.map(ToOwned::to_owned),
    };

    assert_eq!(
        next_page_cursor(&page(true, Some("m_2"))).as_deref(),
        Some("m_2")
    );
    // A truthful `has_more` without a cursor must stop rather than loop.
    assert_eq!(next_page_cursor(&page(true, None)), None);
    assert_eq!(next_page_cursor(&page(false, Some("m_2"))), None);
}

#[tokio::test]
async fn cancellation_stops_a_model_turn_before_the_request_is_sent() {
    let (handle, operation) = OperationContext::create(None, Arc::new(DiscardEvents));
    handle.cancel();
    let mut unreachable = config();
    unreachable.base_url = "http://127.0.0.1:1/v1".to_string();

    let error = AnthropicConnector::new(unreachable, secrets())
        .unwrap()
        .text_model(profile(TextModelOptions::default()))
        .complete(
            TextModelRequest {
                messages: vec![ModelMessage::User("hi".into())],
                tools: Vec::new(),
            },
            &operation,
            OperationStage::Draft,
        )
        .await
        .expect_err("a cancelled operation cannot run a model turn");

    assert_eq!(error.code, ErrorCode::Cancelled);
    assert_eq!(error.class, ErrorClass::Cancelled);
    assert_eq!(error.stage, Some(OperationStage::Draft));
}

#[tokio::test]
async fn cancellation_stops_catalog_pagination() {
    let (handle, operation) = OperationContext::create(None, Arc::new(DiscardEvents));
    handle.cancel();
    let mut unreachable = config();
    unreachable.base_url = "http://127.0.0.1:1/v1".to_string();

    let error = AnthropicConnector::new(unreachable, secrets())
        .unwrap()
        .list_models(&operation)
        .await
        .expect_err("a cancelled operation cannot list models");

    assert_eq!(error.class, ErrorClass::Cancelled);
}
