//! The provider ports: how Sfumato asks a model for something.
//!
//! Four capabilities, each its own trait, because a provider that generates images
//! is not obliged to generate speech. [`TextModel`] is the narrow one an adapter
//! implements; [`AgentRunner`] wraps it to add the tool loop, round limits and
//! cancellation, so those live once rather than in every provider.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};
use thiserror::Error;

use crate::config::{ConnectorConfig, EffectiveConfig, ModelProfile};
use crate::errors::{ErrorClass, OperationStage, SfumatoError, SfumatoResult};
use crate::operation::{OperationContext, OperationEventKind};
use crate::prompts::PromptProvenance;

/// Which limit a generation ran into.
///
/// Distinguished because the remedies differ and only one of them is recoverable:
/// hitting the context window can be retried with compacted input, while a
/// truncated output means the response cannot be used at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextGenerationLimitKind {
    /// The input did not fit. Retryable by compacting what was sent.
    Context,
    /// The reply was cut off at `max_tokens`, or came back empty. Not usable.
    Output,
}

#[derive(Clone, Debug, Error)]
#[error("{message}")]
/// A generation that hit a token limit, with the numbers needed to act on it.
///
/// Typed rather than a string because the recovery depends on
/// [`TextGenerationLimitKind`], and because the message has to name the profile
/// option to change — a limit error nobody can act on is just a failure.
pub struct TextGenerationLimitError {
    /// Which limit was reached.
    pub kind: TextGenerationLimitKind,
    /// Provider-side model identifier, so the message names the right model when
    /// several are configured.
    pub model: String,
    /// The limit that was reached, as configured.
    pub max_tokens: u64,
    /// What the provider said about stopping, when it said anything.
    pub finish_reason: Option<String>,
    /// Tokens the provider reported producing, when it reported them.
    pub completion_tokens: Option<u64>,
    /// Tokens spent on reasoning rather than output, when the provider separates
    /// them. A large value here with an empty response is the signal to reduce the
    /// reasoning budget rather than to raise `max_tokens`.
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

impl From<TextGenerationLimitError> for SfumatoError {
    fn from(limit: TextGenerationLimitError) -> Self {
        let class = match limit.kind {
            TextGenerationLimitKind::Context => ErrorClass::ContextLimit,
            TextGenerationLimitKind::Output => ErrorClass::InvalidOutput,
        };
        let mut error = SfumatoError::provider(class, &limit.message)
            .with_detail("model", limit.model)
            .with_detail("max_tokens", limit.max_tokens.to_string());
        if let Some(reason) = limit.finish_reason {
            error = error.with_detail("finish_reason", reason);
        }
        if let Some(tokens) = limit.completion_tokens {
            error = error.with_detail("completion_tokens", tokens.to_string());
        }
        if let Some(tokens) = limit.reasoning_tokens {
            error = error.with_detail("reasoning_tokens", tokens.to_string());
        }
        error
    }
}

/// One image handed to a text model alongside its prompt.
///
/// Names the file rather than carrying its bytes. Connectors disagree about what
/// they want: `chat/completions` and Anthropic need base64 inline, while the Codex
/// App Server takes a local path and reads the file itself. A path serves both and
/// spares the reader an encode that one of them throws away.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageAttachment {
    /// What the image shows, so a model can tell one attachment from another.
    ///
    /// Sent as text immediately before the image, because no provider carries a
    /// caption field and an unlabelled sequence of frames is unreadable.
    pub label: String,
    /// IANA media type, such as `image/png`.
    pub media_type: String,
    /// Absolute path of the image on disk.
    pub path: PathBuf,
}

