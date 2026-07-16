use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use thiserror::Error;

use crate::config::{EffectiveConfig, ModelProfile};
use crate::prompts::PromptProvenance;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextGenerationLimitKind {
    Context,
    Output,
}

#[derive(Clone, Debug, Error)]
#[error("{message}")]
pub struct TextGenerationLimitError {
    pub kind: TextGenerationLimitKind,
    pub model: String,
    pub max_tokens: u64,
    pub finish_reason: Option<String>,
    pub completion_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    message: String,
}

impl TextGenerationLimitError {
    /// Creates a typed output truncation or empty-response error.
    pub fn output(
        model: String,
        max_tokens: u64,
        finish_reason: Option<String>,
        completion_tokens: Option<u64>,
        reasoning_tokens: Option<u64>,
        empty: bool,
    ) -> Self {
        let reason = finish_reason.as_deref().unwrap_or("unknown");
        let completion = completion_tokens
            .map(|tokens| format!(", completion tokens: {tokens}"))
            .unwrap_or_default();
        let reasoning = reasoning_tokens
            .map(|tokens| format!(", reasoning tokens: {tokens}"))
            .unwrap_or_default();
        let message = if empty {
            format!(
                "Connector response did not include text content (finish reason: {reason}{completion}{reasoning}). Increase the model profile's max_tokens option above {max_tokens}, reduce its reasoning budget, or let Sfumato retry with compacted context."
            )
        } else {
            format!(
                "Connector truncated the '{model}' text response at max_tokens={max_tokens} (finish reason: {reason}{completion}{reasoning}). Sfumato refused to use the incomplete response."
            )
        };
        Self {
            kind: TextGenerationLimitKind::Output,
            model,
            max_tokens,
            finish_reason,
            completion_tokens,
            reasoning_tokens,
            message,
        }
    }

    /// Creates a typed context-window exhaustion error.
    pub fn context(model: String, max_tokens: u64, detail: String) -> Self {
        Self {
            kind: TextGenerationLimitKind::Context,
            model: model.clone(),
            max_tokens,
            finish_reason: None,
            completion_tokens: None,
            reasoning_tokens: None,
            message: format!(
                "Connector rejected the '{model}' request because its context token limit was exceeded: {detail}"
            ),
        }
    }
}

