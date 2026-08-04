use super::*;
use crate::{errors::ErrorClass, providers::TextGenerationLimitError};

#[test]
fn preserves_typed_cancellation_across_the_application_boundary() {
    let expected = SfumatoError::cancelled(Some(OperationStage::Render));
    let actual = public_result::<()>(Err(expected.clone()), ErrorCode::Internal).unwrap_err();
    assert_eq!(actual, expected);
}

#[test]
fn classifies_model_limits_for_machine_readable_callers() {
    let error = TextGenerationLimitError::context(
        "example/model".to_string(),
        4_000,
        "context too large".to_string(),
    );
    let actual = SfumatoError::from(error);
    assert_eq!(actual.code, ErrorCode::Provider);
    assert_eq!(actual.class, ErrorClass::ContextLimit);
    assert!(actual.retryable);
}

#[test]
fn redacts_secret_like_tokens_from_adapter_failures() {
    let actual = SfumatoError::provider(
        ErrorClass::Permanent,
        "request failed with sk-or-v1-super-secret-token",
    )
    .at_stage(OperationStage::Draft);
    assert!(!actual.message.contains("super-secret"));
    assert!(actual.message.contains("[REDACTED]"));
    assert_eq!(actual.stage, Some(OperationStage::Draft));
}

#[test]
fn facade_service_errors_receive_stable_public_codes() {
    let error = public_result::<()>(
        Err(SfumatoError::not_found("model profile was not found")),
        ErrorCode::NotFound,
    )
    .unwrap_err();

    assert_eq!(error.code, ErrorCode::NotFound);
    assert_eq!(error.class, ErrorClass::Permanent);
    assert!(!error.retryable);
    assert_eq!(error.stage, None);
}

#[test]
fn prompt_management_errors_are_scoped_to_prompt_rendering() {
    // ADR-0004 lists prompts among the entities `NotFound` covers, so an absent
    // template reports the same code as an absent project, model, or theme.
    let error = public_prompt_error(PromptError::Missing(PromptId::SlidesDraftSystem));

    assert_eq!(error.code, ErrorCode::NotFound);
    assert_eq!(error.class, ErrorClass::Permanent);
    assert_eq!(error.stage, Some(OperationStage::RenderPrompt));
}

#[test]
fn prompt_rendering_failures_stay_configuration_errors() {
    // Only "does not exist" moved to `NotFound`; a template that exists but
    // cannot be rendered is still a configuration problem.
    let error = public_prompt_error(PromptError::Render {
        id: PromptId::SlidesDraftSystem,
        message: "undefined variable".to_string(),
    });

    assert_eq!(error.code, ErrorCode::Config);
    assert_eq!(error.stage, Some(OperationStage::RenderPrompt));
}

#[test]
fn disabling_a_plugin_also_reaches_the_project_ui_library() {
    // The UI plugin was pushed after the `retain`, so `--disable-plugin <ui-name>`
    // silently did nothing and `--ui ""` was the only way to turn it off.
    let plugins = resolve_page_plugins(
        &["katex".to_string()],
        Vec::new(),
        &["shadcn".to_string()],
        Some("shadcn".to_string()),
    );

    assert_eq!(plugins, vec!["katex".to_string()]);
}

#[test]
fn the_ui_library_is_still_loaded_when_it_is_not_disabled() {
    let plugins = resolve_page_plugins(
        &["katex".to_string()],
        Vec::new(),
        &[],
        Some("shadcn".to_string()),
    );

    assert_eq!(plugins, vec!["katex".to_string(), "shadcn".to_string()]);
}

#[test]
fn an_empty_ui_override_disables_the_project_library() {
    // The documented escape hatch keeps working.
    let plugins =
        resolve_page_plugins(&["katex".to_string()], Vec::new(), &[], Some(String::new()));

    assert_eq!(plugins, vec!["katex".to_string()]);
}

#[test]
fn a_requested_plugin_can_also_be_disabled_and_the_list_stays_unique() {
    let plugins = resolve_page_plugins(
        &["katex".to_string()],
        vec!["mermaid".to_string(), "katex".to_string()],
        &["mermaid".to_string()],
        None,
    );

    assert_eq!(plugins, vec!["katex".to_string()]);
}
