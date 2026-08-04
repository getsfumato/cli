use super::*;

#[test]
fn maps_openrouter_modalities_context_and_pricing() {
    let response: ModelsResponse = serde_json::from_str(
        r#"{
      "data": [{
        "id": "openai/gpt-image-2",
        "name": "GPT Image 2",
        "description": "Image generation model",
        "context_length": 128000,
        "architecture": {
          "input_modalities": ["text", "image"],
          "output_modalities": ["image"]
        },
        "pricing": {"prompt": "0.001", "completion": "0", "image": "0.04"},
        "supported_parameters": ["tools"]
      }]
    }"#,
    )
    .unwrap();

    let model = map_model(response.data.into_iter().next().unwrap());

    assert_eq!(model.id, "openai/gpt-image-2");
    assert_eq!(model.output_modalities, vec!["image"]);
    assert_eq!(model.context_length, Some(128_000));
    assert_eq!(model.metadata["image_price"], "0.04");
}

#[test]
fn validates_video_capabilities_before_submission() {
    let model = VideoModel {
        id: "provider/video".into(),
        supported_durations: vec![5, 8],
        supported_resolutions: vec!["720p".into()],
        supported_aspect_ratios: vec!["16:9".into()],
        supported_parameters: vec!["input_references".into()],
        generate_audio: Some(false),
    };
    let mut request = VideoGenerationRequest {
        prompt: "Animate a waveform".into(),
        duration_seconds: 5,
        resolution: "720p".into(),
        aspect_ratio: "16:9".into(),
        generate_audio: Some(false),
        seed: None,
        references: Vec::new(),
    };

    assert!(model.supports_input_references());
    validate_video_request(&model, &request).unwrap();

    request.generate_audio = Some(true);
    assert!(
        validate_video_request(&model, &request)
            .unwrap_err()
            .to_string()
            .contains("native audio")
    );
}

#[test]
fn serializes_openrouter_video_request_without_empty_references() {
    let payload = CreateVideoRequest {
        model: "provider/video".into(),
        prompt: "Animate a waveform".into(),
        duration: 5,
        resolution: "720p".into(),
        aspect_ratio: "16:9".into(),
        generate_audio: None,
        seed: Some(42),
        input_references: Vec::new(),
    };

    let value = serde_json::to_value(payload).unwrap();

    assert_eq!(value["seed"], 42);
    assert!(value.get("input_references").is_none());
    assert!(value.get("generate_audio").is_none());
}

#[test]
fn a_rate_limited_submit_is_retryable_so_it_is_not_lost() {
    // A 429 on submit used to abort the whole video operation, which is the
    // failure the transport retries were introduced for. The submit started no
    // job, so repeating it cannot bill a second render.
    let error = provider_status_error(
        "OpenRouter",
        "video generation",
        reqwest::StatusCode::TOO_MANY_REQUESTS,
        "slow down",
    );

    assert!(error.retryable);
    assert_eq!(error.class, ErrorClass::Retry);
}

#[test]
fn a_rejected_submit_is_never_retried() {
    // The reason `generate_video` as a whole is not wrapped: a repeat must never
    // be able to start a second billable render. A rejection that repeating
    // cannot fix must therefore stay permanent.
    for status in [400_u16, 401, 403, 404, 422] {
        let error = provider_status_error(
            "OpenRouter",
            "video generation",
            reqwest::StatusCode::from_u16(status).unwrap(),
            "nope",
        );

        assert!(!error.retryable, "HTTP {status} would be retried");
    }
}

#[test]
fn a_transport_failure_that_never_reached_the_provider_is_retryable() {
    let error = transient_transport_error(
        "video submission",
        anyhow::anyhow!("connection reset by peer"),
    );

    assert!(error.retryable);
    assert!(
        error.message.contains("could not reach the provider"),
        "{}",
        error.message
    );
}

#[test]
fn a_cancelled_operation_is_not_turned_into_a_retry() {
    // Cancellation and deadlines are already typed; converting one into a
    // retryable transport error would let a retry outlive the operation.
    let cancelled = SfumatoError::cancelled(Some(OperationStage::Render));
    let error = transient_transport_error("video status", anyhow::Error::new(cancelled));

    assert_eq!(error.code, sfumato_core::errors::ErrorCode::Cancelled);
    assert!(!error.retryable);
}

#[test]
fn a_poll_failure_is_tolerated_only_while_it_looks_transient() {
    // The render is already running and already billed by the time polling
    // starts, so one rate-limited status read must not discard it. The loop keys
    // that decision off `retryable`, so what matters is which statuses set it.
    let tolerated = [408_u16, 429, 500, 502, 503, 504];
    let fatal = [400_u16, 401, 403, 404, 422];

    for status in tolerated {
        let error = provider_status_error(
            "OpenRouter",
            "video status",
            reqwest::StatusCode::from_u16(status).unwrap(),
            "later",
        );
        assert!(error.retryable, "HTTP {status} should not abandon the job");
    }
    for status in fatal {
        let error = provider_status_error(
            "OpenRouter",
            "video status",
            reqwest::StatusCode::from_u16(status).unwrap(),
            "no",
        );
        assert!(!error.retryable, "HTTP {status} should stop polling");
    }
}
