//! Ollama-native local model catalog and runtime status adapter.

use std::{collections::BTreeMap, time::Duration};

use anyhow::{Context, Result, bail};
use reqwest::Client;
use serde::Deserialize;
use sfumato_core::{
    config::OllamaConnectorConfig,
    errors::{ErrorClass, OperationStage, SfumatoError, SfumatoResult},
    operation::OperationContext,
    providers::{ConnectorModelSummary, ConnectorStatus, ConnectorStatusField},
};

use crate::runtime::await_operation;

/// `/api/tags`, `/api/version`, and `/api/ps` all answer in well under a second
/// against a local daemon, and the CLI and TUI run them with a context that may
/// carry no deadline, so this is the only bound on a hung introspection call. A
/// port that accepts the connection and never replies — a wedged Ollama, or
/// something else listening on 11434 — would otherwise hang the process forever.
const INTROSPECTION_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// Ollama adapter paired with the connector's shared OpenAI-compatible transport.
pub struct OllamaConnector {
    client: Client,
    native_base_url: String,
}

impl OllamaConnector {
    /// Creates native Ollama operations.
    pub fn new(config: &OllamaConnectorConfig) -> SfumatoResult<Self> {
        Client::builder()
            .timeout(INTROSPECTION_REQUEST_TIMEOUT)
            .build()
            .map(|client| Self {
                client,
                native_base_url: config.native_base_url.trim_end_matches('/').to_string(),
            })
            .map_err(|error| {
                SfumatoError::config(format_args!(
                    "Could not build the Ollama HTTP client: {error}"
                ))
            })
    }

    /// Lists locally installed models through `/api/tags`.
    pub async fn list_models(
        &self,
        operation: &OperationContext,
    ) -> SfumatoResult<Vec<ConnectorModelSummary>> {
        let result: Result<Vec<ConnectorModelSummary>> = async {
            let response: TagsResponse = self.get_json("/api/tags", operation).await?;
            Ok(response.models.into_iter().map(map_model).collect())
        }
        .await;
        native_result(result)
    }

    /// Reads Ollama version and currently loaded model state.
    pub async fn status(
        &self,
        name: &str,
        operation: &OperationContext,
    ) -> SfumatoResult<ConnectorStatus> {
        let result: Result<ConnectorStatus> = async {
            let version: VersionResponse = self.get_json("/api/version", operation).await?;
            let running: RunningResponse = self.get_json("/api/ps", operation).await?;
            Ok(ConnectorStatus {
                connector: name.into(),
                kind: "ollama".into(),
                fields: vec![
                    ConnectorStatusField {
                        name: "version".into(),
                        value: version.version,
                    },
                    ConnectorStatusField {
                        name: "running_models".into(),
                        value: running.models.len().to_string(),
                    },
                    ConnectorStatusField {
                        name: "loaded".into(),
                        value: running
                            .models
                            .into_iter()
                            .map(|model| model.name)
                            .collect::<Vec<_>>()
                            .join(", "),
                    },
                ],
            })
        }
        .await;
        native_result(result)
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        operation: &OperationContext,
    ) -> Result<T> {
        let response = await_operation(
            operation,
            OperationStage::Resolve,
            self.client
                .get(format!("{}{}", self.native_base_url, path))
                .send(),
        )
        .await?;
        let status = response.status();
        let body = await_operation(operation, OperationStage::Resolve, response.text()).await?;
        if !status.is_success() {
            bail!("Ollama endpoint '{path}' returned HTTP {status}: {body}");
        }
        serde_json::from_str(&body)
            .with_context(|| format!("Ollama endpoint '{path}' returned invalid JSON"))
    }
}

fn map_model(model: TagModel) -> ConnectorModelSummary {
    let mut metadata = BTreeMap::from([
        ("size_bytes".into(), model.size.to_string()),
        ("digest".into(), model.digest),
    ]);
    if let Some(details) = model.details {
        if let Some(value) = details.family {
            metadata.insert("family".into(), value);
        }
        if let Some(value) = details.parameter_size {
            metadata.insert("parameters".into(), value);
        }
        if let Some(value) = details.quantization_level {
            metadata.insert("quantization".into(), value);
        }
    }
    ConnectorModelSummary {
        id: model.model.clone(),
        display_name: model.name.unwrap_or(model.model),
        is_default: false,
        hidden: false,
        input_modalities: vec!["text".into()],
        output_modalities: vec!["text".into()],
        context_length: None,
        description: None,
        metadata,
    }
}

#[derive(Deserialize)]
struct TagsResponse {
    models: Vec<TagModel>,
}
#[derive(Deserialize)]
struct TagModel {
    name: Option<String>,
    model: String,
    size: u64,
    digest: String,
    details: Option<TagDetails>,
}
#[derive(Deserialize)]
struct TagDetails {
    family: Option<String>,
    parameter_size: Option<String>,
    quantization_level: Option<String>,
}
#[derive(Deserialize)]
struct VersionResponse {
    version: String,
}
#[derive(Deserialize)]
struct RunningResponse {
    #[serde(default)]
    models: Vec<RunningModel>,
}
#[derive(Deserialize)]
struct RunningModel {
    name: String,
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
#[path = "../tests/unit/ollama.rs"]
mod tests;
