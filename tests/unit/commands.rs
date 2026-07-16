use super::*;

#[test]
fn parses_repeatable_capability_model_overrides() {
    let parsed = parse_model_overrides(&[
        "text=local-text".to_string(),
        "image=cloud-image".to_string(),
    ])
    .unwrap();
    assert_eq!(parsed.get(&Capability::Text).unwrap(), "local-text");
    assert_eq!(parsed.get(&Capability::Image).unwrap(), "cloud-image");
}

#[test]
fn rejects_invalid_model_override() {
    assert!(parse_model_overrides(&["local-text".to_string()]).is_err());
}

#[test]
fn compacts_long_generation_event_previews() {
    let preview = compact_preview("one\n  two   three four", 13);

    assert_eq!(preview, "one two three...");
}

#[test]
fn formats_tool_arguments_as_readable_paths() {
    let formatted = format_tool_arguments(&serde_json::json!({
        "path": "/tmp/course"
    }));

    assert!(formatted.contains("/tmp/course"));
    assert!(!formatted.contains("{\"path\""));
}

#[test]
fn summarizes_directory_tool_results_without_raw_json() {
    let formatted = format_tool_result(
        "sfumato_list_directory",
        &serde_json::json!({
            "path": "/tmp/course",
            "entries": [
                { "name": "notes.md", "kind": "file", "path": "/tmp/course/notes.md", "bytes": 10 },
                { "name": "week-1", "kind": "directory", "path": "/tmp/course/week-1", "bytes": null }
            ],
            "truncated": false
        })
        .to_string(),
    );
    let plain = strip_ansi(&formatted);

    assert!(plain.contains("listed"));
    assert!(plain.contains("2 entries"));
    assert!(plain.contains("1 files"));
    assert!(plain.contains("1 directories"));
    assert!(plain.contains("notes.md"));
    assert!(!plain.contains("\"entries\""));
}

#[test]
fn summarizes_file_tool_results_without_raw_content() {
    let formatted = format_tool_result(
        "sfumato_read_file",
        &serde_json::json!({
            "path": "/tmp/course/notes.md",
            "content": "# Fourier\n\nLong explanation"
        })
        .to_string(),
    );
    let plain = strip_ansi(&formatted);

    assert!(plain.contains("read"));
    assert!(plain.contains("notes.md"));
    assert!(plain.contains("27 chars"));
    assert!(plain.contains("3 lines"));
    assert!(!plain.contains("\"content\""));
}

#[test]
fn summarizes_generated_image_results_without_raw_json() {
    let formatted = format_tool_result(
        "sfumato_image_gen",
        r#"{"markdown_path":"images/generated-01-wave.png","model_profile":"openrouter-image"}"#,
    );

    assert!(formatted.contains("created"));
    assert!(formatted.contains("images/generated-01-wave.png"));
    assert!(formatted.contains("openrouter-image"));
    assert!(!formatted.contains("markdown_path"));
}

#[test]
fn serializes_typed_operation_errors_for_agent_callers() {
    let error = sfumato_core::errors::SfumatoError::cancelled(Some(
        sfumato_core::errors::OperationStage::Render,
    ));
    let rendered = json_operation_error(&anyhow::Error::new(error));

    assert_eq!(rendered["error"]["code"], "cancelled");
    assert_eq!(rendered["error"]["class"], "cancelled");
    assert_eq!(rendered["error"]["stage"], "render");
    assert_eq!(rendered["error"]["retryable"], false);
}

fn strip_ansi(value: &str) -> String {
    let mut output = String::new();
    let mut chars = value.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            for character in chars.by_ref() {
                if character == 'm' {
                    break;
                }
            }
        } else {
            output.push(character);
        }
    }
    output
}
