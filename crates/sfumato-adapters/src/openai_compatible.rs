use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use reqwest::{Client, RequestBuilder};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc, time::Duration};

use sfumato_core::{
    config::{Capability, EffectiveConfig, ModelProfile, OpenAiCompatibleConnectorConfig},
    errors::{ErrorClass, OperationStage, SfumatoError, SfumatoResult},
    operation::OperationContext,
    providers::{
        AgentRunner, ImageAttachment, ImageGenerationProvider, ImageGenerationRequest,
        ImageGenerationResponse, ModelMessage, ProviderFactory, TextGenerationLimitError,
        TextGenerationProvider, TextModel, TextModelRequest, TextModelResponse, ToolCall,
        ToolDefinition,
    },
    secrets::SecretResolver,
};

use crate::runtime::await_operation;

const HTTP_REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Clone)]
pub struct OpenAiCompatibleConnector {
    client: Client,
    name: String,
    config: OpenAiCompatibleConnectorConfig,
    secrets: Arc<dyn SecretResolver>,
}

impl OpenAiCompatibleConnector {
    pub fn new(
        name: String,
        config: OpenAiCompatibleConnectorConfig,
        secrets: Arc<dyn SecretResolver>,
    ) -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .timeout(HTTP_REQUEST_TIMEOUT)
                .build()
                .context("Could not build the OpenAI-compatible HTTP client")?,
            name,
            config,
            secrets,
        })
    }

    pub fn endpoint(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.config.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    pub(crate) async fn post(&self, path: &str) -> Result<RequestBuilder> {
        let mut request = self.client.post(self.endpoint(path));
        if let Some(reference) = &self.config.credential {
            let api_key = self.secrets.resolve(reference).await?;
            request = request.bearer_auth(api_key.expose());
        }
        for (name, value) in &self.config.headers {
            request = request.header(name, value);
        }
        Ok(request)
    }

    pub(crate) async fn get(&self, path: &str) -> Result<RequestBuilder> {
        let mut request = self.client.get(self.endpoint(path));
        if let Some(reference) = &self.config.credential {
            let api_key = self.secrets.resolve(reference).await?;
            request = request.bearer_auth(api_key.expose());
        }
        for (name, value) in &self.config.headers {
            request = request.header(name, value);
        }
        Ok(request)
    }
}

/// Provider factory for connectors exposing OpenAI-compatible endpoints.
#[derive(Clone)]
pub struct OpenAiCompatibleProviderFactory {
    secrets: Arc<dyn SecretResolver>,
}

impl OpenAiCompatibleProviderFactory {
    /// Creates a provider factory that resolves credentials at request time.
    pub fn new(secrets: Arc<dyn SecretResolver>) -> Self {
        Self { secrets }
    }
}

impl ProviderFactory for OpenAiCompatibleProviderFactory {
    fn text(
        &self,
        config: &EffectiveConfig,
        profile: &ModelProfile,
    ) -> SfumatoResult<Box<dyn TextGenerationProvider>> {
        let result: Result<Box<dyn TextGenerationProvider>> = (|| {
            if !profile.capabilities.contains(&Capability::Text) {
                bail!("Selected model profile does not support text generation");
            }
            let connector = config
                .connectors
                .get(&profile.connector)
                .with_context(|| format!("Connector '{}' was not found", profile.connector))?
                .openai_compatible()
                .with_context(|| {
                    format!("Connector '{}' is not OpenAI-compatible", profile.connector)
                })?;
            let model = OpenAiCompatibleTextProvider::new(
                profile.connector.clone(),
                connector.clone(),
                profile.clone(),
                Arc::clone(&self.secrets),
            )?;
            Ok(Box::new(AgentRunner::new(std::sync::Arc::new(model)))
                as Box<dyn TextGenerationProvider>)
        })();
        result.map_err(|error| SfumatoError::config(format_args!("{error:#}")))
    }

