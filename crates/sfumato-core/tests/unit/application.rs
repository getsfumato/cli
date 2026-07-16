use super::*;

#[test]
fn preserves_typed_cancellation_across_the_application_boundary() {
    let expected = SfumatoError::cancelled(Some(OperationStage::Render));
    let actual = application_error(expected.clone().into(), ErrorCode::Internal, None);
    assert_eq!(actual, expected);
}

#[test]
fn classifies_model_limits_for_machine_readable_callers() {
    let error = TextGenerationLimitError::context(
        "example/model".to_string(),
        4_000,
        "context too large".to_string(),
    );
    let actual = application_error(error.into(), ErrorCode::Internal, None);
    assert_eq!(actual.code, ErrorCode::Provider);
    assert_eq!(actual.class, ErrorClass::ContextLimit);
    assert!(actual.retryable);
}

#[test]
fn redacts_secret_like_tokens_from_adapter_failures() {
    let error = anyhow::anyhow!("request failed with sk-or-v1-super-secret-token");
    let actual = application_error(error, ErrorCode::Provider, Some(OperationStage::Draft));
    assert!(!actual.message.contains("super-secret"));
    assert!(actual.message.contains("[REDACTED]"));
    assert_eq!(actual.stage, Some(OperationStage::Draft));
}

#[test]
fn facade_service_errors_receive_stable_public_codes() {
    let error = public_result::<()>(
        Err(anyhow::anyhow!("model profile was not found")),
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
    let error = public_prompt_error(PromptError::Missing(PromptId::SlidesDraftSystem));

    assert_eq!(error.code, ErrorCode::Config);
    assert_eq!(error.class, ErrorClass::Permanent);
    assert_eq!(error.stage, Some(OperationStage::RenderPrompt));
}