/// One text generation, tools and all.
///
/// Built by a workflow and handed to [`AgentRunner`], which is what turns it into
/// however many provider calls the tool loop needs.
#[derive(Clone)]
pub struct TextGenerationRequest {
    /// The system prompt, rendered from a template.
    pub system_prompt: String,
    /// The first user turn.
    pub user_prompt: String,
    /// Tools the model may call. Empty means it must answer from the prompt alone.
    pub tools: Vec<ToolDefinition>,
    /// Runs the tools. Required if `tools` is non-empty; without it a tool call has
    /// nowhere to go.
    pub tool_executor: Option<Arc<dyn ToolExecutor>>,
    /// Where progress goes. `None` discards it, which is what a dry run wants.
    pub event_sink: Option<Arc<dyn Fn(TextGenerationEvent) + Send + Sync>>,
    /// How many times the model may call tools before being asked to answer.
    ///
    /// A bound, not a target: a model that keeps reaching for tools would otherwise
    /// spend a budget without producing anything.
    pub max_tool_rounds: usize,
    /// Request-specific user message sent when the tool round limit is reached.
    pub tool_exhausted_prompt: Option<String>,
    /// Template provenance used to construct this request.
    pub prompt_provenance: Vec<PromptProvenance>,
    /// Images the model must look at to answer, attached to the first user turn.
    ///
    /// Empty for every text-only request, which keeps connectors that cannot
    /// accept images working exactly as before; a connector that cannot must
    /// reject a non-empty list rather than answer about images it never saw.
    pub images: Vec<ImageAttachment>,
}

impl TextGenerationRequest {
    /// A request with no tools, no event sink and the default round limit.
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
            images: Vec::new(),
        }
    }

    /// Sends an event to the sink, or drops it when there is none.
    pub fn emit(&self, event: TextGenerationEvent) {
        if let Some(sink) = &self.event_sink {
            sink(event);
        }
    }
}

/// What a text generation produced.
#[derive(Clone, Debug)]
pub struct TextGenerationResponse {
    /// The model's answer, with any tool rounds already resolved.
    pub text: String,
}

/// Progress from inside a generation.
///
/// The vocabulary a frontend renders: a run is minutes of model calls, tool calls
/// and repairs, and a spinner would say nothing about which. Sanitized — no prompt
/// text, no credentials — because these reach a terminal and a JSON stream.
#[derive(Clone, Debug)]
pub enum TextGenerationEvent {
    /// A named stage of the workflow began.
    StageStarted {
        /// Which stage.
        stage: GenerationStage,
        /// The model profile serving it, when one was resolved.
        profile: Option<String>,
    },
    /// A provider call began. `round` counts tool rounds, starting at one.
    RequestStarted {
        /// Which round of the tool loop this is.
        round: usize,
    },
    /// Codex App Server resolved and selected one authenticated model.
    /// Codex App Server resolved and selected one authenticated model.
    ModelSelected {
        /// Provider-side identifier.
        model: String,
        /// The name to show a person.
        display_name: String,
    },
    /// The model asked for a tool.
    ToolCallRequested {
        /// Tool name, as declared to the model.
        name: String,
        /// Arguments the model supplied.
        arguments: Value,
    },
    /// A tool ran and returned something.
    ToolCallSucceeded {
        /// Tool name.
        name: String,
        /// A summary of what came back, not necessarily the whole payload.
        result: String,
    },
    /// A tool refused or failed. The model is told, and may try something else.
    ToolCallFailed {
        /// Tool name.
        name: String,
        /// Why it failed, as the model was told.
        error: String,
    },
    /// The provider finished answering.
    ResponseCompleted,
    /// The draft came back without a usable title and is being asked again.
    DraftTitleRepairStarted {
        /// What was wrong with it.
        error: String,
    },
    /// A review produced something unusable and is being retried.
    ReviewRetryStarted {
        /// Which attempt this is.
        attempt: usize,
        /// Why the previous one was rejected.
        error: String,
    },
    /// The input did not fit, and is being re-sent smaller.
    ///
    /// The two sizes are the useful part: they say whether compaction found
    /// anything to remove, which is what distinguishes a recoverable run from one
    /// that will fail again.
    ContextCompactionStarted {
        /// The stage being retried.
        stage: GenerationStage,
        /// Characters sent the first time.
        original_chars: usize,
        /// Characters after compaction.
        compacted_chars: usize,
    },
    /// A rendered deck was measured in a browser.
    LayoutCheckCompleted {
        /// How many slides overflowed. Zero means nothing needs repairing.
        issues: usize,
    },
    /// One overflowing slide is being re-authored.
    LayoutSlideRepairStarted {
        /// The slide number in the deck.
        slide: usize,
        /// Position within this repair pass.
        position: usize,
        /// How many slides this pass will repair.
        total: usize,
        /// The profile doing the repair.
        profile: String,
    },
    /// A slide repair produced something unusable and is being retried.
    LayoutSlideRepairRetryStarted {
        /// The slide number.
        slide: usize,
        /// Which attempt this is.
        attempt: usize,
        /// Why the previous one was rejected.
        error: String,
    },
    /// A generated source failed validation and is about to be repaired.
    ///
    /// Carries the reason because a repair that reports only that it happened
    /// leaves nothing to diagnose: the validation message is the whole signal, and
    /// it is otherwise visible only inside the repair prompt.
    SourceRepairStarted {
        /// What the validator or renderer objected to.
        reason: String,
        /// The scene being re-authored, when the failure named one.
        scene: Option<String>,
    },
}