    fn image(
        &self,
        config: &EffectiveConfig,
        profile: &ModelProfile,
    ) -> SfumatoResult<Box<dyn ImageGenerationProvider>> {
        let result: Result<Box<dyn ImageGenerationProvider>> = (|| {
            if !profile.capabilities.contains(&Capability::Image) {
                bail!("Selected model profile does not support image generation");
            }
            let connector = config
                .connectors
                .get(&profile.connector)
                .with_context(|| format!("Connector '{}' was not found", profile.connector))?
                .openai_compatible()
                .with_context(|| {
                    format!("Connector '{}' is not OpenAI-compatible", profile.connector)
                })?;
            Ok(Box::new(OpenAiCompatibleImageProvider::new(
                profile.connector.clone(),
                connector.clone(),
                profile.clone(),
                Arc::clone(&self.secrets),
            )?) as Box<dyn ImageGenerationProvider>)
        })();
        result.map_err(|error| SfumatoError::config(format_args!("{error:#}")))
    }

    fn video(
        &self,
        _config: &EffectiveConfig,
        _profile: &ModelProfile,
    ) -> SfumatoResult<Box<dyn sfumato_core::providers::VideoGenerationProvider>> {
        Err(SfumatoError::config(
            "Video generation requires a provider-native connector such as OpenRouter",
        ))
    }

    fn speech(
        &self,
        _config: &EffectiveConfig,
        _profile: &ModelProfile,
    ) -> SfumatoResult<Box<dyn sfumato_core::providers::SpeechGenerationProvider>> {
        Err(SfumatoError::config(
            "Speech generation requires a provider-native connector such as ElevenLabs",
        ))
    }
}

#[derive(Clone)]
pub struct OpenAiCompatibleTextProvider {
    connector: OpenAiCompatibleConnector,
    profile: ModelProfile,
}

impl OpenAiCompatibleTextProvider {
    pub fn new(
        connector_name: String,
        connector: OpenAiCompatibleConnectorConfig,
        profile: ModelProfile,
        secrets: Arc<dyn SecretResolver>,
    ) -> Result<Self> {
        Ok(Self {
            connector: OpenAiCompatibleConnector::new(connector_name, connector, secrets)?,
            profile,
        })
    }

    /// Serializes one provider-neutral model turn as a chat-completions request.
    #[cfg(test)]
    pub fn request_body(&self, request: &TextModelRequest) -> ChatCompletionsRequest {
        self.request_body_for_messages(
            request
                .messages
                .clone()
                .into_iter()
                .map(ChatMessage::from)
                .collect(),
            (!request.tools.is_empty()).then_some(request.tools.clone()),
        )
    }

    fn request_body_for_messages(
        &self,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<ToolDefinition>>,
    ) -> ChatCompletionsRequest {
        ChatCompletionsRequest {
            model: self.profile.model.clone(),
            messages,
            tools,
            temperature: self.profile.options.text_temperature(),
            max_tokens: self.profile.options.text_max_tokens(),
            top_p: self.profile.options.text.top_p,
            seed: self.profile.options.text.seed,
            stream: false,
        }
    }
}

#[async_trait]
impl TextModel for OpenAiCompatibleTextProvider {
    async fn complete(
        &self,
        request: TextModelRequest,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<TextModelResponse> {
        let result: Result<TextModelResponse> = async {
            let messages = request
                .messages
                .into_iter()
                .map(ChatMessage::from)
                .collect();
            let tools = (!request.tools.is_empty()).then_some(request.tools);
            let parsed = self
                .send_chat_completion(
                    &self.request_body_for_messages(messages, tools),
                    operation,
                    stage,
                )
                .await?;
            let usage = parsed.usage;
            let choice = parsed
                .choices
                .into_iter()
                .next()
                .context("Connector response did not include any choices")?;
            let finish_reason = choice.finish_reason;
            let assistant = choice.message;
            let assistant_text = assistant.content.as_ref().and_then(MessageContent::text);
            if assistant.tool_calls.is_empty() {
                let content = assistant_text.as_deref().unwrap_or_default().trim();
                if content.is_empty() {
                    return Err(empty_content_error(
                        &self.profile,
                        finish_reason.as_deref(),
                        usage.as_ref(),
                    ));
                }
                ensure_text_response_complete(
                    &self.profile,
                    finish_reason.as_deref(),
                    usage.as_ref(),
                )?;
            }
            Ok(TextModelResponse {
                content: assistant_text,
                tool_calls: assistant.tool_calls,
            })
        }
        .await;
        result.map_err(|error| provider_error(error, stage))
    }
}

#[derive(Clone)]
pub struct OpenAiCompatibleImageProvider {
    connector: OpenAiCompatibleConnector,
    profile: ModelProfile,
}

impl OpenAiCompatibleImageProvider {
    pub fn new(
        connector_name: String,
        connector: OpenAiCompatibleConnectorConfig,
        profile: ModelProfile,
        secrets: Arc<dyn SecretResolver>,
    ) -> Result<Self> {
        Ok(Self {
            connector: OpenAiCompatibleConnector::new(connector_name, connector, secrets)?,
            profile,
        })
    }

