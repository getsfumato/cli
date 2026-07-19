use std::{path::PathBuf, sync::Arc};

use async_trait::async_trait;
use serde_json::json;
use sfumato_adapters::codex_app_server::CodexAppServerProvider;
use sfumato_core::{
    config::{
        Capability, CodexAppServerConnectorConfig, ModelOptions, ModelProfile, TextModelOptions,
    },
    errors::{OperationStage, SfumatoResult},
    operation::OperationContext,
    providers::{
        TextGenerationProvider, TextGenerationRequest, ToolDefinition, ToolExecutionRequest,
        ToolExecutor, ToolFunctionDefinition,
    },
};

struct EchoTool;

#[async_trait]
impl ToolExecutor for EchoTool {
    async fn execute(
        &self,
        request: ToolExecutionRequest,
        _: &OperationContext,
        _: OperationStage,
    ) -> SfumatoResult<String> {
        assert_eq!(request.name, "sfumato_echo");
        Ok(format!("echo:{}", request.arguments["value"]))
    }
}

#[tokio::test]
#[ignore = "uses the installed Codex App Server and the current account quota"]
async fn authenticated_app_server_discovers_a_model_and_executes_a_dynamic_tool() {
    let provider = CodexAppServerProvider::new(
        CodexAppServerConnectorConfig {
            executable: PathBuf::from("codex"),
        },
        ModelProfile {
            connector: "codex".to_string(),
            model: "default".to_string(),
            capabilities: vec![Capability::Text],
            options: ModelOptions {
                text: TextModelOptions::default(),
                image: Default::default(),
            },
        },
        PathBuf::from(env!("CARGO_MANIFEST_DIR")),
    );
    let mut request = TextGenerationRequest::new(
        "You are a protocol test. Use sfumato_echo exactly once, then answer READY.".to_string(),
        "Echo the value app-server and finish.".to_string(),
    );
    request.tools = vec![ToolDefinition {
        kind: "function".to_string(),
        function: ToolFunctionDefinition {
            name: "sfumato_echo".to_string(),
            description: "Echo one value".to_string(),
            parameters: json!({
                "type": "object",
                "properties": { "value": { "type": "string" } },
                "required": ["value"],
                "additionalProperties": false
            }),
        },
    }];
    request.tool_executor = Some(Arc::new(EchoTool));

    let response = provider
        .generate_text(
            request,
            &OperationContext::detached(),
            OperationStage::Draft,
        )
        .await
        .unwrap();

    assert!(response.text.contains("READY"));
}
