mod openai_compatible;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

use crate::config::{Capability, EffectiveConfig, ModelProfile};

pub use openai_compatible::OpenAiCompatibleTextProvider;

#[derive(Clone)]
pub struct TextGenerationRequest {
    pub system_prompt: String,
    pub user_prompt: String,
    pub tools: Vec<ToolDefinition>,
    pub tool_executor: Option<Arc<dyn ToolExecutor>>,
    pub max_tool_rounds: usize,
}

impl TextGenerationRequest {
    pub fn new(system_prompt: String, user_prompt: String) -> Self {
        Self {
            system_prompt,
            user_prompt,
            tools: Vec::new(),
            tool_executor: None,
            max_tool_rounds: 4,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TextGenerationResponse {
    pub text: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub kind: String,
    pub function: ToolFunctionDefinition,
}

#[derive(Clone, Debug, Serialize)]
pub struct ToolFunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Clone, Debug)]
pub struct ToolExecutionRequest {
    pub name: String,
    pub arguments: Value,
}

pub trait ToolExecutor: Send + Sync {
    fn execute(&self, request: ToolExecutionRequest) -> Result<String>;
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ToolCall {
    pub id: Option<String>,
    #[serde(default = "function_tool_kind")]
    #[serde(rename = "type")]
    pub kind: String,
    pub function: ToolCallFunction,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: Value,
}

fn function_tool_kind() -> String {
    "function".to_string()
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