    pub fn request_body(&self, request: &ImageGenerationRequest) -> Result<ImageRequest> {
        let mut options = BTreeMap::new();
        insert_image_option(&mut options, "quality", &self.profile.options.image.quality);
        insert_image_option(
            &mut options,
            "background",
            &self.profile.options.image.background,
        );
        insert_image_option(&mut options, "size", &self.profile.options.image.size);
        insert_image_option(
            &mut options,
            "aspect_ratio",
            &self.profile.options.image.aspect_ratio,
        );
        insert_image_option(
            &mut options,
            "output_format",
            &self.profile.options.image.output_format,
        );
        Ok(ImageRequest {
            model: self.profile.model.clone(),
            prompt: request.prompt.clone(),
            n: 1,
            options,
        })
    }
}

#[async_trait]
impl ImageGenerationProvider for OpenAiCompatibleImageProvider {
    async fn generate_image(
        &self,
        request: ImageGenerationRequest,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<ImageGenerationResponse> {
        let result: Result<ImageGenerationResponse> = async {
            let body = self.request_body(&request)?;
            let response = await_operation(
                operation,
                stage,
                self.connector.post("images").await?.json(&body).send(),
            )
            .await
            .with_context(|| {
                format!(
                    "Could not reach OpenAI-compatible connector '{}' for image generation",
                    self.connector.name
                )
            })?;
            let status = response.status();
            let text = await_operation(operation, stage, response.text())
                .await
                .context("Could not read image generation response body")?;
            if !status.is_success() {
                bail!(
                    "OpenAI-compatible connector '{}' returned HTTP {}: {}",
                    self.connector.name,
                    status,
                    text
                );
            }
            let parsed: ImageResponse =
                serde_json::from_str(&text).context("Could not parse image generation response")?;
            let image = parsed
                .data
                .into_iter()
                .next()
                .context("Image generation response did not include an image")?;
            let bytes = STANDARD
                .decode(&image.b64_json)
                .context("Image generation response contained invalid base64")?;
            if bytes.is_empty() {
                bail!("Image generation response contained an empty image");
            }
            Ok(ImageGenerationResponse {
                bytes,
                media_type: image.media_type.unwrap_or_else(|| "image/png".to_string()),
            })
        }
        .await;
        result.map_err(|error| provider_error(error, stage))
    }
}

fn provider_error(error: anyhow::Error, stage: OperationStage) -> SfumatoError {
    if let Some(error) = error.downcast_ref::<SfumatoError>() {
        let mut error = error.clone();
        if error.stage.is_none() {
            error.stage = Some(stage);
        }
        return error;
    }
    if let Some(limit) = error.downcast_ref::<TextGenerationLimitError>() {
        return SfumatoError::from(limit.clone()).at_stage(stage);
    }

    let message = format!("{error:#}");
    let class = if message.contains("Could not reach")
        || message.contains("operation timed out")
        || message.contains("timed out")
        || message.contains("HTTP 429")
        || message.contains("HTTP 500")
        || message.contains("HTTP 502")
        || message.contains("HTTP 503")
        || message.contains("HTTP 504")
    {
        ErrorClass::Retry
    } else {
        ErrorClass::Permanent
    };
    SfumatoError::provider(class, message).at_stage(stage)
}

#[derive(Debug, Serialize)]
pub struct ImageRequest {
    pub model: String,
    pub prompt: String,
    pub n: u8,
    #[serde(flatten)]
    pub options: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct ImageResponse {
    data: Vec<ImageResponseData>,
}

#[derive(Debug, Deserialize)]
struct ImageResponseData {
    b64_json: String,
    #[serde(default)]
    media_type: Option<String>,
}

impl OpenAiCompatibleTextProvider {
    async fn send_chat_completion(
        &self,
        body: &ChatCompletionsRequest,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> Result<ChatCompletionsResponse> {
        let response = await_operation(
            operation,
            stage,
            self.connector
                .post("chat/completions")
                .await?
                .json(body)
                .send(),
        )
        .await
        .with_context(|| {
            format!(
                "Could not reach OpenAI-compatible connector '{}'",
                self.connector.name
            )
        })?;
        let status = response.status();
        let text = await_operation(operation, stage, response.text())
            .await
            .context("Could not read connector response body")?;
        if !status.is_success() {
            if is_context_limit_response(&text) {
                return Err(TextGenerationLimitError::context(
                    self.profile.model.clone(),
                    u64::from(self.profile.options.text_max_tokens()),
                    compact_error_detail(&text),
                )
                .into());
            }
            bail!(
                "OpenAI-compatible connector '{}' returned HTTP {}: {}",
                self.connector.name,
                status,
                text
            );
        }
        serde_json::from_str(&text).context("Could not parse chat completions response")
    }
}

fn insert_image_option(
    options: &mut BTreeMap<String, serde_json::Value>,
    key: &str,
    value: &Option<String>,
) {
    if let Some(value) = value {
        options.insert(key.to_string(), serde_json::Value::String(value.clone()));
    }
}

fn empty_content_error(
    profile: &ModelProfile,
    finish_reason: Option<&str>,
    usage: Option<&CompletionUsage>,
) -> anyhow::Error {
    if matches!(finish_reason, Some("length" | "max_tokens")) {
        return TextGenerationLimitError::output(
            profile.model.clone(),
            u64::from(profile.options.text_max_tokens()),
            finish_reason.map(ToOwned::to_owned),
            usage.and_then(|usage| usage.completion_tokens),
            reasoning_tokens(usage),
            true,
        )
        .into();
    }
    anyhow::anyhow!(
        "Connector response did not include text content (finish reason: {}).",
        finish_reason.unwrap_or("unknown")
    )
}

fn ensure_text_response_complete(
    profile: &ModelProfile,
    finish_reason: Option<&str>,
    usage: Option<&CompletionUsage>,
) -> Result<()> {
    if !matches!(finish_reason, Some("length" | "max_tokens")) {
        return Ok(());
    }

    Err(TextGenerationLimitError::output(
        profile.model.clone(),
        u64::from(profile.options.text_max_tokens()),
        finish_reason.map(ToOwned::to_owned),
        usage.and_then(|usage| usage.completion_tokens),
        reasoning_tokens(usage),
        false,
    )
    .into())
}

fn reasoning_tokens(usage: Option<&CompletionUsage>) -> Option<u64> {
    usage
        .and_then(|usage| usage.completion_tokens_details.as_ref())
        .and_then(|details| details.reasoning_tokens)
}

pub(crate) fn is_context_limit_response(body: &str) -> bool {
    let normalized = body.to_ascii_lowercase();
    let structured_code = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            value
                .pointer("/error/code")
                .and_then(serde_json::Value::as_str)
                .map(str::to_ascii_lowercase)
        });
    matches!(
        structured_code.as_deref(),
        Some("context_length_exceeded" | "max_context_length" | "context_window_exceeded")
    ) || normalized.contains("context length")
        || normalized.contains("maximum context")
        || normalized.contains("context window")
        || normalized.contains("prompt is too long")
        || normalized.contains("too many tokens")
}

pub(crate) fn compact_error_detail(body: &str) -> String {
    let detail = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            value
                .pointer("/error/message")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| body.to_string());
    let mut compact = detail.split_whitespace().collect::<Vec<_>>().join(" ");
    compact.truncate(compact.floor_char_boundary(500));
    compact
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionsRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,
    pub temperature: f32,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    pub stream: bool,
}

/// What one chat message carries.
///
/// `chat/completions` accepts either a bare string or an array of typed parts on
/// the same `content` field, and only the array form can carry an image. Untagged
/// so a plain-text message serialises exactly as it did before this existed —
/// several connectors reject the array form for system and tool roles.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum MessageContent {
    /// A single block of text.
    Text(String),
    /// Interleaved text and images.
    Parts(Vec<ContentPart>),
}

