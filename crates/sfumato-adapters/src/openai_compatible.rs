use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use reqwest::{Client, RequestBuilder};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, time::Duration};

use sfumato_core::{
    config::{Capability, EffectiveConfig, ModelProfile, OpenAiCompatibleConnectorConfig},
    providers::{
        AgentRunner, ImageGenerationProvider, ImageGenerationRequest, ImageGenerationResponse,
        ModelMessage, ProviderFactory, TextGenerationLimitError, TextGenerationProvider, TextModel,
        TextModelRequest, TextModelResponse, ToolCall, ToolDefinition,
    },
};

#[derive(Clone, Debug)]
pub struct OpenAiCompatibleConnector {
    client: Client,
    name: String,
    config: OpenAiCompatibleConnectorConfig,
}

impl OpenAiCompatibleConnector {
    pub fn new(name: String, config: OpenAiCompatibleConnectorConfig) -> Result<Self> {
        let _ = resolve_api_key(&config)?;
        Ok(Self {
            client: Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .context("Could not build the OpenAI-compatible HTTP client")?,
            name,
            config,
        })
    }

    pub fn endpoint(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.config.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    fn post(&self, path: &str) -> Result<RequestBuilder> {
        let mut request = self.client.post(self.endpoint(path));
        if let Some(api_key) = resolve_api_key(&self.config)? {
            request = request.bearer_auth(api_key);
        }
        for (name, value) in &self.config.headers {
            request = request.header(name, value);
        }
        Ok(request)
    }
}

/// Provider factory for connectors exposing OpenAI-compatible endpoints.
#[derive(Clone, Copy, Debug, Default)]
pub struct OpenAiCompatibleProviderFactory;

impl ProviderFactory for OpenAiCompatibleProviderFactory {
    fn text(
        &self,
        config: &EffectiveConfig,
        profile: &ModelProfile,
    ) -> Result<Box<dyn TextGenerationProvider>> {
        if !profile.capabilities.contains(&Capability::Text) {
            bail!("Selected model profile does not support text generation");
        }
        let connector = config
            .connectors
            .get(&profile.connector)
            .with_context(|| format!("Connector '{}' was not found", profile.connector))?;
        let model = OpenAiCompatibleTextProvider::new(
            profile.connector.clone(),
            connector.clone(),
            profile.clone(),
        )?;
        Ok(Box::new(AgentRunner::new(std::sync::Arc::new(model))))
    }

    fn image(
        &self,
        config: &EffectiveConfig,
        profile: &ModelProfile,
    ) -> Result<Box<dyn ImageGenerationProvider>> {
        if !profile.capabilities.contains(&Capability::Image) {
            bail!("Selected model profile does not support image generation");
        }
        let connector = config
            .connectors
            .get(&profile.connector)
            .with_context(|| format!("Connector '{}' was not found", profile.connector))?;
        Ok(Box::new(OpenAiCompatibleImageProvider::new(
            profile.connector.clone(),
            connector.clone(),
            profile.clone(),
        )?))
    }
}

#[derive(Clone, Debug)]
pub struct OpenAiCompatibleTextProvider {
    connector: OpenAiCompatibleConnector,
    profile: ModelProfile,
}

impl OpenAiCompatibleTextProvider {
    pub fn new(
        connector_name: String,
        connector: OpenAiCompatibleConnectorConfig,
        profile: ModelProfile,
    ) -> Result<Self> {
        Ok(Self {
            connector: OpenAiCompatibleConnector::new(connector_name, connector)?,
            profile,
        })
    }

    /// Serializes one provider-neutral model turn as a chat-completions request.
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
            temperature: option_float(&self.profile, "temperature", 0.4),
            max_tokens: option_integer(&self.profile, "max_tokens", 4000) as u32,
            stream: false,
        }
    }
}

#[async_trait]
impl TextModel for OpenAiCompatibleTextProvider {
    async fn complete(&self, request: TextModelRequest) -> Result<TextModelResponse> {
        let messages = request
            .messages
            .into_iter()
            .map(ChatMessage::from)
            .collect();
        let tools = (!request.tools.is_empty()).then_some(request.tools);
        let parsed = self
            .send_chat_completion(&self.request_body_for_messages(messages, tools))
            .await?;
        let usage = parsed.usage;
        let choice = parsed
            .choices
            .into_iter()
            .next()
            .context("Connector response did not include any choices")?;
        let finish_reason = choice.finish_reason;
        let assistant = choice.message;
        if assistant.tool_calls.is_empty() {
            let content = assistant.content.as_deref().unwrap_or_default().trim();
            if content.is_empty() {
                bail!(empty_content_error(
                    &self.profile,
                    finish_reason.as_deref(),
                    usage.as_ref()
                ));
            }
            ensure_text_response_complete(&self.profile, finish_reason.as_deref(), usage.as_ref())?;
        }
        Ok(TextModelResponse {
            content: assistant.content,
            tool_calls: assistant.tool_calls,
        })
    }
}

