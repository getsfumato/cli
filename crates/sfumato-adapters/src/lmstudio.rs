//! LM Studio-native local model catalog and runtime status adapter.
//!
//! Generation reuses the connector's shared OpenAI-compatible transport, because
//! LM Studio genuinely serves `chat/completions` on `/v1`. Only the native REST
//! surface lives here: `/api/v0/models` reports per-model architecture,
//! quantization, and load state that the OpenAI-compatible `/v1/models` omits.
//!
//! This adapter spawns no child process, so the process-reaping case in
//! `docs/reference/testing.md` does not apply; the cancellation checkpoints do.

use std::{collections::BTreeMap, sync::Arc};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use sfumato_core::{
    config::{LmStudioConnectorConfig, OpenAiCompatibleConnectorConfig},
    errors::{ErrorClass, OperationStage, SfumatoError, SfumatoResult},
    operation::OperationContext,
    providers::{ConnectorModelSummary, ConnectorStatus, ConnectorStatusField},
    secrets::SecretResolver,
};

use crate::{
    openai_compatible::{OpenAiCompatibleConnector, provider_status_error},
    runtime::await_operation,
};

/// LM Studio adapter over the connector's native REST root.
pub struct LmStudioConnector {
    // Kept private: `OpenAiCompatibleConnector` is `pub(crate)`, so a public
    // field of that type would not compile.
    native: OpenAiCompatibleConnector,
    /// The configured OpenAI-compatible transport, used for the `/v1` fallback.
    ///
    /// Derived from `native_base_url` it would break every deployment where the
    /// two roots differ — a remote LM Studio, or a `native_base_url` that already
    /// includes `/api/v0` as LM Studio's own documentation writes it.
    transport: OpenAiCompatibleConnector,
}

impl LmStudioConnector {
    /// Creates native LM Studio operations.
    ///
    /// The native transport carries the connector's credential and headers, so an
    /// optional LM Studio server key applies to the native surface as well as to
    /// chat completions.
    pub fn new(
        name: &str,
        config: &LmStudioConnectorConfig,
        secrets: Arc<dyn SecretResolver>,
    ) -> SfumatoResult<Self> {
        let connector = |base_url: String| {
            OpenAiCompatibleConnector::new(
                name.to_string(),
                OpenAiCompatibleConnectorConfig {
                    base_url,
                    credential: config.transport.credential.clone(),
                    headers: config.transport.headers.clone(),
                },
                secrets.clone(),
            )
            .map_err(|error| {
                SfumatoError::config(format_args!("{error:#}")).at_stage(OperationStage::Resolve)
            })
        };
        Ok(Self {
            native: connector(native_root(&config.native_base_url))?,
            transport: connector(config.transport.base_url.trim_end_matches('/').to_string())?,
        })
    }

    /// Lists local models through `/api/v0/models`, falling back to `/v1/models`.
    pub async fn list_models(
        &self,
        operation: &OperationContext,
    ) -> SfumatoResult<Vec<ConnectorModelSummary>> {
        let result: Result<Vec<ConnectorModelSummary>> = async {
            match self.native_models(operation).await? {
                Some(models) => Ok(models.into_iter().map(map_native_model).collect()),
                None => {
                    let fallback: OpenAiModelsResponse = self
                        .get_json(&self.transport, "models", operation)
                        .await?
                        .context("LM Studio exposes neither /api/v0/models nor /v1/models")?;
                    Ok(fallback.data.into_iter().map(map_openai_model).collect())
                }
            }
        }
        .await;
        native_result(result)
    }

    /// Reports how many local models are installed and which are loaded.
    ///
    /// LM Studio publishes no version endpoint, so every field is derived from the
    /// model catalog rather than invented.
    pub async fn status(
        &self,
        name: &str,
        operation: &OperationContext,
    ) -> SfumatoResult<ConnectorStatus> {
        let result: Result<ConnectorStatus> = async {
            let models = match self.native_models(operation).await? {
                Some(models) => models,
                None => bail!(
                    "LM Studio endpoint '/api/v0/models' did not answer with a native model catalog; native status requires LM Studio 0.3.6 or newer at '{}'",
                    self.native.endpoint("api/v0/models")
                ),
            };
            let loaded = models
                .iter()
                .filter(|model| model.state.as_deref() == Some("loaded"))
                .map(describe_loaded_model)
                .collect::<Vec<_>>();
            Ok(ConnectorStatus {
                connector: name.into(),
                kind: "lmstudio".into(),
                fields: vec![
                    ConnectorStatusField {
                        name: "total_models".into(),
                        value: models.len().to_string(),
                    },
                    ConnectorStatusField {
                        name: "loaded_models".into(),
                        value: loaded.len().to_string(),
                    },
                    ConnectorStatusField {
                        name: "loaded".into(),
                        value: loaded.join(", "),
                    },
                    ConnectorStatusField {
                        name: "vision_models".into(),
                        value: count_kind(&models, "vlm").to_string(),
                    },
                    ConnectorStatusField {
                        name: "embedding_models".into(),
                        value: count_kind(&models, "embeddings").to_string(),
                    },
                ],
            })
        }
        .await;
        native_result(result)
    }