impl MessageContent {
    /// The assistant text this content carries, if any.
    ///
    /// Models answer in text even when asked about images, but a connector is
    /// free to reply in the array form, so both shapes have to read back.
    fn text(&self) -> Option<String> {
        match self {
            Self::Text(value) => Some(value.clone()),
            Self::Parts(parts) => {
                let joined = parts
                    .iter()
                    .filter_map(|part| match part {
                        ContentPart::Text { text } => Some(text.as_str()),
                        ContentPart::ImageUrl { .. } => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                (!joined.is_empty()).then_some(joined)
            }
        }
    }
}

/// One typed part of a multi-part chat message.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    /// Literal text.
    Text {
        /// The text itself.
        text: String,
    },
    /// An image referenced by URL, including a `data:` URI.
    ImageUrl {
        /// The wrapper object the API requires around the URL.
        image_url: ImageUrl,
    },
}

/// The URL wrapper `chat/completions` requires around an image reference.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ImageUrl {
    /// An `https:` or `data:` URL.
    pub url: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<MessageContent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_details: Option<serde_json::Value>,
}

impl ChatMessage {
    fn text(role: &str, content: String) -> Self {
        Self {
            role: role.to_string(),
            content: Some(MessageContent::Text(content)),
            tool_calls: Vec::new(),
            tool_call_id: None,
            reasoning: None,
            reasoning_details: None,
        }
    }

