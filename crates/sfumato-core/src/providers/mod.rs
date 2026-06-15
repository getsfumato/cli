mod openai_compatible;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;

use crate::config::{Capability, EffectiveConfig, ModelProfile};

pub use openai_compatible::OpenAiCompatibleTextProvider;

#[derive(Clone, Debug)]
pub struct TextGenerationRequest {
    pub system_prompt: String,
    pub user_prompt: String,
}

#[derive(Clone, Debug)]
pub struct TextGenerationResponse {
    pub text: String,
}

#[async_trait]
pub trait TextGenerationProvider: Send + Sync {
    async fn generate_text(&self, request: TextGenerationRequest)
    -> Result<TextGenerationResponse>;
}

#[allow(dead_code)]
pub trait ImageGenerationProvider: Send + Sync {}
#[allow(dead_code)]
pub trait VideoGenerationProvider: Send + Sync {}
#[allow(dead_code)]
pub trait SpeechSynthesisProvider: Send + Sync {}

pub fn build_text_provider(
    config: &EffectiveConfig,
    profile: &ModelProfile,
) -> Result<Box<dyn TextGenerationProvider>> {
    if !profile.capabilities.contains(&Capability::Text) {
        bail!("Selected model profile does not support text generation");
    }
    let connector = config.connectors.get(&profile.connector).with_context(|| {
        format!(
            "OpenAI-compatible connector '{}' was not found",
            profile.connector
        )
    })?;
    Ok(Box::new(OpenAiCompatibleTextProvider::new(
        profile.connector.clone(),
        connector.clone(),
        profile.clone(),
    )?))
}