/// Which part of a workflow an event came from.
///
/// Per-resource rather than generic, because "reviewing" means something different
/// for a deck than for a film and a reader watching a run wants the specific one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GenerationStage {
    /// Writing the first version.
    Draft,
    /// Applying a focused instruction to something that already exists.
    Edit,
    /// Re-authoring output that failed structural validation.
    ValidationRepair,
    /// Reviewing content against the instruction and the sources.
    SemanticReview,
    /// Re-authoring a diagram the renderer rejected.
    DiagramRepair,
    /// Measuring a rendered deck in a browser.
    LayoutCheck,
    /// Re-authoring slides that overflowed.
    LayoutRepair,
    /// Producing the final artifact.
    Rendering,
    /// Writing a page's fragment, CSS and script.
    PageDraft,
    /// Reviewing a page.
    PageReview,
    /// Re-authoring a page the browser or the validator rejected.
    PageRepair,
    /// Assembling and inspecting the standalone page.
    PageRendering,
    /// Turning the instruction into a scene plan.
    VideoPlanning,
    /// Reviewing that plan before anything is authored.
    VideoReview,
    /// Writing the scene sources the engine renders.
    VideoAuthoring,
    /// Re-authoring a scene that failed to render or validate.
    VideoRepair,
    /// An image-capable model inspecting rendered frames.
    VideoVisualReview,
    /// Handing the authored sources to the engine.
    VideoRendering,
    /// Speaking the planned narration and timing it against the storyboard.
    VideoNarration,
    /// Writing a document's sections.
    DocumentDraft,
    /// Re-authoring document content that failed structural validation.
    DocumentValidationRepair,
    /// Re-authoring a document diagram the renderer rejected.
    DocumentDiagramRepair,
    /// Reviewing a document.
    DocumentReview,
    /// Measuring the paginated result, which is where prose reflow shows up.
    DocumentFormatCheck,
    /// Re-authoring content that paginated badly.
    DocumentFormatRepair,
    /// Producing the PDF.
    DocumentRendering,
}