    /// Builds a user turn whose text is followed by labelled images.
    ///
    /// Each label precedes its own image so the model can name what it is looking
    /// at; a bare run of frames gives it no way to report which one is wrong. An
    /// image that cannot be read is dropped with its label kept, so the model is
    /// told a frame is missing instead of silently judging a shorter film.
    fn with_images(content: String, images: Vec<ImageAttachment>) -> Self {
        let mut parts = vec![ContentPart::Text { text: content }];
        for image in images {
            match data_uri(&image) {
                Ok(url) => {
                    parts.push(ContentPart::Text {
                        text: image.label.clone(),
                    });
                    parts.push(ContentPart::ImageUrl {
                        image_url: ImageUrl { url },
                    });
                }
                Err(error) => parts.push(ContentPart::Text {
                    text: format!("{} could not be read: {error}", image.label),
                }),
            }
        }
        Self {
            role: "user".to_string(),
            content: Some(MessageContent::Parts(parts)),
            tool_calls: Vec::new(),
            tool_call_id: None,
            reasoning: None,
            reasoning_details: None,
        }
    }
}

/// Encodes one attachment as the `data:` URI the API accepts inline.
fn data_uri(image: &ImageAttachment) -> Result<String> {
    let bytes = std::fs::read(&image.path)
        .with_context(|| format!("Could not read {}", image.path.display()))?;
    Ok(format!(
        "data:{};base64,{}",
        image.media_type,
        STANDARD.encode(&bytes)
    ))
}

impl From<ModelMessage> for ChatMessage {
    fn from(message: ModelMessage) -> Self {
        match message {
            ModelMessage::System(content) => Self::text("system", content),
            ModelMessage::User(content) => Self::text("user", content),
            ModelMessage::UserWithImages { content, images } => Self::with_images(content, images),
            ModelMessage::Assistant {
                content,
                tool_calls,
            } => Self {
                role: "assistant".to_string(),
                content: content.map(MessageContent::Text),
                tool_calls,
                tool_call_id: None,
                reasoning: None,
                reasoning_details: None,
            },
            // `failed` is dropped deliberately: `chat/completions` tool messages
            // carry no error flag, so the JSON error body is the only signal.
            ModelMessage::Tool {
                tool_call_id,
                name: _,
                content,
                failed: _,
            } => Self {
                role: "tool".to_string(),
                content: Some(MessageContent::Text(content)),
                tool_calls: Vec::new(),
                tool_call_id,
                reasoning: None,
                reasoning_details: None,
            },
        }
    }
}

#[derive(Debug, Deserialize)]
struct ChatCompletionsResponse {
    choices: Vec<ChatChoice>,
    #[serde(default)]
    usage: Option<CompletionUsage>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CompletionUsage {
    #[serde(default)]
    completion_tokens: Option<u64>,
    #[serde(default)]
    completion_tokens_details: Option<CompletionTokenDetails>,
}

#[derive(Debug, Deserialize)]
struct CompletionTokenDetails {
    #[serde(default)]
    reasoning_tokens: Option<u64>,
}

#[cfg(test)]
#[path = "../tests/unit/openai_compatible.rs"]
mod tests;
