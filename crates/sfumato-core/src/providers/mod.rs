mod openai_compatible;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

use crate::config::{Capability, EffectiveConfig, ModelProfile};

pub use openai_compatible::{OpenAiCompatibleImageProvider, OpenAiCompatibleTextProvider};

#[derive(Clone)]
pub struct TextGenerationRequest {
    pub system_prompt: String,
    pub user_prompt: String,
    pub tools: Vec<ToolDefinition>,
    pub tool_executor: Option<Arc<dyn ToolExecutor>>,
    pub event_sink: Option<Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
    pub max_tool_rounds: usize,
}

impl TextGenerationRequest {
    pub fn new(system_prompt: String, user_prompt: String) -> Self {
        Self {
            system_prompt,
            user_prompt,
            tools: Vec::new(),
            tool_executor: None,
            event_sink: None,
            max_tool_rounds: 8,
        }
    }

    pub fn emit(&self, event: TextGenerationEvent) {
        if let Some(sink) = &self.event_sink {
            sink(event);
        }
    }
}

#[derive(Clone, Debug)]
pub struct TextGenerationResponse {
    pub text: String,
}

#[derive(Clone, Debug)]
pub enum TextGenerationEvent {
    StageStarted {
        stage: GenerationStage,
        profile: Option<String>,
    },
    RequestStarted {
        round: usize,
    },
    ToolCallRequested {
        name: String,
        arguments: Value,
    },
    ToolCallSucceeded {
        name: String,
        result: String,
    },
    ToolCallFailed {
        name: String,
        error: String,
    },
    ResponseCompleted,
    DraftTitleRepairStarted {
        error: String,
    },
    ReviewRetryStarted {
        attempt: usize,
        error: String,
    },
    LayoutCheckCompleted {
        issues: usize,
    },
    LayoutSlideRepairStarted {
        slide: usize,
        position: usize,
        total: usize,
        profile: String,
    },
    LayoutSlideRepairRetryStarted {
        slide: usize,
        attempt: usize,
        error: String,
    },
}

#[derive(Clone, Copy, Debug)]
pub enum GenerationStage {
    Draft,
    SemanticReview,
    LayoutCheck,
    LayoutRepair,
    Rendering,
}

impl GenerationStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "drafting slides",
            Self::SemanticReview => "reviewing content",
            Self::LayoutCheck => "checking layout",
            Self::LayoutRepair => "repairing layout",
            Self::Rendering => "rendering artifacts",
        }
    }
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

#[async_trait]
pub trait ToolExecutor: Send + Sync {
    async fn execute(&self, request: ToolExecutionRequest) -> Result<String>;
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

#[derive(Clone, Debug)]
pub struct ImageGenerationRequest {
    pub prompt: String,
}

#[derive(Clone, Debug)]
pub struct ImageGenerationResponse {
    pub bytes: Vec<u8>,
    pub media_type: String,
}

#[async_trait]
pub trait ImageGenerationProvider: Send + Sync {
    async fn generate_image(
        &self,
        request: ImageGenerationRequest,
    ) -> Result<ImageGenerationResponse>;
}
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

pub fn build_image_provider(
    config: &EffectiveConfig,
    profile: &ModelProfile,
) -> Result<Box<dyn ImageGenerationProvider>> {
    if !profile.capabilities.contains(&Capability::Image) {
        bail!("Selected model profile does not support image generation");
    }
    let connector = config.connectors.get(&profile.connector).with_context(|| {
        format!(
            "OpenAI-compatible connector '{}' was not found",
            profile.connector
        )
    })?;
    Ok(Box::new(OpenAiCompatibleImageProvider::new(
        profile.connector.clone(),
        connector.clone(),
        profile.clone(),
    )?))
}
