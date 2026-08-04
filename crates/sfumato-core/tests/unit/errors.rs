//! Unit tests for user-facing error redaction.

use super::*;

fn message_of(raw: &str) -> String {
    SfumatoError::provider(ErrorClass::Permanent, raw).message
}

fn leaks(raw: &str) -> bool {
    message_of(&format!("provider rejected the key {raw}")).contains(raw)
}

#[test]
fn redacts_the_key_shapes_of_shipped_connectors() {
    // Every one of these reached `error.message` intact before.
    for secret in [
        "0123456789abcdef0123456789abcdef",       // ElevenLabs, 32-hex
        "sk_0123456789abcdef0123456789abcdef",    // ElevenLabs, prefixed
        "AIzaSyC1234567890abcdefghijklmnopqrstu", // Google
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef", // 64-hex
        "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dBjftJeZ4CVPmB92K27uhbUJU1p1r_wW1gFWFOEjXk", // JWT
    ] {
        assert!(!leaks(secret), "leaked {secret}");
    }
}

#[test]
fn keeps_redacting_the_shapes_it_already_caught() {
    for secret in [
        "sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789",
        "sk-or-v1-abcdefghijklmnopqrstuvwxyz0123456789",
        "sk_or_v1_abcdefghijklmnopqrstuvwxyz0123456789",
    ] {
        assert!(!leaks(secret), "leaked {secret}");
    }
}

#[test]
fn redacts_a_key_that_a_provider_echoes_inside_json() {
    let message = message_of(r#"{"error":"invalid key","key":"sk-ant-api03-abcdefghijklmnop"}"#);
    assert!(!message.contains("sk-ant-api03"), "{message}");
}

#[test]
fn preserves_the_context_users_need_to_act_on_an_error() {
    // Over-redaction is its own failure: these all survived before and must
    // keep surviving, or the message stops naming what went wrong.
    for benign in [
        "/Users/someone/Library/Application Support/sfumato/themes/sfumato-default/document/print.css",
        "claude-opus-4-20250514",
        "us.anthropic.claude-sonnet-4-20250514-v1:0",
        "550e8400-e29b-41d4-a716-446655440000",
        "http://localhost:11434/v1",
        "validate_selected_capabilities",
    ] {
        let message = message_of(&format!("could not reach {benign}"));
        assert!(message.contains(benign), "redacted {benign} -> {message}");
    }
}

#[test]
fn redacts_details_because_they_are_documented_as_sanitized() {
    let error = SfumatoError::provider(ErrorClass::Permanent, "rejected")
        .with_detail("key", "sk-ant-api03-abcdefghijklmnopqrstuvwxyz");
    assert_eq!(
        error.details.get("key").map(String::as_str),
        Some("[REDACTED]")
    );
}

#[test]
fn keeps_a_provider_request_id_readable_in_details() {
    // Support asks for this value by name, so it must survive redaction.
    for request_id in [
        "req_011CQjmVL3xN8kFhP2wZ",
        "550e8400-e29b-41d4-a716-446655440000",
    ] {
        let error = SfumatoError::provider(ErrorClass::Retry, "rate limited")
            .with_detail("request_id", request_id);
        assert_eq!(
            error.details.get("request_id").map(String::as_str),
            Some(request_id)
        );
    }
}

#[test]
fn sanitizes_messages_built_through_the_public_constructor() {
    // `new` is public; before, it was the way past both redaction and the cap.
    let error = SfumatoError::new(
        ErrorCode::Config,
        ErrorClass::Permanent,
        "key sk-ant-api03-abcdefghijklmnopqrstuvwxyz was refused",
    );
    assert!(!error.message.contains("sk-ant-api03"), "{}", error.message);
}

#[test]
fn caps_a_long_message_exactly_once() {
    let error = SfumatoError::provider(ErrorClass::Permanent, "a ".repeat(4_000));
    // 2,000 characters plus the single ellipsis; a second pass would append
    // another one and push it past the cap.
    assert_eq!(error.message.chars().count(), 2_003, "{}", error.message);
    assert!(error.message.ends_with("..."));
    assert!(!error.message.ends_with("......"));
}

#[test]
fn an_absent_prompt_template_is_not_found_rather_than_uncoded() {
    // Parsing used to happen in the presentation layer, so this reached the
    // user with no code prefix at all.
    let error = SfumatoError::from(crate::prompts::PromptError::UnknownId("nope".to_string()));

    assert_eq!(error.code, ErrorCode::NotFound);
    // Nothing was rendered, so no stage should be claimed.
    assert_eq!(error.stage, None);
}

#[test]
fn a_prompt_render_failure_stays_a_config_error_at_its_stage() {
    let error = SfumatoError::from(crate::prompts::PromptError::Render {
        id: crate::prompts::PromptId::all()[0],
        message: "strict mode".to_string(),
    });

    assert_eq!(error.code, ErrorCode::Config);
    assert_eq!(error.stage, Some(OperationStage::RenderPrompt));
}

#[test]
fn an_absent_lookup_resolves_to_one_code_for_every_entity() {
    // ADR-0004 makes these codes a public contract, and they disagreed across
    // services performing the same kind of lookup.
    let missing: Option<u8> = None;
    let error = missing
        .or_not_found("Connector 'nope' was not found")
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::NotFound);
    assert_eq!(error.class, ErrorClass::Permanent);
}
