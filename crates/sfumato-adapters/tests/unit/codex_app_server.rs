use serde_json::json;
use sfumato_core::{
    errors::{ErrorClass, OperationStage},
    providers::{ToolDefinition, ToolFunctionDefinition},
};

use super::{CodexModel, completed_agent_text, dynamic_tools, resolve_model, turn_error};

fn model(id: &str, is_default: bool) -> CodexModel {
    CodexModel {
        id: id.to_string(),
        model: id.to_string(),
        display_name: id.to_uppercase(),
        is_default,
        hidden: false,
        input_modalities: vec!["text".to_string()],
    }
}

#[test]
fn maps_sfumato_tools_to_dynamic_tool_specs() {
    let tools = dynamic_tools(&[ToolDefinition {
        kind: "function".to_string(),
        function: ToolFunctionDefinition {
            name: "sfumato_read_file".to_string(),
            description: "Read a project file".to_string(),
            parameters: json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            }),
        },
    }]);

    assert_eq!(tools[0]["type"], "function");
    assert_eq!(tools[0]["name"], "sfumato_read_file");
    assert_eq!(tools[0]["inputSchema"]["required"][0], "path");
}

#[test]
fn resolves_default_and_explicit_codex_models() {
    let models = vec![model("gpt-5.4", true), model("gpt-5.5-codex", false)];

    assert_eq!(
        resolve_model(&models, "default", OperationStage::Resolve)
            .unwrap()
            .model,
        "gpt-5.4"
    );
    assert_eq!(
        resolve_model(&models, "gpt-5.5-codex", OperationStage::Resolve)
            .unwrap()
            .id,
        "gpt-5.5-codex"
    );
    assert!(resolve_model(&models, "missing", OperationStage::Resolve).is_err());
}

#[test]
fn extracts_completed_agent_messages() {
    let notification = json!({
        "method": "item/completed",
        "params": {
            "item": {
                "type": "agentMessage",
                "id": "item-1",
                "text": "READY",
                "phase": "final_answer"
            }
        }
    });

    assert_eq!(completed_agent_text(&notification), Some("READY"));
}

#[test]
fn classifies_context_window_turn_failures() {
    let params = json!({
        "turn": {
            "error": {
                "message": "too much context",
                "codexErrorInfo": "contextWindowExceeded"
            }
        }
    });

    let error = turn_error(&params, "failed", OperationStage::Draft);
    assert_eq!(error.class, ErrorClass::ContextLimit);
    assert_eq!(error.stage, Some(OperationStage::Draft));
}
