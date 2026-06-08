mod openai_like;

use anyhow::{Result, bail};
use async_trait::async_trait;

use crate::config::{OpenAiLikeProviderConfig, SfumatoConfig};

pub use openai_like::OpenAiLikeProvider;

#[derive(Clone, Debug)]
pub struct GenerateTextRequest {
    pub system_prompt: String,
    pub user_prompt: String,
    pub model: String,
    pub temperature: f32,
    pub max_tokens: u32,
}

#[derive(Clone, Debug)]
pub struct GenerateTextResponse {
    pub text: String,
}

#[async_trait]
pub trait LanguageModelProvider: Send + Sync {
    async fn generate_text(&self, request: GenerateTextRequest) -> Result<GenerateTextResponse>;
}

#[derive(Clone, Debug)]
pub enum ProviderKind {
    Ollama,
    OpenRouter,
}

impl ProviderKind {
    pub fn from_config(config: &SfumatoConfig) -> Result<Self> {
        match config.inference.provider.as_str() {
            "ollama" => Ok(Self::Ollama),
            "openrouter" => Ok(Self::OpenRouter),
            other => bail!("Unknown provider '{other}'. Use 'ollama' or 'openrouter'."),
        }
    }

    pub fn build_provider(&self, config: &SfumatoConfig) -> Result<Box<dyn LanguageModelProvider>> {
        let provider_config = match self {
            Self::Ollama => &config.providers.ollama,
            Self::OpenRouter => &config.providers.openrouter,
        };

        Ok(Box::new(OpenAiLikeProvider::new(
            provider_config.clone(),
            self.clone(),
        )?))
    }

    pub fn credential(&self, config: &OpenAiLikeProviderConfig) -> Result<String> {
        if let Some(api_key) = &config.api_key {
            return Ok(api_key.clone());
        }

        if let Some(env_name) = &config.api_key_env {
            return std::env::var(env_name)
                .map_err(|_| anyhow::anyhow!("Missing API key environment variable {env_name}"));
        }

        match self {
            Self::Ollama => Ok("ollama".to_string()),
            Self::OpenRouter => bail!("OpenRouter requires an API key or api_key_env setting"),
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Ollama => "ollama",
            Self::OpenRouter => "openrouter",
        }
    }
}
