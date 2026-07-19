use super::*;

#[test]
fn maps_ollama_local_model_details() {
    let response: TagsResponse = serde_json::from_str(
        r#"{
      "models": [{
        "name": "gemma3:latest",
        "model": "gemma3:latest",
        "size": 3338801804,
        "digest": "abc123",
        "details": {
          "family": "gemma3",
          "parameter_size": "4.3B",
          "quantization_level": "Q4_K_M"
        }
      }]
    }"#,
    )
    .unwrap();

    let model = map_model(response.models.into_iter().next().unwrap());

    assert_eq!(model.id, "gemma3:latest");
    assert_eq!(model.metadata["family"], "gemma3");
    assert_eq!(model.metadata["quantization"], "Q4_K_M");
}
