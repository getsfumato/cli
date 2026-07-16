use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use sfumato_core::providers::{
    AgentRunner, ModelMessage, TextGenerationEvent, TextGenerationProvider, TextGenerationRequest,
    TextModel, TextModelRequest, TextModelResponse, ToolCall, ToolCallFunction,
    ToolExecutionRequest, ToolExecutor,
};
use sfumato_core::{errors::OperationStage, operation::OperationContext};

struct ScriptedModel {
    responses: Mutex<VecDeque<TextModelResponse>>,
    requests: Mutex<Vec<TextModelRequest>>,
}

#[async_trait]
impl TextModel for ScriptedModel {
    async fn complete(
        &self,
        request: TextModelRequest,
        _operation: &OperationContext,
        _stage: OperationStage,
    ) -> Result<TextModelResponse> {
        self.requests.lock().unwrap().push(request);
        Ok(self.responses.lock().unwrap().pop_front().unwrap())
    }
}

struct EchoTool;

#[async_trait]
impl ToolExecutor for EchoTool {
    async fn execute(
        &self,
        request: ToolExecutionRequest,
        _operation: &OperationContext,
        _stage: OperationStage,
    ) -> Result<String> {
        Ok(json!({"tool": request.name, "arguments": request.arguments}).to_string())
    }
}

fn call(id: &str) -> ToolCall {
    ToolCall {
        id: Some(id.to_string()),
        kind: "function".to_string(),
        function: ToolCallFunction {
            name: "sfumato_read_file".to_string(),
            arguments: json!({"path": "/tmp/note.md"}),
        },
    }
}

#[tokio::test]
async fn agent_runner_owns_tool_rounds_and_transcript() {
    let model = Arc::new(ScriptedModel {
        responses: Mutex::new(VecDeque::from([
            TextModelResponse {
                content: None,
                tool_calls: vec![call("call-1")],
            },
            TextModelResponse {
                content: Some("Complete deck".to_string()),
                tool_calls: vec![],
            },
        ])),
        requests: Mutex::new(Vec::new()),
    });
    let runner = AgentRunner::new(model.clone());
    let events = Arc::new(Mutex::new(Vec::new()));
    let event_log = events.clone();
    let mut request = TextGenerationRequest::new("system".into(), "user".into());
    request.max_tool_rounds = 2;
    request.tool_executor = Some(Arc::new(EchoTool));
    request.event_sink = Some(Arc::new(move |event| event_log.lock().unwrap().push(event)));

    let response = runner
        .generate_text(
            request,
            &OperationContext::detached(),
            OperationStage::Draft,
        )
        .await
        .unwrap();

    assert_eq!(response.text, "Complete deck");
    let requests = model.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(
        matches!(requests[1].messages.last(), Some(ModelMessage::Tool { name, .. }) if name == "sfumato_read_file")
    );
    assert!(
        events
            .lock()
            .unwrap()
            .iter()
            .any(|event| matches!(event, TextGenerationEvent::ToolCallSucceeded { .. }))
    );
}

#[tokio::test]
async fn tool_exhaustion_disables_tools_for_the_contract_turn() {
    let model = Arc::new(ScriptedModel {
        responses: Mutex::new(VecDeque::from([
            TextModelResponse {
                content: None,
                tool_calls: vec![call("call-1")],
            },
            TextModelResponse {
                content: Some("Final answer".to_string()),
                tool_calls: vec![],
            },
        ])),
        requests: Mutex::new(Vec::new()),
    });
    let runner = AgentRunner::new(model.clone());
    let mut request = TextGenerationRequest::new("system".into(), "user".into());
    request.max_tool_rounds = 1;
    request.tool_executor = Some(Arc::new(EchoTool));
    request.tool_exhausted_prompt = Some("Return the final output now.".to_string());

    let response = runner
        .generate_text(
            request,
            &OperationContext::detached(),
            OperationStage::Draft,
        )
        .await
        .unwrap();

    assert_eq!(response.text, "Final answer");
    let requests = model.requests.lock().unwrap();
    assert!(requests[1].tools.is_empty());
    assert!(
        matches!(requests[1].messages.last(), Some(ModelMessage::User(message)) if message == "Return the final output now.")
    );
}
