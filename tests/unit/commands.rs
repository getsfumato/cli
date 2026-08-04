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

#[test]
fn a_typed_error_serializes_the_same_object_as_an_anyhow_one() {
    // `video preview` and `video approve` return `SfumatoError` directly and used
    // to propagate with `?`, so a `--json` caller got nothing parseable while the
    // other five commands in the same group emitted this object.
    let error = sfumato_core::errors::SfumatoError::not_found("session 'nope' was not found");

    let direct = json_typed_error(&error);
    let through_anyhow = json_operation_error(&anyhow::Error::new(error));

    assert_eq!(direct, through_anyhow);
    assert_eq!(direct["error"]["code"], "not_found");
    assert_eq!(direct["error"]["retryable"], false);
}

#[test]
fn every_command_that_accepts_json_can_report_an_error_as_json() {
    // The contract is per-command, so this pins the list rather than trusting
    // that a new `--json` flag remembers to wire the error path.
    let with_json = [
        "generate slides --json",
        "generate document --json",
        "generate page --json",
        "generate video --json",
        "edit slides --json",
        "video preview --json",
        "video approve --json",
    ];
    let source = include_str!("../../src/commands/mod.rs");
    // Five sites use the anyhow helper; the two video arms use the typed one.
    let anyhow_sites = source.matches("json_operation_error(&error)").count();
    let typed_sites = source.matches("json_typed_error(&error)").count();

    assert_eq!(
        anyhow_sites + typed_sites,
        with_json.len(),
        "{} commands accept --json but {} emit a JSON error",
        with_json.len(),
        anyhow_sites + typed_sites
    );
}

#[test]
fn a_short_content_hash_is_displayed_rather_than_panicking() {
    // These values come from persisted records, not a freshly computed digest, so a
    // hand-edited or corrupt manifest carrying a short hash panicked on the slice.
    assert_eq!(short_hash("abc"), "abc");
    assert_eq!(short_hash(""), "");
    assert_eq!(short_hash("0123456789ab"), "0123456789ab");
    assert_eq!(short_hash("0123456789abcdef"), "0123456789ab");
}

#[test]
fn a_hash_with_multibyte_characters_does_not_panic_on_the_boundary() {
    // A corrupt record is not guaranteed to be hex, and slicing by byte index into
    // a multibyte character is its own panic.
    let hash = "ñ".repeat(20);
    let shown = short_hash(&hash);

    assert!(hash.starts_with(shown) || shown == hash, "{shown}");
}

#[test]
fn no_pdf_can_turn_off_a_project_that_enables_pdf() {
    // The override used to be OR-ed with `marp.pdf`, which ships as `true`, so no
    // flag combination could skip the PDF for one run.
    #[derive(clap::Parser)]
    struct Harness {
        #[command(flatten)]
        args: crate::cli::SlidesArgs,
    }
    use clap::Parser;

    let parse = |extra: &[&str]| {
        let mut argv = vec!["sfumato", "--instruction", "x"];
        argv.extend_from_slice(extra);
        argv.push("in.md");
        Harness::try_parse_from(argv).map(|harness| harness.args)
    };

    let with = parse(&["--pdf"]).expect("--pdf parses");
    let without = parse(&["--no-pdf"]).expect("--no-pdf parses");
    let neither = parse(&[]).expect("neither flag parses");

    assert_eq!(flag_override(with.pdf, with.no_pdf), Some(true));
    assert_eq!(flag_override(without.pdf, without.no_pdf), Some(false));
    // Absent means the project decides, which is what the old `false` resolved to.
    assert_eq!(flag_override(neither.pdf, neither.no_pdf), None);
    // Contradictory flags are refused rather than silently resolved.
    assert!(parse(&["--pdf", "--no-pdf"]).is_err());
}
