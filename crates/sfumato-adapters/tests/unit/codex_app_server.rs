use serde_json::json;
use sfumato_core::{
    errors::{ErrorClass, OperationStage},
    providers::{ToolDefinition, ToolFunctionDefinition},
};

use super::{CodexModel, completed_agent_text, dynamic_tools, resolve_model, turn_error, turn_input};

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

fn image(path: &std::path::Path) -> sfumato_core::providers::ImageAttachment {
    sfumato_core::providers::ImageAttachment {
        label: "Frame at 4.00s, scene 2".to_string(),
        media_type: "image/png".to_string(),
        path: path.to_path_buf(),
    }
}

#[test]
fn attaches_images_as_local_paths_the_app_server_reads_itself() {
    // The protocol takes a path and opens the file on its side, so a snapshot needs
    // no encoding at all — unlike the two HTTP connectors, which inline base64.
    let frame = std::path::Path::new("/tmp/frame-01.png");
    let mut model = model("gpt-5.6-sol", true);
    model.input_modalities = vec!["text".to_string(), "image".to_string()];

    let input = turn_input("review these", &[image(frame)], &model, OperationStage::Review).unwrap();

    let items = input.as_array().unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0], json!({"type": "text", "text": "review these"}));
    // The label precedes its image: the protocol has no caption field, so this is
    // the only way a finding can name the frame it came from.
    assert_eq!(
        items[1],
        json!({"type": "text", "text": "Frame at 4.00s, scene 2"})
    );
    assert_eq!(items[2], json!({"type": "localImage", "path": "/tmp/frame-01.png"}));
}

#[test]
fn refuses_images_only_when_the_model_says_it_cannot_read_them() {
    // Judged on the model's own declaration, not on the connector kind: this
    // connector does carry image input, and refusing every request for it left the
    // visual review unreachable on the one model that was configured.
    let frame = std::path::Path::new("/tmp/frame-01.png");
    let mut text_only = model("gpt-5.6-sol", true);
    text_only.input_modalities = vec!["text".to_string()];

    let error = turn_input("review these", &[image(frame)], &text_only, OperationStage::Review)
        .expect_err("a text-only model must refuse rather than answer blind");

    assert_eq!(error.class, ErrorClass::Permanent);
    assert!(error.to_string().contains("accepts text only"), "{error}");
    assert!(
        error.to_string().contains("image-capable"),
        "the message has to say what to do instead: {error}"
    );
}

#[test]
fn an_older_catalog_without_declared_modalities_still_accepts_images() {
    // The protocol says to read a missing list as accepting both, so an absent
    // declaration is not a refusal.
    let frame = std::path::Path::new("/tmp/frame-01.png");
    let mut unknown = model("gpt-5.6-sol", true);
    unknown.input_modalities.clear();

    let input =
        turn_input("review these", &[image(frame)], &unknown, OperationStage::Review).unwrap();

    assert_eq!(input.as_array().unwrap().len(), 3);
}

#[test]
fn a_text_only_turn_carries_exactly_one_input_item() {
    let model = model("gpt-5.6-sol", true);

    let input = turn_input("draft a plan", &[], &model, OperationStage::Draft).unwrap();

    assert_eq!(input, json!([{"type": "text", "text": "draft a plan"}]));
}

#[test]
fn a_completed_image_item_reports_the_file_the_tool_wrote() {
    let message = json!({
        "method": "item/completed",
        "params": {
            "item": {
                "type": "imageGeneration",
                "id": "image-run-1",
                "status": "completed",
                "revisedPrompt": null,
                "result": "ok",
                "savedPath": "/tmp/generated_images/session/image-run-1.png",
            }
        }
    });

    assert_eq!(
        super::generated_image_path(&message),
        Some("/tmp/generated_images/session/image-run-1.png")
    );
}

#[test]
fn an_image_item_without_a_saved_file_reports_nothing() {
    // The tool ran and produced nothing usable, which is the same outcome for the
    // caller as never running: there is no file to read.
    let message = json!({
        "method": "item/completed",
        "params": {
            "item": { "type": "imageGeneration", "id": "run", "status": "failed", "result": "" }
        }
    });

    assert!(super::generated_image_path(&message).is_none());

    // An unrelated item must not be mistaken for one.
    let text = json!({
        "method": "item/completed",
        "params": { "item": { "type": "agentMessage", "text": "here you go" } }
    });
    assert!(super::generated_image_path(&text).is_none());
}

#[test]
fn a_turn_that_answered_in_prose_is_reported_as_a_failed_tool_with_a_worked_example() {
    // Generation here is indirect: the model has to choose to invoke its own image
    // tool. A written description is a failure, not an empty success, and the error
    // has to show the direct alternative rather than just say no.
    let error = super::missing_image_error(
        "gpt-5.6-sol",
        "I can describe the illustration instead: a cutaway of an optical fibre…",
        OperationStage::Draft,
    );

    assert_eq!(error.class, ErrorClass::InvalidOutput);
    let message = error.to_string();
    assert!(message.contains("did not generate an image"), "{message}");
    // The model's own words, so the cause is visible without re-running anything.
    assert!(message.contains("I can describe the illustration"), "{message}");
    assert!(message.contains("chooses to invoke its own"), "{message}");
    // A copy-pasteable profile for a connector that does return bytes.
    assert!(message.contains("[models.gpt-image]"), "{message}");
    assert!(message.contains("openai/gpt-image-2"), "{message}");
}

#[test]
fn a_silent_turn_says_so_rather_than_quoting_an_empty_answer() {
    let error = super::missing_image_error("gpt-5.6-sol", "   ", OperationStage::Draft);

    assert!(
        error.to_string().contains("no message at all"),
        "{error}"
    );
}

#[test]
fn the_tool_ceiling_leaves_room_for_calls_already_in_flight() {
    // Measured on a real run: the planner made nine tool calls inside a single agent
    // turn against a budget of eight, and the tenth killed the whole generation. This
    // transport cannot withdraw a model's tools mid-turn, so calls queued before the
    // notice arrived were being punished as defiance.
    assert_eq!(super::tool_call_ceiling(8), 16);
    assert_eq!(super::tool_call_ceiling(16), 24);

    // Calls up to the budget run; past it they are refused, not fatal.
    let budget = 8;
    assert!(9 <= super::tool_call_ceiling(budget), "the ninth is refused, not fatal");
    assert!(
        super::tool_call_ceiling(budget) + 1 > super::tool_call_ceiling(budget),
        "a model that ignores every refusal still terminates"
    );
}

#[test]
fn the_ceiling_cannot_overflow_on_an_absurd_budget() {
    assert_eq!(super::tool_call_ceiling(usize::MAX), usize::MAX);
}