#[derive(Clone, Debug)]
pub struct OpenAiCompatibleImageProvider {
    connector: OpenAiCompatibleConnector,
    profile: ModelProfile,
}

impl OpenAiCompatibleImageProvider {
    pub fn new(
        connector_name: String,
        connector: OpenAiCompatibleConnectorConfig,
        profile: ModelProfile,
    ) -> Result<Self> {
        Ok(Self {
            connector: OpenAiCompatibleConnector::new(connector_name, connector)?,
            profile,
        })
    }

    pub fn request_body(&self, request: &ImageGenerationRequest) -> Result<ImageRequest> {
        let mut options = BTreeMap::new();
        for (key, value) in &self.profile.options {
            if matches!(key.as_str(), "model" | "prompt" | "n" | "stream") {
                bail!("Image model option '{key}' is reserved by Sfumato");
            }
            options.insert(
                key.clone(),
                serde_json::to_value(value)
                    .with_context(|| format!("Could not serialize image model option '{key}'"))?,
            );
        }
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
    ) -> Result<ImageGenerationResponse> {
        let body = self.request_body(&request)?;
        let response = self
            .connector
            .post("images")?
            .json(&body)
            .send()
            .await
            .with_context(|| {
                format!(
                    "Could not reach OpenAI-compatible connector '{}' for image generation",
                    self.connector.name
                )
            })?;
        let status = response.status();
        let text = response
            .text()
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
    ) -> Result<ChatCompletionsResponse> {
        let response = self
            .connector
            .post("chat/completions")?
            .json(body)
            .send()
            .await
            .with_context(|| {
                format!(
                    "Could not reach OpenAI-compatible connector '{}'",
                    self.connector.name
                )
            })?;
        let status = response.status();
        let text = response
            .text()
            .await
            .context("Could not read connector response body")?;
        if !status.is_success() {
            if is_context_limit_response(&text) {
                return Err(TextGenerationLimitError::context(
                    self.profile.model.clone(),
                    option_integer(&self.profile, "max_tokens", 4000) as u64,
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

fn resolve_api_key(connector: &OpenAiCompatibleConnectorConfig) -> Result<Option<String>> {
    let Some(reference) = &connector.credential else {
        return Ok(None);
    };
    match reference.scheme() {
        "env" => std::env::var(reference.target()).map(Some).map_err(|_| {
            anyhow::anyhow!(
                "Missing API key environment variable {}",
                reference.target()
            )
        }),
        scheme => bail!("Unsupported connector credential scheme '{scheme}'"),
    }
}

fn option_float(profile: &ModelProfile, key: &str, default: f32) -> f32 {
    profile
        .options
        .get(key)
        .and_then(toml::Value::as_float)
        .map(|value| value as f32)
        .unwrap_or(default)
}

fn option_integer(profile: &ModelProfile, key: &str, default: i64) -> i64 {
    profile
        .options
        .get(key)
        .and_then(toml::Value::as_integer)
        .unwrap_or(default)
}

fn empty_content_error(
    profile: &ModelProfile,
    finish_reason: Option<&str>,
    usage: Option<&CompletionUsage>,
) -> anyhow::Error {
    if matches!(finish_reason, Some("length" | "max_tokens")) {
        return TextGenerationLimitError::output(
            profile.model.clone(),
            option_integer(profile, "max_tokens", 4000) as u64,
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
        option_integer(profile, "max_tokens", 4000) as u64,
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

fn is_context_limit_response(body: &str) -> bool {
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

fn compact_error_detail(body: &str) -> String {
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
    pub stream: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
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
            content: Some(content),
            tool_calls: Vec::new(),
            tool_call_id: None,
            reasoning: None,
            reasoning_details: None,
        }
    }
}

impl From<ModelMessage> for ChatMessage {
    fn from(message: ModelMessage) -> Self {
        match message {
            ModelMessage::System(content) => Self::text("system", content),
            ModelMessage::User(content) => Self::text("user", content),
            ModelMessage::Assistant {
                content,
                tool_calls,
            } => Self {
                role: "assistant".to_string(),
                content,
                tool_calls,
                tool_call_id: None,
                reasoning: None,
                reasoning_details: None,
            },
            ModelMessage::Tool {
                tool_call_id,
                name: _,
                content,
            } => Self {
                role: "tool".to_string(),
                content: Some(content),
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
