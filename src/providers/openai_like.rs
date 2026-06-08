use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::{
    config::OpenAiLikeProviderConfig,
    providers::{GenerateTextRequest, GenerateTextResponse, LanguageModelProvider, ProviderKind},
};

#[derive(Clone, Debug)]
pub struct OpenAiLikeProvider {
    client: Client,
    config: OpenAiLikeProviderConfig,
    kind: ProviderKind,
}

impl OpenAiLikeProvider {
    pub fn new(config: OpenAiLikeProviderConfig, kind: ProviderKind) -> Result<Self> {
        let _ = kind.credential(&config)?;

        Ok(Self {
            client: Client::new(),
            config,
            kind,
        })
    }

    fn endpoint(&self) -> String {
        format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        )
    }

    pub fn request_body(&self, request: &GenerateTextRequest) -> ChatCompletionsRequest {
        ChatCompletionsRequest {
            model: request.model.clone(),
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
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            stream: false,
        }
    }
}

#[async_trait]
impl LanguageModelProvider for OpenAiLikeProvider {
    async fn generate_text(&self, request: GenerateTextRequest) -> Result<GenerateTextResponse> {
        let api_key = self.kind.credential(&self.config)?;
        let body = self.request_body(&request);

        let response = self
            .client
            .post(self.endpoint())
            .bearer_auth(api_key)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("Could not reach {} provider", self.kind.label()))?;

        let status = response.status();
        let text = response
            .text()
            .await
            .context("Could not read provider response body")?;

        if !status.is_success() {
            bail!(
                "{} provider returned HTTP {}: {}",
                self.kind.label(),
                status,
                text
            );
        }

        let parsed: ChatCompletionsResponse =
            serde_json::from_str(&text).context("Could not parse provider chat response")?;
        let content = parsed
            .choices
            .first()
            .map(|choice| choice.message.content.trim().to_string())
            .filter(|content| !content.is_empty())
            .context("Provider response did not include text content")?;

        Ok(GenerateTextResponse { text: content })
    }
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
#[path = "../../tests/unit/openai_like.rs"]
mod tests;