#[derive(Clone)]
pub struct TextGenerationRequest {
    pub system_prompt: String,
    pub user_prompt: String,
    pub tools: Vec<ToolDefinition>,
    pub tool_executor: Option<Arc<dyn ToolExecutor>>,
    pub event_sink: Option<Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
    pub max_tool_rounds: usize,
    /// Request-specific user message sent when the tool round limit is reached.
    pub tool_exhausted_prompt: Option<String>,
    /// Template provenance used to construct this request.
    pub prompt_provenance: Vec<PromptProvenance>,
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
            tool_exhausted_prompt: None,
            prompt_provenance: Vec::new(),
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
    ContextCompactionStarted {
        stage: GenerationStage,
        original_chars: usize,
        compacted_chars: usize,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GenerationStage {
    Draft,
    Edit,
    SemanticReview,
    LayoutCheck,
    LayoutRepair,
    Rendering,
}

impl GenerationStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "drafting slides",
            Self::Edit => "editing slide content",
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

/// Provider-neutral conversation message used for one model turn.
#[derive(Clone, Debug)]
pub enum ModelMessage {
    /// System instruction.
    System(String),
    /// User instruction or follow-up.
    User(String),
    /// Assistant response, including any requested tools.
    Assistant {
        /// Optional assistant text.
        content: Option<String>,
        /// Tool calls requested by the assistant.
        tool_calls: Vec<ToolCall>,
    },
    /// Result of one tool call.
    Tool {
        /// Provider tool-call identifier.
        tool_call_id: Option<String>,
        /// Tool name.
        name: String,
        /// JSON or text result supplied to the model.
        content: String,
    },
}

/// Input for exactly one provider model turn.
#[derive(Clone, Debug)]
pub struct TextModelRequest {
    /// Complete conversation transcript.
    pub messages: Vec<ModelMessage>,
    /// Tools available for this turn; empty disables tool calling.
    pub tools: Vec<ToolDefinition>,
}

/// Output from exactly one provider model turn.
#[derive(Clone, Debug)]
pub struct TextModelResponse {
    /// Optional assistant text.
    pub content: Option<String>,
    /// Tool calls requested by the assistant.
    pub tool_calls: Vec<ToolCall>,
}

/// Low-level text model transport. Implementations perform one request only.
#[async_trait]
pub trait TextModel: Send + Sync {
    /// Performs one model turn without executing tools or retrying.
    async fn complete(&self, request: TextModelRequest) -> Result<TextModelResponse>;
}

/// Application-level agent loop that executes provider-neutral tools.
pub struct AgentRunner {
    model: Arc<dyn TextModel>,
}

impl AgentRunner {
    /// Creates an agent over a one-turn model transport.
    pub fn new(model: Arc<dyn TextModel>) -> Self {
        Self { model }
    }
}

#[async_trait]
impl TextGenerationProvider for AgentRunner {
    async fn generate_text(
        &self,
        request: TextGenerationRequest,
    ) -> Result<TextGenerationResponse> {
        let mut messages = vec![
            ModelMessage::System(request.system_prompt.clone()),
            ModelMessage::User(request.user_prompt.clone()),
        ];

        for round in 0..request.max_tool_rounds {
            request.emit(TextGenerationEvent::RequestStarted { round: round + 1 });
            let response = self
                .model
                .complete(TextModelRequest {
                    messages: messages.clone(),
                    tools: request.tools.clone(),
                })
                .await?;
            if response.tool_calls.is_empty() {
                return complete_agent_response(&request, response.content);
            }

            let executor = request.tool_executor.as_ref().context(
                "Connector requested tool calls, but no Sfumato tool executor is available",
            )?;
            messages.push(ModelMessage::Assistant {
                content: response.content,
                tool_calls: response.tool_calls.clone(),
            });
            for tool_call in response.tool_calls {
                request.emit(TextGenerationEvent::ToolCallRequested {
                    name: tool_call.function.name.clone(),
                    arguments: tool_call.function.arguments.clone(),
                });
                let result = match executor
                    .execute(ToolExecutionRequest {
                        name: tool_call.function.name.clone(),
                        arguments: tool_call.function.arguments.clone(),
                    })
                    .await
                {
                    Ok(result) => {
                        request.emit(TextGenerationEvent::ToolCallSucceeded {
                            name: tool_call.function.name.clone(),
                            result: result.clone(),
                        });
                        result
                    }
                    Err(error) => {
                        let error = format!("{error:#}");
                        request.emit(TextGenerationEvent::ToolCallFailed {
                            name: tool_call.function.name.clone(),
                            error: error.clone(),
                        });
                        serde_json::json!({ "error": error }).to_string()
                    }
                };
                messages.push(ModelMessage::Tool {
                    tool_call_id: tool_call.id,
                    name: tool_call.function.name,
                    content: result,
                });
            }
        }

        let exhausted = request.tool_exhausted_prompt.clone().context(
            "The model exhausted its tool rounds, but this request has no output-contract prompt",
        )?;
        messages.push(ModelMessage::User(exhausted));
        request.emit(TextGenerationEvent::RequestStarted {
            round: request.max_tool_rounds + 1,
        });
        let response = self
            .model
            .complete(TextModelRequest {
                messages,
                tools: Vec::new(),
            })
            .await?;
        if !response.tool_calls.is_empty() {
            bail!("Model requested tools after tool calling was disabled");
        }
        complete_agent_response(&request, response.content)
    }
}

fn complete_agent_response(
    request: &TextGenerationRequest,
    content: Option<String>,
) -> Result<TextGenerationResponse> {
    let text = content.unwrap_or_default().trim().to_string();
    if text.is_empty() {
        bail!("Connector response did not include text content");
    }
    request.emit(TextGenerationEvent::ResponseCompleted);
    Ok(TextGenerationResponse { text })
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
/// Port for resolving model profiles into provider implementations.
pub trait ProviderFactory: Send + Sync {
    /// Builds a text-generation provider for a resolved profile.
    fn text(
        &self,
        config: &EffectiveConfig,
        profile: &ModelProfile,
    ) -> Result<Box<dyn TextGenerationProvider>>;

    /// Builds an image-generation provider for a resolved profile.
    fn image(
        &self,
        config: &EffectiveConfig,
        profile: &ModelProfile,
    ) -> Result<Box<dyn ImageGenerationProvider>>;
}
