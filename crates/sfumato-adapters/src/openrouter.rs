//! OpenRouter-native catalog and API-key usage adapter.

use std::{collections::BTreeMap, sync::Arc};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use sfumato_core::{
    config::OpenRouterConnectorConfig,
    errors::{ErrorClass, OperationStage, SfumatoError, SfumatoResult},
    operation::OperationContext,
    providers::{ConnectorModelSummary, ConnectorStatus, ConnectorStatusField},
    secrets::SecretResolver,
};

use crate::{openai_compatible::OpenAiCompatibleConnector, runtime::await_operation};

/// OpenRouter adapter composed around the shared OpenAI-compatible transport.
pub struct OpenRouterConnector {
    transport: OpenAiCompatibleConnector,
}

impl OpenRouterConnector {
    /// Creates native operations using the same connection and credential as generation.
    pub fn new(
        name: &str,
        config: &OpenRouterConnectorConfig,
        secrets: Arc<dyn SecretResolver>,
    ) -> SfumatoResult<Self> {
        OpenAiCompatibleConnector::new(name.to_string(), config.transport.clone(), secrets)
            .map(|transport| Self { transport })
            .map_err(|error| SfumatoError::config(format_args!("{error:#}")))
    }

    /// Lists the complete OpenRouter model catalog with modalities and pricing metadata.
    pub async fn list_models(
        &self,
        operation: &OperationContext,
    ) -> SfumatoResult<Vec<ConnectorModelSummary>> {
        let result: Result<Vec<ConnectorModelSummary>> = async {
            let request = self.transport.get("models").await?;
            let response =
                await_operation(operation, OperationStage::Resolve, request.send()).await?;
            let status = response.status();
            let body = await_operation(operation, OperationStage::Resolve, response.text()).await?;
            if !status.is_success() {
                bail!("OpenRouter model catalog returned HTTP {status}: {body}");
            }
            let response: ModelsResponse = serde_json::from_str(&body)
                .context("OpenRouter model catalog returned invalid JSON")?;
            Ok(response.data.into_iter().map(map_model).collect())
        }
        .await;
        native_result(result)
    }

    /// Reads usage and limit state for the configured OpenRouter API key.
    pub async fn status(
        &self,
        name: &str,
        operation: &OperationContext,
    ) -> SfumatoResult<ConnectorStatus> {
        let result: Result<ConnectorStatus> = async {
            let request = self.transport.get("key").await?;
            let response =
                await_operation(operation, OperationStage::Resolve, request.send()).await?;
            let status = response.status();
            let body = await_operation(operation, OperationStage::Resolve, response.text()).await?;
            if !status.is_success() {
                bail!("OpenRouter API-key status returned HTTP {status}: {body}");
            }
            let data: KeyResponse = serde_json::from_str(&body)
                .context("OpenRouter API-key status returned invalid JSON")?;
            let data = data.data;
            let mut fields = vec![field("label", data.label), field("usage_total", data.usage)];
            fields.push(field("usage_daily", data.usage_daily));
            fields.push(field("usage_weekly", data.usage_weekly));
            fields.push(field("usage_monthly", data.usage_monthly));
            fields.push(field("free_tier", data.is_free_tier));
            if let Some(limit) = data.limit {
                fields.push(field("credit_limit", limit));
            }
            if let Some(remaining) = data.limit_remaining {
                fields.push(field("credit_remaining", remaining));
            }
            if let Some(reset) = data.limit_reset {
                fields.push(field("limit_reset", reset));
            }
            Ok(ConnectorStatus {
                connector: name.into(),
                kind: "openrouter".into(),
                fields,
            })
        }
        .await;
        native_result(result)
    }
}

fn map_model(model: OpenRouterModel) -> ConnectorModelSummary {
    let mut metadata = BTreeMap::new();
    if let Some(pricing) = model.pricing {
        if let Some(value) = pricing.prompt {
            metadata.insert("prompt_price".into(), value);
        }
        if let Some(value) = pricing.completion {
            metadata.insert("completion_price".into(), value);
        }
        if let Some(value) = pricing.image {
            metadata.insert("image_price".into(), value);
        }
    }
    if !model.supported_parameters.is_empty() {
        metadata.insert("parameters".into(), model.supported_parameters.join(", "));
    }
    let (input_modalities, output_modalities) = model
        .architecture
        .map(|value| (value.input_modalities, value.output_modalities))
        .unwrap_or_default();
    ConnectorModelSummary {
        id: model.id.clone(),
        display_name: model.name.unwrap_or(model.id),
        is_default: false,
        hidden: false,
        input_modalities,
        output_modalities,
        context_length: model.context_length,
        description: model.description,
        metadata,
    }
}

#[derive(Deserialize)]
struct ModelsResponse {
    data: Vec<OpenRouterModel>,
}
#[derive(Deserialize)]
struct OpenRouterModel {
    id: String,
    name: Option<String>,
    description: Option<String>,
    context_length: Option<u64>,
    architecture: Option<Architecture>,
    pricing: Option<Pricing>,
    #[serde(default)]
    supported_parameters: Vec<String>,
}
#[derive(Deserialize, Default)]
struct Architecture {
    #[serde(default)]
    input_modalities: Vec<String>,
    #[serde(default)]
    output_modalities: Vec<String>,
}
#[derive(Deserialize)]
struct Pricing {
    prompt: Option<String>,
    completion: Option<String>,
    image: Option<String>,
}
#[derive(Deserialize)]
struct KeyResponse {
    data: KeyData,
}
#[derive(Deserialize)]
struct KeyData {
    label: String,
    limit: Option<f64>,
    limit_reset: Option<String>,
    limit_remaining: Option<f64>,
    usage: f64,
    usage_daily: f64,
    usage_weekly: f64,
    usage_monthly: f64,
    is_free_tier: bool,
}

fn field(name: &str, value: impl ToString) -> ConnectorStatusField {
    ConnectorStatusField {
        name: name.into(),
        value: value.to_string(),
    }
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
#[path = "../tests/unit/openrouter.rs"]
mod tests;
