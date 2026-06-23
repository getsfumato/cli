use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use reqwest::{Client, RequestBuilder};
use serde::{Deserialize, Serialize};

use crate::{
    config::{ModelProfile, OpenAiCompatibleConnectorConfig},
    providers::{
        TextGenerationProvider, TextGenerationRequest, TextGenerationResponse, ToolCall,
        ToolDefinition, ToolExecutionRequest,
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
            client: Client::new(),
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
        let mut request = self
            .client
            .post(self.endpoint(path))
            .bearer_auth(resolve_api_key(&self.config)?);
        for (name, value) in &self.config.headers {
            request = request.header(name, value);
        }
        Ok(request)
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

    pub fn request_body(&self, request: &TextGenerationRequest) -> ChatCompletionsRequest {
        self.request_body_for_messages(
            initial_messages(request),
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
impl TextGenerationProvider for OpenAiCompatibleTextProvider {
    async fn generate_text(
        &self,
        request: TextGenerationRequest,
    ) -> Result<TextGenerationResponse> {
        let tools = (!request.tools.is_empty()).then_some(request.tools.clone());
        let mut messages = initial_messages(&request);

        for _ in 0..=request.max_tool_rounds {
            let body = self.request_body_for_messages(messages.clone(), tools.clone());
            let parsed = self.send_chat_completion(&body).await?;
            let assistant_message = parsed
                .choices
                .into_iter()
                .next()
                .map(|choice| choice.message)
                .context("Connector response did not include any choices")?;

            if assistant_message.tool_calls.is_empty() {
                let content = assistant_message
                    .content
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if content.is_empty() {
                    bail!("Connector response did not include text content");
                }
                return Ok(TextGenerationResponse { text: content });
            }

            let executor = request.tool_executor.as_ref().context(
                "Connector requested tool calls, but no Sfumato tool executor is available",
            )?;
            let tool_calls = assistant_message.tool_calls.clone();
            messages.push(assistant_message);
            for tool_call in tool_calls {
                let result = executor.execute(ToolExecutionRequest {
                    name: tool_call.function.name.clone(),
                    arguments: tool_call.function.arguments.clone(),
                })?;
                messages.push(ChatMessage::tool_response(tool_call, result));
            }
        }

        bail!(
            "Connector exceeded the maximum of {} Sfumato tool rounds",
            request.max_tool_rounds
        )
    }
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

fn resolve_api_key(connector: &OpenAiCompatibleConnectorConfig) -> Result<String> {
    if let Some(api_key) = &connector.api_key {
        return Ok(api_key.clone());
    }
    if let Some(env_name) = &connector.api_key_env {
        return std::env::var(env_name)
            .map_err(|_| anyhow::anyhow!("Missing API key environment variable {env_name}"));
    }
    bail!("OpenAI-compatible connector requires api_key or api_key_env")
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
}

impl ChatMessage {
    fn text(role: &str, content: String) -> Self {
        Self {
            role: role.to_string(),
            content: Some(content),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }

    fn tool_response(tool_call: ToolCall, content: String) -> Self {
        Self {
            role: "tool".to_string(),
            content: Some(content),
            tool_calls: Vec::new(),
            tool_call_id: tool_call.id,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ChatCompletionsResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

fn initial_messages(request: &TextGenerationRequest) -> Vec<ChatMessage> {
    vec![
        ChatMessage::text("system", request.system_prompt.clone()),
        ChatMessage::text("user", request.user_prompt.clone()),
    ]
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../../tests/unit/openai_compatible.rs"]
mod tests;
