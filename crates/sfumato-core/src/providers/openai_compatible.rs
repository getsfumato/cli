use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use reqwest::{Client, RequestBuilder};
use serde::{Deserialize, Serialize};

use crate::{
    config::{ModelProfile, OpenAiCompatibleConnectorConfig},
    providers::{TextGenerationProvider, TextGenerationRequest, TextGenerationResponse},
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
        ChatCompletionsRequest {
            model: self.profile.model.clone(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: request.system_prompt.clone(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: request.user_prompt.clone(),
                },
            ],
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
        let response = self
            .connector
            .post("chat/completions")?
            .json(&self.request_body(&request))
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
        let parsed: ChatCompletionsResponse =
            serde_json::from_str(&text).context("Could not parse chat completions response")?;
        let content = parsed
            .choices
            .first()
            .map(|choice| choice.message.content.trim().to_string())
            .filter(|content| !content.is_empty())
            .context("Connector response did not include text content")?;
        Ok(TextGenerationResponse { text: content })
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
    pub temperature: f32,
    pub max_tokens: u32,
    pub stream: bool,
}

#[derive(Debug, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionsResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ChatResponseMessage {
    content: String,
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../../tests/unit/openai_compatible.rs"]
mod tests;
