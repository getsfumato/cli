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
