//! OpenRouter-native catalog and API-key usage adapter.

use std::{collections::BTreeMap, path::Path, sync::Arc, time::Duration};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use sfumato_core::{
    config::{ModelProfile, OpenRouterConnectorConfig},
    errors::{ErrorClass, OperationStage, SfumatoError, SfumatoResult},
    operation::OperationContext,
    providers::{
        ConnectorModelSummary, ConnectorStatus, ConnectorStatusField, VideoGenerationProvider,
        VideoGenerationRequest, VideoGenerationResponse,
    },
    retry::RetryPolicy,
    secrets::SecretResolver,
};

use crate::{
    openai_compatible::{OpenAiCompatibleConnector, provider_status_error},
    retry::with_retry,
    runtime::await_operation,
};

/// OpenRouter adapter composed around the shared OpenAI-compatible transport.
#[derive(Clone)]
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
                return Err(provider_status_error(
                    "OpenRouter",
                    "the model catalog",
                    status,
                    &body,
                )
                .into());
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
                return Err(
                    provider_status_error("OpenRouter", "API-key status", status, &body).into(),
                );
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

    /// Creates a provider for OpenRouter's asynchronous video API.
    pub fn video_provider(&self, profile: ModelProfile) -> OpenRouterVideoProvider {
        OpenRouterVideoProvider {
            transport: self.transport.clone(),
            profile,
        }
    }
}

/// OpenRouter-native asynchronous text/image/reference-to-video provider.
pub struct OpenRouterVideoProvider {
    transport: OpenAiCompatibleConnector,
    profile: ModelProfile,
}