    /// Reads `/api/v0/models`, mapping "not the native surface" onto `None`.
    ///
    /// A 200 body without a `data` envelope means the server on this port is not
    /// an LM Studio native REST surface, so it must fall back rather than report
    /// an empty local catalog as a successful answer.
    async fn native_models(
        &self,
        operation: &OperationContext,
    ) -> Result<Option<Vec<NativeModel>>> {
        Ok(self
            .get_json::<NativeModelsResponse>(&self.native, "api/v0/models", operation)
            .await?
            .and_then(|response| response.data))
    }

    /// Reads one endpoint, mapping HTTP 404 to `None` for the fallback.
    async fn get_json<T: for<'de> Deserialize<'de>>(
        &self,
        connector: &OpenAiCompatibleConnector,
        path: &str,
        operation: &OperationContext,
    ) -> Result<Option<T>> {
        let request = connector.get(path).await?;
        let response = await_operation(operation, OperationStage::Resolve, request.send())
            .await
            .with_context(|| format!("Could not reach {}", connector.endpoint(path)))?;
        let status = response.status();
        let body = await_operation(operation, OperationStage::Resolve, response.text()).await?;
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !status.is_success() {
            return Err(provider_status_error(
                "LM Studio",
                &format!("endpoint '{path}'"),
                status,
                &body,
            )
            .into());
        }
        if body.trim().is_empty() {
            return Ok(None);
        }
        serde_json::from_str(&body)
            .map(Some)
            .with_context(|| format!("LM Studio endpoint '{path}' returned invalid JSON"))
    }
}

fn count_kind(models: &[NativeModel], kind: &str) -> usize {
    models
        .iter()
        .filter(|model| model.kind.as_deref() == Some(kind))
        .count()
}

fn describe_loaded_model(model: &NativeModel) -> String {
    match (model.loaded_context_length, model.max_context_length) {
        (Some(loaded), Some(max)) => format!("{} ({loaded}/{max})", model.id),
        _ => model.id.clone(),
    }
}

/// Maps LM Studio's model `type` onto provider-neutral modalities.
fn modalities(kind: Option<&str>) -> (Vec<String>, Vec<String>) {
    match kind {
        Some("vlm") => (vec!["text".into(), "image".into()], vec!["text".into()]),
        Some("embeddings") => (vec!["text".into()], vec!["embedding".into()]),
        _ => (vec!["text".into()], vec!["text".into()]),
    }
}

fn map_native_model(model: NativeModel) -> ConnectorModelSummary {
    let (input_modalities, output_modalities) = modalities(model.kind.as_deref());
    let mut metadata = BTreeMap::new();
    for (key, value) in [
        ("type", model.kind),
        ("arch", model.arch),
        ("publisher", model.publisher),
        ("quantization", model.quantization),
        ("compatibility_type", model.compatibility_type),
        ("state", model.state),
    ] {
        if let Some(value) = value {
            metadata.insert(key.to_string(), value);
        }
    }
    if let Some(value) = model.loaded_context_length {
        metadata.insert("loaded_context_length".into(), value.to_string());
    }
    ConnectorModelSummary {
        display_name: model.id.clone(),
        id: model.id,
        is_default: false,
        hidden: false,
        input_modalities,
        output_modalities,
        context_length: model.max_context_length,
        description: None,
        metadata,
    }
}

fn map_openai_model(model: OpenAiModel) -> ConnectorModelSummary {
    let (input_modalities, output_modalities) = modalities(None);
    let mut metadata = BTreeMap::new();
    if let Some(value) = model.owned_by {
        metadata.insert("owned_by".into(), value);
    }
    ConnectorModelSummary {
        display_name: model.id.clone(),
        id: model.id,
        is_default: false,
        hidden: false,
        input_modalities,
        output_modalities,
        context_length: None,
        description: None,
        metadata,
    }
}

#[derive(Deserialize)]
struct NativeModelsResponse {
    /// `None` — not an empty vector — when the body carries no `data` envelope,
    /// which distinguishes "no native surface here" from "no models installed".
    data: Option<Vec<NativeModel>>,
}

/// Normalizes a configured native root, tolerating the documented `/api/v0` and
/// a copied `/v1` suffix so `/api/v0/models` is not built off the wrong root.
fn native_root(native_base_url: &str) -> String {
    native_base_url
        .trim_end_matches('/')
        .trim_end_matches("/api/v0")
        .trim_end_matches("/v1")
        .trim_end_matches('/')
        .to_string()
}

#[derive(Deserialize)]
struct NativeModel {
    id: String,
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    arch: Option<String>,
    #[serde(default)]
    publisher: Option<String>,
    #[serde(default)]
    quantization: Option<String>,
    #[serde(default)]
    compatibility_type: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    max_context_length: Option<u64>,
    #[serde(default)]
    loaded_context_length: Option<u64>,
}

#[derive(Deserialize)]
struct OpenAiModelsResponse {
    #[serde(default)]
    data: Vec<OpenAiModel>,
}

#[derive(Deserialize)]
struct OpenAiModel {
    id: String,
    #[serde(default)]
    owned_by: Option<String>,
}

fn native_result<T>(result: Result<T>) -> SfumatoResult<T> {
    result.map_err(|error| {
        if let Some(error) = error.downcast_ref::<SfumatoError>() {
            return error.clone();
        }
        SfumatoError::provider(ErrorClass::Unavailable, format_args!("{error:#}"))
            .at_stage(OperationStage::Resolve)
    })
}

#[cfg(test)]
#[path = "../tests/unit/lmstudio.rs"]
mod tests;