impl GenerationStage {
    /// The stable identifier used in events and JSON output.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "drafting slides",
            Self::Edit => "editing slide content",
            Self::ValidationRepair => "repairing slide structure",
            Self::SemanticReview => "reviewing content",
            Self::DiagramRepair => "repairing diagrams",
            Self::LayoutCheck => "checking layout",
            Self::LayoutRepair => "repairing layout",
            Self::Rendering => "rendering artifacts",
            Self::PageDraft => "drafting page",
            Self::PageReview => "reviewing page content",
            Self::PageRepair => "repairing page",
            Self::PageRendering => "assembling page artifacts",
            Self::VideoPlanning => "planning video",
            Self::VideoReview => "reviewing video plan",
            Self::VideoAuthoring => "authoring video source",
            Self::VideoRepair => "repairing video source",
            Self::VideoVisualReview => "reviewing rendered frames",
            Self::VideoRendering => "rendering video",
            Self::VideoNarration => "synthesizing narration",
            Self::DocumentDraft => "drafting document",
            Self::DocumentValidationRepair => "repairing document structure",
            Self::DocumentDiagramRepair => "repairing document diagrams",
            Self::DocumentReview => "reviewing document content",
            Self::DocumentFormatCheck => "checking page format",
            Self::DocumentFormatRepair => "repairing page format",
            Self::DocumentRendering => "rendering document PDF",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
/// One tool, as declared to a model.
pub struct ToolDefinition {
    /// Always `function`; the field exists because the wire format has it.
    #[serde(rename = "type")]
    pub kind: String,
    /// What the model is told it can call.
    pub function: ToolFunctionDefinition,
}

#[derive(Clone, Debug, Serialize)]
/// The name, purpose and argument schema a model needs to call a tool.
pub struct ToolFunctionDefinition {
    /// Name the model calls.
    pub name: String,
    /// What it does, in prose. This is what the model actually reasons about, so it
    /// carries the usage guidance and not only the summary.
    pub description: String,
    /// JSON Schema for the arguments.
    pub parameters: Value,
}

#[derive(Clone, Debug)]
/// A model's request to run one tool.
pub struct ToolExecutionRequest {
    /// Which tool.
    pub name: String,
    /// What the model passed. Unvalidated: the executor is what checks it.
    pub arguments: Value,
}

/// Runs the tools a model asks for.
///
/// Returns a `String` because the result goes back to the model as a message.
/// A refusal is a returned error rather than a failed generation: the model is told
/// why and can try something else, which is how a path outside the project's
/// readable roots gets declined without ending the run.
#[async_trait]
pub trait ToolExecutor: Send + Sync {
    /// Runs one tool call and returns what the model should be told.
    async fn execute(
        &self,
        request: ToolExecutionRequest,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<String>;
}

#[derive(Clone, Debug, Deserialize, Serialize)]
/// A tool call as a provider reported it.
pub struct ToolCall {
    /// Provider-assigned identifier, echoed back with the result. Absent on
    /// providers that do not use one.
    pub id: Option<String>,
    /// Always `function`; defaulted because not every provider sends it.
    #[serde(default = "function_tool_kind")]
    #[serde(rename = "type")]
    pub kind: String,
    /// Which tool, and with what.
    pub function: ToolCallFunction,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
/// The tool named in a call, and its arguments.
pub struct ToolCallFunction {
    /// Tool name.
    pub name: String,
    /// Arguments as the model produced them.
    pub arguments: Value,
}

fn function_tool_kind() -> String {
    "function".to_string()
}

/// A text generation with the tool loop already handled.
///
/// What the workflows depend on. [`AgentRunner`] is the implementation; a provider
/// adapter implements the narrower [`TextModel`] instead and gets the loop, the
/// round limit and cancellation from it.
#[async_trait]
pub trait TextGenerationProvider: Send + Sync {
    /// Runs a request to completion, resolving tool calls along the way.
    async fn generate_text(
        &self,
        request: TextGenerationRequest,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<TextGenerationResponse>;
}

/// Provider-neutral conversation message used for one model turn.
#[derive(Clone, Debug)]
pub enum ModelMessage {
    /// System instruction.
    System(String),
    /// User instruction or follow-up.
    User(String),
    /// User instruction the model must read alongside images.
    ///
    /// Distinct from `User` so a connector without image support fails loudly on
    /// exactly the turn it cannot represent, instead of silently dropping the
    /// evidence and answering from the text alone.
    UserWithImages {
        /// The instruction itself.
        content: String,
        /// Images the model must look at, in the order they are presented.
        images: Vec<ImageAttachment>,
    },
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
        /// Whether `content` describes a tool failure rather than a result.
        ///
        /// Providers with a native error flag on tool results set it from this,
        /// so the model is not told a failed call succeeded.
        failed: bool,
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
    async fn complete(
        &self,
        request: TextModelRequest,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<TextModelResponse>;
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
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<TextGenerationResponse> {
        operation.checkpoint(stage)?;
        let mut messages = vec![
            ModelMessage::System(request.system_prompt.clone()),
            if request.images.is_empty() {
                ModelMessage::User(request.user_prompt.clone())
            } else {
                ModelMessage::UserWithImages {
                    content: request.user_prompt.clone(),
                    images: request.images.clone(),
                }
            },
        ];

        for round in 0..request.max_tool_rounds {
            operation.checkpoint(stage)?;
            operation.emit(
                stage,
                OperationEventKind::Progress,
                BTreeMap::from([
                    ("activity".to_string(), "model_request".to_string()),
                    ("round".to_string(), (round + 1).to_string()),
                ]),
            );
            request.emit(TextGenerationEvent::RequestStarted { round: round + 1 });
            let response = self
                .model
                .complete(
                    TextModelRequest {
                        messages: messages.clone(),
                        tools: request.tools.clone(),
                    },
                    operation,
                    stage,
                )
                .await?;
            if response.tool_calls.is_empty() {
                return complete_agent_response(&request, response.content);
            }

            let executor = request.tool_executor.as_ref().ok_or_else(|| {
                SfumatoError::tool(
                    ErrorClass::Permanent,
                    "Connector requested tool calls, but no Sfumato tool executor is available",
                )
                .at_stage(stage)
            })?;
            messages.push(ModelMessage::Assistant {
                content: response.content,
                tool_calls: response.tool_calls.clone(),
            });
            for tool_call in response.tool_calls {
                operation.checkpoint(stage)?;
                operation.emit(
                    stage,
                    OperationEventKind::Progress,
                    BTreeMap::from([
                        ("activity".to_string(), "tool_call".to_string()),
                        ("tool".to_string(), tool_call.function.name.clone()),
                    ]),
                );
                request.emit(TextGenerationEvent::ToolCallRequested {
                    name: tool_call.function.name.clone(),
                    arguments: tool_call.function.arguments.clone(),
                });
                let (result, failed) = match executor
                    .execute(
                        ToolExecutionRequest {
                            name: tool_call.function.name.clone(),
                            arguments: tool_call.function.arguments.clone(),
                        },
                        operation,
                        stage,
                    )
                    .await
                {
                    Ok(result) => {
                        request.emit(TextGenerationEvent::ToolCallSucceeded {
                            name: tool_call.function.name.clone(),
                            result: result.clone(),
                        });
                        (result, false)
                    }
                    Err(error) => {
                        if error.class == ErrorClass::Cancelled {
                            return Err(error);
                        }
                        let error = error.to_string();
                        request.emit(TextGenerationEvent::ToolCallFailed {
                            name: tool_call.function.name.clone(),
                            error: error.clone(),
                        });
                        (serde_json::json!({ "error": error }).to_string(), true)
                    }
                };
                messages.push(ModelMessage::Tool {
                    tool_call_id: tool_call.id,
                    name: tool_call.function.name,
                    content: result,
                    failed,
                });
            }
        }

        let exhausted = request.tool_exhausted_prompt.clone().ok_or_else(|| {
            SfumatoError::internal(
                "The model exhausted its tool rounds, but this request has no output-contract prompt",
            )
            .at_stage(stage)
        })?;
        messages.push(ModelMessage::User(exhausted));
        request.emit(TextGenerationEvent::RequestStarted {
            round: request.max_tool_rounds + 1,
        });
        operation.checkpoint(stage)?;
        let response = self
            .model
            .complete(
                TextModelRequest {
                    messages,
                    tools: Vec::new(),
                },
                operation,
                stage,
            )
            .await?;
        if !response.tool_calls.is_empty() {
            return Err(SfumatoError::provider(
                ErrorClass::InvalidOutput,
                "Model requested tools after tool calling was disabled",
            )
            .at_stage(stage));
        }
        complete_agent_response(&request, response.content)
    }
}

fn complete_agent_response(
    request: &TextGenerationRequest,
    content: Option<String>,
) -> SfumatoResult<TextGenerationResponse> {
    let text = content.unwrap_or_default().trim().to_string();
    if text.is_empty() {
        return Err(SfumatoError::provider(
            ErrorClass::InvalidOutput,
            "Connector response did not include text content",
        ));
    }
    request.emit(TextGenerationEvent::ResponseCompleted);
    Ok(TextGenerationResponse { text })
}

/// One image to generate.
#[derive(Clone, Debug)]
pub struct ImageGenerationRequest {
    /// What to draw. Written by the drafting model, not by a person.
    pub prompt: String,
}

/// A generated image.
#[derive(Clone, Debug)]
pub struct ImageGenerationResponse {
    /// The encoded image. Bytes rather than a path: nothing has decided where this
    /// belongs yet, and a rejected image should not have to be cleaned up.
    pub bytes: Vec<u8>,
    /// IANA media type, which decides the file extension when it is stored.
    pub media_type: String,
}

/// Provider-neutral request for one directly generated video clip.
#[derive(Clone, Debug)]
pub struct VideoGenerationRequest {
    /// Reviewed provider prompt.
    pub prompt: String,
    /// Exact requested duration in seconds.
    pub duration_seconds: u32,
    /// Requested resolution such as `720p`.
    pub resolution: String,
    /// Requested aspect ratio such as `16:9`.
    pub aspect_ratio: String,
    /// Native audio preference; `None` preserves provider defaults.
    pub generate_audio: Option<bool>,
    /// Optional deterministic seed.
    pub seed: Option<i64>,
    /// Local reusable artifacts sent as provider references.
    pub references: Vec<PathBuf>,
}

/// Bytes and provider metadata returned for one generated video clip.
#[derive(Clone, Debug)]
pub struct VideoGenerationResponse {
    /// Playable video bytes.
    pub bytes: Vec<u8>,
    /// Response media type, normally `video/mp4`.
    pub media_type: String,
    /// Asynchronous provider job identifier.
    pub provider_job_id: Option<String>,
    /// Provider-reported cost when available.
    pub cost: Option<f64>,
}

/// Provider-neutral request for one spoken passage.
#[derive(Clone, Debug, Default)]
pub struct SpeechGenerationRequest {
    /// Exactly the words to speak.
    pub text: String,
    /// Provider voice identifier; `None` uses the profile's configured voice.
    pub voice: Option<String>,
    /// Text spoken immediately before this passage, for prosodic continuity.
    ///
    /// Carried because a synthesiser given one sentence at a time restarts its
    /// intonation on every scene, which is what makes stitched narration sound
    /// like a list rather than a person.
    pub previous_text: Option<String>,
    /// Text spoken immediately after this passage, for the same reason.
    pub next_text: Option<String>,
}

/// One spoken word located on the synthesized audio's own timeline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpeechWordTiming {
    /// The word as it was spoken, without surrounding whitespace.
    pub text: String,
    /// Seconds from the start of this passage's audio.
    pub start_seconds: f32,
    /// Seconds from the start of this passage's audio.
    pub end_seconds: f32,
}

/// Audio bytes and word timings returned for one spoken passage.
#[derive(Clone, Debug)]
pub struct SpeechGenerationResponse {
    /// Playable audio bytes.
    pub bytes: Vec<u8>,
    /// Response media type, normally `audio/mpeg`.
    pub media_type: String,
    /// Spoken length in seconds, as reported by the provider's own alignment.
    ///
    /// Taken from the provider rather than measured from the container because a
    /// caption has to line up with the words, and only the provider knows where
    /// they fall. A provider that reports no alignment reports no duration, and
    /// the caller decides what to do about it.
    pub duration_seconds: Option<f32>,
    /// Word-level timings, empty when the provider returned no alignment.
    pub words: Vec<SpeechWordTiming>,
}

/// Port for one synchronous provider-native speech synthesis.
#[async_trait]
pub trait SpeechGenerationProvider: Send + Sync {
    /// Speaks one passage and returns its audio with word timings.
    async fn generate_speech(
        &self,
        request: SpeechGenerationRequest,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<SpeechGenerationResponse>;
}

/// Model metadata discovered from a connector's authenticated catalog.
#[derive(Clone, Debug, Serialize)]
pub struct ConnectorModelSummary {
    /// Stable provider model identifier accepted by model profiles.
    pub id: String,
    /// Human-readable model name.
    pub display_name: String,
    /// Whether the connector recommends this model by default.
    pub is_default: bool,
    /// Whether the model is hidden from normal selectors.
    pub hidden: bool,
    /// Supported provider input modalities.
    pub input_modalities: Vec<String>,
    /// Provider output modalities, when advertised.
    pub output_modalities: Vec<String>,
    /// Maximum context window in tokens, when advertised.
    pub context_length: Option<u64>,
    /// Concise provider description.
    pub description: Option<String>,
    /// Provider-native pricing or local model details safe for presentation.
    pub metadata: BTreeMap<String, String>,
}

/// Optional native operation exposed by a connector adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorCapability {
    /// Discover models and their provider metadata.
    ModelCatalog,
    /// Read authenticated account identity or plan information.
    Account,
    /// Read usage, credits, or rate-limit state.
    Usage,
    /// Inspect richer details for locally installed models.
    ModelDetails,
    /// Manage locally installed models through provider-native operations.
    ModelManagement,
    /// Inspect a local runtime and currently loaded models.
    RuntimeStatus,
    /// Generate videos through a provider-native asynchronous API.
    VideoGeneration,
    /// Synthesize speech through a provider-native API.
    SpeechGeneration,
    /// Discover the voices this account may speak with.
    VoiceCatalog,
}

impl ConnectorCapability {
    /// Stable presentation and automation identifier.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ModelCatalog => "model_catalog",
            Self::Account => "account",
            Self::Usage => "usage",
            Self::ModelDetails => "model_details",
            Self::ModelManagement => "model_management",
            Self::RuntimeStatus => "runtime_status",
            Self::VideoGeneration => "video_generation",
            Self::SpeechGeneration => "speech_generation",
            Self::VoiceCatalog => "voice_catalog",
        }
    }
}

/// Connector-native feature set presented by CLI, TUI, and future frontends.
#[derive(Clone, Debug, Serialize)]
pub struct ConnectorCapabilities {
    /// Connector kind owning these features.
    pub kind: String,
    /// Supported optional operations.
    pub features: Vec<ConnectorCapability>,
}

/// One labeled account, usage, credit, or rate-limit value.
#[derive(Clone, Debug, Serialize)]
pub struct ConnectorStatusField {
    /// Stable field name.
    pub name: String,
    /// Human-readable value with no secret material.
    pub value: String,
}

/// Provider-native connector status suitable for presentation frontends.
#[derive(Clone, Debug, Serialize)]
pub struct ConnectorStatus {
    /// Configured connector name.
    pub connector: String,
    /// Provider connector kind.
    pub kind: String,
    /// Provider-specific but typed key/value rows.
    pub fields: Vec<ConnectorStatusField>,
}

/// Port for connector-native discovery and account operations.
#[async_trait]
pub trait ConnectorIntrospection: Send + Sync {
    /// Reports optional native features without performing I/O.
    fn capabilities(&self, connector: &ConnectorConfig) -> ConnectorCapabilities;