#[async_trait]
impl VideoGenerationProvider for OpenRouterVideoProvider {
    async fn generate_video(
        &self,
        request: VideoGenerationRequest,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<VideoGenerationResponse> {
        native_result(self.generate(request, operation, stage).await)
    }
}

impl OpenRouterVideoProvider {
    async fn generate(
        &self,
        request: VideoGenerationRequest,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> Result<VideoGenerationResponse> {
        let model = self.video_model(operation, stage).await?;
        validate_video_request(&model, &request)?;
        let input_references = if model.supports_input_references() {
            request
                .references
                .iter()
                .map(|path| reference_payload(path))
                .collect::<Result<Vec<_>>>()?
        } else {
            Vec::new()
        };
        let payload = CreateVideoRequest {
            model: self.profile.model.clone(),
            prompt: request.prompt,
            duration: request.duration_seconds,
            resolution: request.resolution,
            aspect_ratio: request.aspect_ratio,
            generate_audio: request.generate_audio,
            seed: request.seed,
            input_references,
        };
        // Retried here, around the submit alone, which is the step that can be
        // repeated safely: a rejected submit started no job, so unlike wrapping
        // `generate_video` this cannot bill a second render for one already
        // running. A 429 or a 503 on submit used to abort the whole operation,
        // which is exactly the failure the transport retries were added for.
        let submitted: VideoJob = with_retry(
            RetryPolicy::default(),
            operation,
            stage,
            "video submission",
            || async {
                let submit = self
                    .transport
                    .post("videos")
                    .await
                    .map_err(|error| {
                        SfumatoError::provider(ErrorClass::Permanent, format!("{error:#}"))
                    })?
                    .json(&payload)
                    .send();
                let response = await_operation(operation, stage, submit)
                    .await
                    .map_err(|error| transient_transport_error("video submission", error))?;
                let status = response.status();
                let body = await_operation(operation, stage, response.text())
                    .await
                    .map_err(|error| transient_transport_error("video submission", error))?;
                if !status.is_success() {
                    return Err(provider_status_error(
                        "OpenRouter",
                        "video generation",
                        status,
                        &body,
                    ));
                }
                serde_json::from_str(&body).map_err(|error| {
                    SfumatoError::provider(
                        ErrorClass::InvalidOutput,
                        format!("OpenRouter video generation returned invalid JSON: {error}"),
                    )
                })
            },
        )
        .await?;
        let poll_seconds = self
            .profile
            .options
            .video
            .poll_interval_seconds
            .unwrap_or(10);
        let timeout_seconds = self.profile.options.video.timeout_seconds.unwrap_or(900);
        let started = std::time::Instant::now();
        let mut transient_polls = 0_u32;
        let completed = loop {
            operation.checkpoint(stage)?;
            if started.elapsed() > Duration::from_secs(timeout_seconds) {
                bail!(
                    "OpenRouter video job '{}' exceeded {timeout_seconds} seconds",
                    submitted.id
                );
            }
            let status_request = self
                .transport
                .get(&format!("videos/{}", submitted.id))
                .await?
                .send();
            // A status read is idempotent, and by this point the render is
            // already running and already billed. Abandoning the job because one
            // poll was rate limited threw away the thing that cost money, and on
            // a long render a transient failure somewhere in the loop is the more
            // likely case. Transient failures are tolerated up to a bound; a
            // permanent one still stops immediately.
            let poll = async {
                let response = await_operation(operation, stage, status_request)
                    .await
                    .map_err(|error| transient_transport_error("video status", error))?;
                let status = response.status();
                let body = await_operation(operation, stage, response.text())
                    .await
                    .map_err(|error| transient_transport_error("video status", error))?;
                if !status.is_success() {
                    return Err(provider_status_error(
                        "OpenRouter",
                        "video status",
                        status,
                        &body,
                    ));
                }
                serde_json::from_str::<VideoJob>(&body).map_err(|error| {
                    SfumatoError::provider(
                        ErrorClass::Retry,
                        format_args!("OpenRouter video status returned invalid JSON: {error}"),
                    )
                })
            };
            let job = match poll.await {
                Ok(job) => {
                    transient_polls = 0;
                    job
                }
                Err(error) if error.retryable && transient_polls < MAX_TRANSIENT_POLLS => {
                    transient_polls += 1;
                    await_operation(operation, stage, async move {
                        tokio::time::sleep(Duration::from_secs(poll_seconds)).await;
                        Ok::<(), anyhow::Error>(())
                    })
                    .await?;
                    continue;
                }
                Err(error) => return Err(error.into()),
            };
            match job.status.as_str() {
                "completed" => break job,
                "failed" | "cancelled" | "expired" => {
                    bail!(
                        "OpenRouter video job '{}' {}: {}",
                        job.id,
                        job.status,
                        job.error.unwrap_or_else(|| "unknown provider error".into())
                    );
                }
                "pending" | "in_progress" => {}
                other => bail!("OpenRouter video job returned unknown status '{other}'"),
            }
            await_operation(operation, stage, async move {
                tokio::time::sleep(Duration::from_secs(poll_seconds)).await;
                Ok::<(), anyhow::Error>(())
            })
            .await?;
        };
        let download = self
            .transport
            .get(&format!("videos/{}/content?index=0", completed.id))
            .await?
            .send();
        let response = await_operation(operation, stage, download).await?;
        let status = response.status();
        let media_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("video/mp4")
            .split(';')
            .next()
            .unwrap_or("video/mp4")
            .to_string();
        let bytes = await_operation(operation, stage, response.bytes())
            .await?
            .to_vec();
        if !status.is_success() {
            bail!("OpenRouter video download returned HTTP {status}");
        }
        if bytes.is_empty() {
            bail!("OpenRouter video download returned an empty file");
        }
        Ok(VideoGenerationResponse {
            bytes,
            media_type,
            provider_job_id: Some(completed.id),
            cost: completed.usage.and_then(|usage| usage.cost),
        })
    }

    async fn video_model(
        &self,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> Result<VideoModel> {
        let request = self.transport.get("videos/models").await?.send();
        let response = await_operation(operation, stage, request).await?;
        let status = response.status();
        let body = await_operation(operation, stage, response.text()).await?;
        if !status.is_success() {
            bail!("OpenRouter video model catalog returned HTTP {status}: {body}");
        }
        let catalog: VideoModelsResponse = serde_json::from_str(&body)
            .context("OpenRouter video model catalog returned invalid JSON")?;
        catalog
            .data
            .into_iter()
            .find(|model| model.id == self.profile.model)
            .with_context(|| format!("'{}' is not an OpenRouter video model", self.profile.model))
    }
}

#[derive(Serialize)]
struct CreateVideoRequest {
    model: String,
    prompt: String,
    duration: u32,
    resolution: String,
    aspect_ratio: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    generate_audio: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    seed: Option<i64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    input_references: Vec<VideoReference>,
}

#[derive(Serialize)]
struct VideoReference {
    #[serde(rename = "type")]
    kind: &'static str,
    image_url: VideoReferenceUrl,
}

#[derive(Serialize)]
struct VideoReferenceUrl {
    url: String,
}

fn reference_payload(path: &Path) -> Result<VideoReference> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("Could not read video reference {}", path.display()))?;
    let media_type = match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        other => bail!(
            "Video reference '{}' has unsupported extension '{other}'",
            path.display()
        ),
    };
    Ok(VideoReference {
        kind: "image_url",
        image_url: VideoReferenceUrl {
            url: format!("data:{media_type};base64,{}", STANDARD.encode(bytes)),
        },
    })
}

