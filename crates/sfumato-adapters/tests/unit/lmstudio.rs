use super::*;

use sfumato_core::{
    errors::ErrorCode,
    operation::DiscardEvents,
    secrets::{SecretResolver, SecretValue},
};
use std::time::Duration;

struct TestSecrets;

#[async_trait::async_trait]
impl SecretResolver for TestSecrets {
    async fn resolve(
        &self,
        _reference: &sfumato_core::config::SecretRef,
    ) -> SfumatoResult<SecretValue> {
        Ok(SecretValue::new("resolved-secret".to_string()))
    }
}

fn secrets() -> Arc<dyn SecretResolver> {
    Arc::new(TestSecrets)
}

fn connector(credential: Option<sfumato_core::config::SecretRef>) -> LmStudioConnectorConfig {
    LmStudioConnectorConfig {
        transport: OpenAiCompatibleConnectorConfig {
            base_url: "http://localhost:1234/v1".to_string(),
            credential,
            headers: BTreeMap::new(),
        },
        native_base_url: "http://localhost:1234".to_string(),
    }
}

/// A base URL on a port that cannot accept connections, so the cancellation
/// tests prove the checkpoint fires before any socket work.
fn unreachable_connector() -> LmStudioConnectorConfig {
    LmStudioConnectorConfig {
        transport: OpenAiCompatibleConnectorConfig {
            base_url: "http://127.0.0.1:1/v1".to_string(),
            credential: None,
            headers: BTreeMap::new(),
        },
        native_base_url: "http://127.0.0.1:1".to_string(),
    }
}

#[test]
fn maps_native_model_details_and_load_state() {
    let response: NativeModelsResponse = serde_json::from_str(
        r#"{
          "data": [{
            "id": "qwen2-vl-7b-instruct",
            "object": "model",
            "type": "vlm",
            "publisher": "mlx-community",
            "arch": "qwen2_vl",
            "compatibility_type": "mlx",
            "quantization": "4bit",
            "state": "loaded",
            "max_context_length": 32768,
            "loaded_context_length": 4096
          }]
        }"#,
    )
    .unwrap();

    let model = map_native_model(response.data.into_iter().next().unwrap());

    assert_eq!(model.id, "qwen2-vl-7b-instruct");
    assert_eq!(model.display_name, "qwen2-vl-7b-instruct");
    assert_eq!(model.context_length, Some(32768));
    assert_eq!(model.input_modalities, vec!["text", "image"]);
    assert_eq!(model.output_modalities, vec!["text"]);
    assert_eq!(model.metadata["arch"], "qwen2_vl");
    assert_eq!(model.metadata["quantization"], "4bit");
    assert_eq!(model.metadata["state"], "loaded");
    assert_eq!(model.metadata["loaded_context_length"], "4096");
}

#[test]
fn maps_embedding_models_to_the_embedding_output_modality() {
    let (inputs, outputs) = modalities(Some("embeddings"));

    assert_eq!(inputs, vec!["text"]);
    assert_eq!(outputs, vec!["embedding"]);
}

#[test]
fn maps_a_sparse_openai_model_list_as_a_fallback() {
    let response: OpenAiModelsResponse =
        serde_json::from_str(r#"{"data":[{"id":"local-model","object":"model"}]}"#).unwrap();

    let model = map_openai_model(response.data.into_iter().next().unwrap());

    assert_eq!(model.id, "local-model");
    assert_eq!(model.context_length, None);
    assert_eq!(model.input_modalities, vec!["text"]);
    assert!(model.metadata.is_empty());
}

#[test]
fn describes_loaded_models_with_their_context_usage() {
    let response: NativeModelsResponse = serde_json::from_str(
        r#"{"data":[
            {"id":"a","type":"llm","state":"loaded","max_context_length":8192,"loaded_context_length":2048},
            {"id":"b","type":"embeddings","state":"not-loaded"},
            {"id":"c","type":"vlm","state":"loaded"}
        ]}"#,
    )
    .unwrap();

    assert_eq!(count_kind(&response.data, "vlm"), 1);
    assert_eq!(count_kind(&response.data, "embeddings"), 1);
    assert_eq!(describe_loaded_model(&response.data[0]), "a (2048/8192)");
    // Without both context figures the label degrades to the bare id.
    assert_eq!(describe_loaded_model(&response.data[2]), "c");
}

#[tokio::test]
async fn builds_native_requests_without_a_credential_by_default() {
    let request = LmStudioConnector::new("lmstudio", &connector(None), secrets())
        .unwrap()
        .native
        .get("api/v0/models")
        .await
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(
        request.url().as_str(),
        "http://localhost:1234/api/v0/models"
    );
    assert!(
        request
            .headers()
            .get(reqwest::header::AUTHORIZATION)
            .is_none()
    );
}

#[tokio::test]
async fn applies_the_optional_lmstudio_server_key_to_the_native_surface() {
    let credential = sfumato_core::config::SecretRef::stored("connector/lmstudio").unwrap();
    let request = LmStudioConnector::new("lmstudio", &connector(Some(credential)), secrets())
        .unwrap()
        .native
        .get("api/v0/models")
        .await
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(
        request.headers()[reqwest::header::AUTHORIZATION],
        "Bearer resolved-secret"
    );
}

#[tokio::test]
async fn cancellation_stops_the_catalog_before_a_request_is_sent() {
    let (handle, operation) = OperationContext::create(None, Arc::new(DiscardEvents));
    handle.cancel();

    let error = LmStudioConnector::new("lmstudio", &unreachable_connector(), secrets())
        .unwrap()
        .list_models(&operation)
        .await
        .expect_err("a cancelled operation cannot list models");

    assert_eq!(error.code, ErrorCode::Cancelled);
    assert_eq!(error.class, ErrorClass::Cancelled);
}

#[tokio::test]
async fn an_expired_deadline_stops_native_status() {
    let (_handle, operation) =
        OperationContext::create(Some(Duration::from_millis(0)), Arc::new(DiscardEvents));

    let error = LmStudioConnector::new("lmstudio", &unreachable_connector(), secrets())
        .unwrap()
        .status("lmstudio", &operation)
        .await
        .expect_err("an expired deadline cannot read status");

    assert_eq!(error.class, ErrorClass::Cancelled);
    assert_eq!(
        error.details.get("reason").map(String::as_str),
        Some("deadline_exceeded")
    );
}
