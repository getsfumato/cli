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