#[derive(Deserialize)]
struct VideoModelsResponse {
    data: Vec<VideoModel>,
}

#[derive(Deserialize)]
struct VideoModel {
    id: String,
    #[serde(default)]
    supported_durations: Vec<u32>,
    #[serde(default)]
    supported_resolutions: Vec<String>,
    #[serde(default)]
    supported_aspect_ratios: Vec<String>,
    #[serde(default)]
    supported_parameters: Vec<String>,
    #[serde(default)]
    generate_audio: Option<bool>,
}

impl VideoModel {
    fn supports_input_references(&self) -> bool {
        self.supported_parameters
            .iter()
            .any(|parameter| parameter == "input_references")
    }
}

fn validate_video_request(model: &VideoModel, request: &VideoGenerationRequest) -> Result<()> {
    if !model.supported_durations.is_empty()
        && !model
            .supported_durations
            .contains(&request.duration_seconds)
    {
        bail!(
            "Video model '{}' does not support duration {}s",
            model.id,
            request.duration_seconds
        );
    }
    if !model.supported_resolutions.is_empty()
        && !model.supported_resolutions.contains(&request.resolution)
    {
        bail!(
            "Video model '{}' does not support resolution '{}'",
            model.id,
            request.resolution
        );
    }
    if !model.supported_aspect_ratios.is_empty()
        && !model
            .supported_aspect_ratios
            .contains(&request.aspect_ratio)
    {
        bail!(
            "Video model '{}' does not support aspect ratio '{}'",
            model.id,
            request.aspect_ratio
        );
    }
    if request.generate_audio == Some(true) && model.generate_audio == Some(false) {
        bail!("Video model '{}' does not support native audio", model.id);
    }
    Ok(())
}

#[derive(Deserialize)]
struct VideoJob {
    id: String,
    status: String,
    error: Option<String>,
    usage: Option<VideoUsage>,
}

#[derive(Deserialize)]
struct VideoUsage {
    cost: Option<f64>,
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

/// Consecutive transient poll failures tolerated before the job is abandoned.
///
/// The loop is already bounded by the job timeout, so this only stops a permanent
/// fault that keeps presenting as a retryable one from spinning until then.
const MAX_TRANSIENT_POLLS: u32 = 5;

/// Classifies a transport failure that never produced a response.
///
/// A request that did not reach the provider started no job, so repeating it
/// cannot double-bill. An operation cancellation or deadline is already a typed
/// error and is passed through so a retry does not outlive it.
fn transient_transport_error(what: &str, error: anyhow::Error) -> SfumatoError {
    if let Some(typed) = error.downcast_ref::<SfumatoError>() {
        return typed.clone();
    }
    SfumatoError::provider(
        ErrorClass::Retry,
        format_args!("OpenRouter {what} could not reach the provider: {error:#}"),
    )
    .at_stage(OperationStage::Render)
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