    /// Lists models available through the connector's current authentication.
    async fn list_models(
        &self,
        connector_name: &str,
        connector: &ConnectorConfig,
        operation: &OperationContext,
    ) -> SfumatoResult<Vec<ConnectorModelSummary>>;

    /// Reads provider-native account, credit, usage, or runtime status.
    async fn status(
        &self,
        connector_name: &str,
        connector: &ConnectorConfig,
        operation: &OperationContext,
    ) -> SfumatoResult<ConnectorStatus>;
}

/// Generates images.
#[async_trait]
pub trait ImageGenerationProvider: Send + Sync {
    /// Generates one image.
    async fn generate_image(
        &self,
        request: ImageGenerationRequest,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<ImageGenerationResponse>;
}

/// Port for one asynchronous provider-native video generation.
#[async_trait]
pub trait VideoGenerationProvider: Send + Sync {
    /// Validates model capabilities, generates one clip, and returns its bytes.
    async fn generate_video(
        &self,
        request: VideoGenerationRequest,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<VideoGenerationResponse>;
}
/// Port for resolving model profiles into provider implementations.
pub trait ProviderFactory: Send + Sync {
    /// Builds a text-generation provider for a resolved profile.
    fn text(
        &self,
        config: &EffectiveConfig,
        profile: &ModelProfile,
    ) -> SfumatoResult<Box<dyn TextGenerationProvider>>;

    /// Builds an image-generation provider for a resolved profile.
    fn image(
        &self,
        config: &EffectiveConfig,
        profile: &ModelProfile,
    ) -> SfumatoResult<Box<dyn ImageGenerationProvider>>;

    /// Builds a provider-native video generator for a resolved profile.
    fn video(
        &self,
        config: &EffectiveConfig,
        profile: &ModelProfile,
    ) -> SfumatoResult<Box<dyn VideoGenerationProvider>>;

    /// Builds a provider-native speech synthesizer for a resolved profile.
    fn speech(
        &self,
        config: &EffectiveConfig,
        profile: &ModelProfile,
    ) -> SfumatoResult<Box<dyn SpeechGenerationProvider>>;
}
