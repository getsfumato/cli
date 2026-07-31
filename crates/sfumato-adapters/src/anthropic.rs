//! Anthropic-native Messages API adapter.
//!
//! This is the second HTTP request format in the adapters layer: the Messages API
//! is not `chat/completions`, so it owns its wire types and authenticates with
//! `x-api-key` instead of bearer auth. It still implements only
//! [`sfumato_core::providers::TextModel`] and is wrapped in
//! `AgentRunner` by the provider factory, so the tool loop, cancellation, and
//! round limits stay provider-neutral.
//!
//! This adapter spawns no child process, so the process-reaping case in
//! `docs/reference/testing.md` does not apply; the cancellation checkpoints do.

use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use reqwest::{
    Client, RequestBuilder, StatusCode,
    header::{HeaderMap, HeaderName, HeaderValue},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sfumato_core::{
    config::{AnthropicConnectorConfig, ModelProfile},
    errors::{ErrorClass, OperationStage, SfumatoError, SfumatoResult},
    operation::{OperationContext, OperationEventKind},
    providers::{
        ConnectorModelSummary, ConnectorStatus, ConnectorStatusField, TextGenerationLimitError,
        TextModel, TextModelRequest, TextModelResponse, ToolCall, ToolCallFunction,
    },
    secrets::SecretResolver,
};

use crate::{
    openai_compatible::{compact_error_detail, is_context_limit_response},
    runtime::await_operation,
};

/// A single Claude turn at default effort routinely runs minutes, so the shared
/// 300s budget would surface as a spurious transport failure.
const HTTP_REQUEST_TIMEOUT: Duration = Duration::from_secs(600);
/// Catalog and status reads answer in well under a second and the CLI runs them
/// with a detached context that has no deadline, so they must not inherit the
/// generation timeout: it would be the only bound on a hung introspection call.
const INTROSPECTION_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const ANTHROPIC_VERSION_HEADER: &str = "anthropic-version";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const API_KEY_HEADER: &str = "x-api-key";
const DEFAULT_MODEL: &str = "claude-opus-5";
/// Documented non-streaming default. The 128K ceiling needs streaming, which this
/// workspace does not implement anywhere.
const DEFAULT_MAX_TOKENS: u32 = 16_000;
const MAX_NON_STREAMING_TOKENS: u32 = 32_000;
const MIN_MAX_TOKENS: u32 = 1_024;
const MODEL_PAGE_LIMIT: u32 = 100;
const MODEL_PAGE_BUDGET: usize = 20;
/// Bounds the verbatim-replay memo; one generation stage runs far fewer rounds.
const TURN_MEMO_LIMIT: usize = 32;

/// Native Anthropic transport applying `x-api-key` and a pinned API version.
#[derive(Clone)]
pub struct AnthropicConnector {
    client: Client,
    /// Separate client so the long generation timeout never becomes the only
    /// bound on a catalog or status read.
    introspect_client: Client,
    config: AnthropicConnectorConfig,
    secrets: Arc<dyn SecretResolver>,
}

impl AnthropicConnector {
    /// Creates the native Messages API transport.
    pub fn new(
        config: AnthropicConnectorConfig,
        secrets: Arc<dyn SecretResolver>,
    ) -> SfumatoResult<Self> {
        let build = |timeout: Duration| {
            Client::builder().timeout(timeout).build().map_err(|error| {
                SfumatoError::config(format_args!(
                    "Could not build the Anthropic HTTP client: {error}"
                ))
            })
        };
        Ok(Self {
            client: build(HTTP_REQUEST_TIMEOUT)?,
            introspect_client: build(INTROSPECTION_REQUEST_TIMEOUT)?,
            config,
            secrets,
        })
    }

    pub(crate) fn endpoint(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.config.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    /// Builds an authorized POST. Credentials resolve per request, never cached.
    pub(crate) async fn post(
        &self,
        path: &str,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> Result<RequestBuilder> {
        let request = self.client.post(self.endpoint(path));
        self.authorize(request, operation, stage).await
    }

    /// Builds an authorized GET. Credentials resolve per request, never cached.
    pub(crate) async fn get(
        &self,
        path: &str,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> Result<RequestBuilder> {
        let request = self.introspect_client.get(self.endpoint(path));
        self.authorize(request, operation, stage).await
    }

    async fn authorize(
        &self,
        request: RequestBuilder,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> Result<RequestBuilder> {
        // Assembled in a HeaderMap rather than through `RequestBuilder::header`,
        // which appends: a configured `anthropic-version` has to replace the
        // pinned one, not add a second value of the same header.
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static(ANTHROPIC_VERSION_HEADER),
            HeaderValue::from_static(ANTHROPIC_VERSION),
        );
        if let Some(reference) = &self.config.credential {
            let api_key =
                await_operation(operation, stage, self.secrets.resolve(reference)).await?;
            let mut value = HeaderValue::from_str(api_key.expose())
                .context("Anthropic credential is not a valid HTTP header value")?;
            value.set_sensitive(true);
            headers.insert(HeaderName::from_static(API_KEY_HEADER), value);
        }
        for (name, value) in &self.config.headers {
            let name = HeaderName::try_from(name.as_str())
                .with_context(|| format!("Configured header name '{name}' is invalid"))?;
            let value = HeaderValue::from_str(value)
                .with_context(|| format!("Configured value for header '{name}' is invalid"))?;
            headers.insert(name, value);
        }
        // `RequestBuilder::headers` replaces same-named values, so the loop above
        // is an override rather than an append.
        Ok(request.headers(headers))
    }

    /// Builds a one-turn text model over this transport.
    pub fn text_model(&self, profile: ModelProfile) -> AnthropicTextModel {
        AnthropicTextModel {
            connector: self.clone(),
            profile,
            turns: Arc::new(Mutex::new(TurnMemo::default())),
            warned: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Lists models available to the configured credential.
    pub async fn list_models(
        &self,
        operation: &OperationContext,
    ) -> SfumatoResult<Vec<ConnectorModelSummary>> {
        let result: Result<Vec<ConnectorModelSummary>> = async {
            let mut summaries = Vec::new();
            let mut cursor: Option<String> = None;
            for _ in 0..MODEL_PAGE_BUDGET {
                operation.checkpoint(OperationStage::Resolve)?;
                let path = match &cursor {
                    Some(after) => {
                        format!("models?limit={MODEL_PAGE_LIMIT}&after_id={after}")
                    }
                    None => format!("models?limit={MODEL_PAGE_LIMIT}"),
                };
                let page: ModelsResponse = self.get_json(&path, operation).await?;
                summaries.extend(page.data.iter().cloned().map(map_model));
                match next_page_cursor(&page) {
                    Some(next) => cursor = Some(next),
                    None => break,
                }
            }
            Ok(summaries)
        }
        .await;
        native_result(result)
    }

    /// Reports credential reachability and the default model's limits.
    pub async fn status(
        &self,
        name: &str,
        operation: &OperationContext,
    ) -> SfumatoResult<ConnectorStatus> {
        let result: Result<ConnectorStatus> = async {
            let page: ModelsResponse = self.get_json("models?limit=1000", operation).await?;
            let default = page.data.iter().find(|model| model.id == DEFAULT_MODEL);
            let mut fields = vec![
                field("api_version", ANTHROPIC_VERSION.to_string()),
                field("base_url", self.config.base_url.clone()),
                field(
                    "credential",
                    self.config
                        .credential
                        .as_ref()
                        .map(|reference| reference.scheme().to_string())
                        .unwrap_or_else(|| "none".to_string()),
                ),
                field("authenticated", "yes".to_string()),
                field("models_available", page.data.len().to_string()),
            ];
            if let Some(model) = default {
                fields.push(field("default_model", model.id.clone()));
                if let Some(context) = model.max_input_tokens {
                    fields.push(field("default_model_context", context.to_string()));
                }
                if let Some(output) = model.max_tokens {
                    fields.push(field("default_model_max_output", output.to_string()));
                }
            }
            Ok(ConnectorStatus {
                connector: name.into(),
                kind: "anthropic".into(),
                fields,
            })
        }
        .await;
        native_result(result)
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        operation: &OperationContext,
    ) -> Result<T> {
        let request = self.get(path, operation, OperationStage::Resolve).await?;
        let response = await_operation(operation, OperationStage::Resolve, request.send())
            .await
            // Classified as retryable by `provider_error`/`native_result`: a
            // connect or DNS failure is transient, not a permanent rejection.
            .with_context(|| format!("Could not reach {}", self.endpoint(path)))?;
        let status = response.status();
        let body = await_operation(operation, OperationStage::Resolve, response.text()).await?;
        if !status.is_success() {
            return Err(classify_response(status, &body, None, DEFAULT_MODEL).into());
        }
        serde_json::from_str(&body)
            .with_context(|| format!("Anthropic endpoint '{path}' returned invalid JSON"))
    }
}

fn field(name: &str, value: String) -> ConnectorStatusField {
    ConnectorStatusField {
        name: name.into(),
        value,
    }
}

/// One-turn Messages API model wrapped in `AgentRunner` by the provider factory.
#[derive(Clone)]
pub struct AnthropicTextModel {
    connector: AnthropicConnector,
    profile: ModelProfile,
    /// Raw assistant turns keyed by their first `tool_use` id.
    ///
    /// `ModelMessage::Assistant` cannot carry a thinking block, but Claude thinks
    /// by default and the API expects thinking blocks replayed unchanged. Memoing
    /// the raw response lets the next request echo it verbatim.
    turns: Arc<Mutex<TurnMemo>>,
    warned: Arc<AtomicBool>,
}

/// Bounded verbatim-replay store that evicts the oldest turn.
///
/// Clearing every entry instead would drop thinking blocks that the transcript
/// still has to replay unchanged, which the Messages API rejects mid-run.
#[derive(Default)]
pub(crate) struct TurnMemo {
    entries: BTreeMap<String, Vec<Value>>,
    /// Insertion order; `entries` is keyed by tool-use id, which carries none.
    order: VecDeque<String>,
}

impl TurnMemo {
    fn insert(&mut self, key: String, content: Vec<Value>) {
        if self.entries.remove(&key).is_some() {
            self.order.retain(|existing| *existing != key);
        }
        while self.entries.len() >= TURN_MEMO_LIMIT {
            match self.order.pop_front() {
                Some(oldest) => {
                    self.entries.remove(&oldest);
                }
                None => break,
            }
        }
        self.order.push_back(key.clone());
        self.entries.insert(key, content);
    }

    fn get(&self, key: &str) -> Option<&Vec<Value>> {
        self.entries.get(key)
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub(crate) fn contains(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }
}

impl AnthropicTextModel {
    fn model(&self) -> &str {
        if self.profile.model.trim().is_empty() {
            DEFAULT_MODEL
        } else {
            self.profile.model.as_str()
        }
    }

    /// Reads `max_tokens` directly rather than through `text_max_tokens()`, whose
    /// 4000 default would truncate: thinking is on by default and shares the
    /// budget with the response text.
    fn max_tokens(&self) -> u32 {
        self.profile
            .options
            .text
            .max_tokens
            .unwrap_or(DEFAULT_MAX_TOKENS)
            .clamp(MIN_MAX_TOKENS, MAX_NON_STREAMING_TOKENS)
    }

    /// Serializes one provider-neutral turn as a Messages API request.
    pub(crate) fn request_body(&self, request: &TextModelRequest) -> Result<MessagesRequest> {
        let (system, messages) = translate_messages(&request.messages, &self.turns)?;
        ensure_conversation_shape(&messages)?;
        let tools = (!request.tools.is_empty()).then(|| {
            request
                .tools
                .iter()
                .map(|tool| AnthropicTool {
                    name: tool.function.name.clone(),
                    description: tool.function.description.clone(),
                    input_schema: tool.function.parameters.clone(),
                })
                .collect::<Vec<_>>()
        });
        // The Messages API rejects any request whose transcript carries tool_use
        // or tool_result blocks without a `tools` array, which is exactly the
        // shape of the agent loop's tool-exhausted final turn. Redeclaring the
        // replayed tools keeps that request legal, and `tool_choice: none`
        // preserves the caller's intent that no further tool runs.
        let (tools, tool_choice) = match tools {
            Some(tools) => (Some(tools), None),
            None => match replayed_tool_definitions(&messages) {
                replayed if replayed.is_empty() => (None, None),
                replayed => (Some(replayed), Some(ToolChoice::None)),
            },
        };
        Ok(MessagesRequest {
            model: self.model().to_string(),
            max_tokens: self.max_tokens(),
            system,
            messages,
            tools,
            tool_choice,
        })
    }

    /// Warns once per stage about profile options this connector cannot honor.
    ///
    /// Covers both the inert sampling options and a `max_tokens` the adapter had
    /// to adjust, because the truncation error's remediation ("increase
    /// max_tokens") is unreachable above the non-streaming ceiling and the user
    /// would otherwise retry it forever.
    fn warn_unsupported_options(&self, operation: &OperationContext, stage: OperationStage) {
        let inert = [
            self.profile.options.text.temperature.map(|_| "temperature"),
            self.profile.options.text.top_p.map(|_| "top_p"),
            self.profile.options.text.seed.map(|_| "seed"),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        let adjusted = self
            .profile
            .options
            .text
            .max_tokens
            .filter(|configured| *configured != self.max_tokens());
        if (inert.is_empty() && adjusted.is_none()) || self.warned.swap(true, Ordering::AcqRel) {
            return;
        }
        let mut details = BTreeMap::from([
            ("activity".to_string(), "unsupported_option".to_string()),
            ("model".to_string(), self.model().to_string()),
        ]);
        if !inert.is_empty() {
            details.insert("options".to_string(), inert.join(", "));
        }
        if let Some(configured) = adjusted {
            details.insert(
                "max_tokens".to_string(),
                format!(
                    "{configured} adjusted to {}; non-streaming requests accept {MIN_MAX_TOKENS}-{MAX_NON_STREAMING_TOKENS}",
                    self.max_tokens()
                ),
            );
        }
        operation.emit(stage, OperationEventKind::Warning, details);
    }

    fn memoize_turn(&self, content: &[Value], tool_calls: &[ToolCall]) {
        let Some(first) = tool_calls.first().and_then(|call| call.id.clone()) else {
            return;
        };
        let Ok(mut turns) = self.turns.lock() else {
            return;
        };
        turns.insert(first, content.to_vec());
    }

    async fn send(
        &self,
        body: &MessagesRequest,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> Result<MessagesResponse> {
        let request = self.connector.post("messages", operation, stage).await?;
        let response = await_operation(operation, stage, request.json(body).send())
            .await
            // `provider_error` reads this phrase to classify a connect or DNS
            // failure as retryable rather than permanent.
            .with_context(|| format!("Could not reach {}", self.connector.endpoint("messages")))?;
        let status = response.status();
        let retry_after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned);
        let text = await_operation(operation, stage, response.text()).await?;
        if !status.is_success() {
            return Err(
                classify_response(status, &text, retry_after.as_deref(), self.model()).into(),
            );
        }
        serde_json::from_str(&text).context("Anthropic returned an invalid Messages API response")
    }
}

#[async_trait]
impl TextModel for AnthropicTextModel {
    async fn complete(
        &self,
        request: TextModelRequest,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<TextModelResponse> {
        self.warn_unsupported_options(operation, stage);
        let result: Result<TextModelResponse> = async {
            let body = self.request_body(&request)?;
            let parsed = self.send(&body, operation, stage).await?;
            let tool_calls = response_tool_calls(&parsed.content)?;
            self.memoize_turn(&parsed.content, &tool_calls);
            interpret_response(self.model(), self.max_tokens(), parsed, tool_calls)
        }
        .await;
        result.map_err(|error| provider_error(error, stage))
    }
}

/// Folds provider-neutral messages into a Messages API conversation.
fn translate_messages(
    messages: &[sfumato_core::providers::ModelMessage],
    turns: &Arc<Mutex<TurnMemo>>,
) -> Result<(Option<String>, Vec<AnthropicMessage>)> {
    use sfumato_core::providers::ModelMessage;

    let mut system: Vec<String> = Vec::new();
    let mut translated: Vec<AnthropicMessage> = Vec::new();
    for message in messages {
        match message {
            // System instructions are a top-level field, never a conversation turn.
            ModelMessage::System(text) => system.push(text.clone()),
            ModelMessage::User(text) => push_blocks(
                &mut translated,
                "user",
                vec![RequestBlock::Text { text: text.clone() }],
            ),
            ModelMessage::UserWithImages { content, images } => {
                let mut blocks = vec![RequestBlock::Text {
                    text: content.clone(),
                }];
                for image in images {
                    // The label goes in front of its own image: Claude has no
                    // caption field, so this is the only way it can name which
                    // frame it is talking about. An unreadable file keeps its
                    // label, so the model is told a frame is missing rather than
                    // judging a film it never fully saw.
                    blocks.push(RequestBlock::Text {
                        text: image.label.clone(),
                    });
                    match std::fs::read(&image.path) {
                        Ok(bytes) => blocks.push(RequestBlock::Image {
                            source: ImageSource {
                                kind: "base64",
                                media_type: image.media_type.clone(),
                                data: STANDARD.encode(&bytes),
                            },
                        }),
                        Err(error) => blocks.push(RequestBlock::Text {
                            text: format!("could not be read: {error}"),
                        }),
                    }
                }
                push_blocks(&mut translated, "user", blocks);
            }
            ModelMessage::Assistant {
                content,
                tool_calls,
            } => {
                let blocks = replay_or_rebuild(content.as_deref(), tool_calls, turns);
                push_blocks(&mut translated, "assistant", blocks);
            }
            ModelMessage::Tool {
                tool_call_id,
                name,
                content,
                failed,
            } => {
                let tool_use_id = tool_call_id.clone().with_context(|| {
                    format!(
                        "Tool result for '{name}' is missing the provider tool-call id that Anthropic requires"
                    )
                })?;
                // Coalesced into the trailing user turn: Anthropic expects every
                // tool_result for one assistant turn in a single user message, and
                // splitting them degrades parallel tool use.
                push_blocks(
                    &mut translated,
                    "user",
                    vec![RequestBlock::ToolResult {
                        tool_use_id,
                        content: content.clone(),
                        // Anthropic's own failure signal: without it Claude reads
                        // a failed tool call as having returned successfully.
                        is_error: *failed,
                    }],
                );
            }
        }
    }
    let system = (!system.is_empty()).then(|| system.join("\n\n"));
    Ok((system, translated))
}

fn push_blocks(
    messages: &mut Vec<AnthropicMessage>,
    role: &'static str,
    blocks: Vec<RequestBlock>,
) {
    if blocks.is_empty() {
        return;
    }
    match messages.last_mut() {
        Some(last) if last.role == role => last.content.extend(blocks),
        _ => messages.push(AnthropicMessage {
            role,
            content: blocks,
        }),
    }
}

fn replay_or_rebuild(
    content: Option<&str>,
    tool_calls: &[ToolCall],
    turns: &Arc<Mutex<TurnMemo>>,
) -> Vec<RequestBlock> {
    let memoized = tool_calls
        .first()
        .and_then(|call| call.id.as_deref())
        .and_then(|id| turns.lock().ok()?.get(id).cloned());
    if let Some(raw) = memoized {
        return raw.into_iter().map(RequestBlock::Verbatim).collect();
    }
    let mut blocks = Vec::new();
    if let Some(text) = content.map(str::trim).filter(|text| !text.is_empty()) {
        blocks.push(RequestBlock::Text {
            text: text.to_string(),
        });
    }
    for call in tool_calls {
        blocks.push(RequestBlock::ToolUse {
            id: call.id.clone().unwrap_or_default(),
            name: call.function.name.clone(),
            input: call.function.arguments.clone(),
        });
    }
    blocks
}

/// Collects placeholder definitions for every tool the transcript already used.
///
/// Names come from both rebuilt and verbatim-replayed blocks, because a memoized
/// turn carries its `tool_use` blocks as raw provider JSON. The schema is
/// deliberately permissive: these definitions exist to make the transcript legal,
/// not to invite another call.
pub(crate) fn replayed_tool_definitions(messages: &[AnthropicMessage]) -> Vec<AnthropicTool> {
    let mut names: Vec<String> = Vec::new();
    for block in messages.iter().flat_map(|message| message.content.iter()) {
        let name = match block {
            RequestBlock::ToolUse { name, .. } => Some(name.clone()),
            RequestBlock::Verbatim(value)
                if value.get("type").and_then(Value::as_str) == Some("tool_use") =>
            {
                value
                    .get("name")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            }
            _ => None,
        };
        if let Some(name) = name.filter(|name| !names.contains(name)) {
            names.push(name);
        }
    }
    names
        .into_iter()
        .map(|name| AnthropicTool {
            name,
            description: "Used earlier in this conversation; not callable on this turn."
                .to_string(),
            input_schema: json!({"type": "object"}),
        })
        .collect()
}

/// Rejects shapes the Messages API refuses, including assistant prefills.
fn ensure_conversation_shape(messages: &[AnthropicMessage]) -> Result<()> {
    let first = messages
        .first()
        .context("Anthropic requests require at least one message")?;
    if first.role != "user" {
        bail!("Anthropic conversations must begin with a user message");
    }
    if messages.last().is_some_and(|last| last.role == "assistant") {
        bail!(
            "Anthropic rejects a trailing assistant turn; assistant prefill is unsupported on current models"
        );
    }
    Ok(())
}

fn response_text(content: &[Value]) -> Option<String> {
    let text = content
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    (!text.trim().is_empty()).then_some(text)
}

fn response_tool_calls(content: &[Value]) -> Result<Vec<ToolCall>> {
    content
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("tool_use"))
        .map(|block| {
            let name = block
                .get("name")
                .and_then(Value::as_str)
                .context("Anthropic tool_use block is missing its name")?;
            Ok(ToolCall {
                id: block
                    .get("id")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                kind: "function".to_string(),
                function: ToolCallFunction {
                    name: name.to_string(),
                    // Already a JSON object, unlike OpenAI's stringified arguments.
                    arguments: block.get("input").cloned().unwrap_or(Value::Null),
                },
            })
        })
        .collect()
}

fn interpret_response(
    model: &str,
    max_tokens: u32,
    parsed: MessagesResponse,
    tool_calls: Vec<ToolCall>,
) -> Result<TextModelResponse> {
    let content = response_text(&parsed.content);
    let completion_tokens = parsed.usage.as_ref().and_then(|usage| usage.output_tokens);
    let empty = content.is_none() && tool_calls.is_empty();
    let stop_reason = parsed.stop_reason.as_deref();

    // Checked before the tool-call shortcut below: a `tool_use` block cut off at
    // the output cap has partial arguments, and executing it is worse than
    // surfacing the typed truncation error.
    if stop_reason == Some("max_tokens") {
        return Err(TextGenerationLimitError::output(
            model.to_string(),
            u64::from(max_tokens),
            stop_reason.map(ToOwned::to_owned),
            completion_tokens,
            None,
            empty,
        )
        .into());
    }
    if !tool_calls.is_empty() {
        return Ok(TextModelResponse {
            content,
            tool_calls,
        });
    }
    match stop_reason {
        Some("end_turn" | "stop_sequence") if !empty => Ok(TextModelResponse {
            content,
            tool_calls,
        }),
        // Truncation is handled above; this arm is the empty end_turn case.
        Some("end_turn" | "stop_sequence") => Err(TextGenerationLimitError::output(
            model.to_string(),
            u64::from(max_tokens),
            stop_reason.map(ToOwned::to_owned),
            completion_tokens,
            None,
            empty,
        )
        .into()),
        Some("model_context_window_exceeded") => Err(TextGenerationLimitError::context(
            model.to_string(),
            u64::from(max_tokens),
            "the request exceeded the model's context window".to_string(),
        )
        .into()),
        Some("pause_turn") => Err(SfumatoError::provider(
            ErrorClass::Retry,
            "Anthropic paused the turn; resending the transcript resumes it",
        )
        .into()),
        Some("refusal") => {
            let mut error = SfumatoError::provider(
                ErrorClass::Permanent,
                "Anthropic declined the request for policy reasons",
            );
            if let Some(details) = &parsed.stop_details {
                if let Some(category) = &details.category {
                    error = error.with_detail("refusal_category", category.clone());
                }
                if let Some(explanation) = &details.explanation {
                    error = error.with_detail("refusal_explanation", explanation.clone());
                }
            }
            Err(error.into())
        }
        other => Err(SfumatoError::provider(
            ErrorClass::InvalidOutput,
            format_args!(
                "Anthropic returned an unrecognized stop reason '{}'",
                other.unwrap_or("none")
            ),
        )
        .into()),
    }
}

/// Maps an Anthropic error response onto a typed recovery class.
pub(crate) fn classify_response(
    status: StatusCode,
    body: &str,
    retry_after: Option<&str>,
    model: &str,
) -> SfumatoError {
    let envelope = serde_json::from_str::<ApiErrorEnvelope>(body).ok();
    let detail = compact_error_detail(body);
    let request_id = envelope
        .as_ref()
        .and_then(|envelope| envelope.request_id.clone());

    let error = match status.as_u16() {
        400 if is_context_limit_response(body) => SfumatoError::from(
            TextGenerationLimitError::context(model.to_string(), 0, detail.clone()),
        ),
        // Compaction, not surrender, is the right recovery for an oversized request.
        413 => SfumatoError::from(TextGenerationLimitError::context(
            model.to_string(),
            0,
            detail.clone(),
        )),
        401 => SfumatoError::provider(
            ErrorClass::Permanent,
            format_args!(
                "Anthropic rejected the credential: {detail}. Run `sfumato connector login <name>`."
            ),
        ),
        429 => SfumatoError::provider(
            ErrorClass::Retry,
            format_args!("Anthropic rate limit reached: {detail}"),
        ),
        529 => SfumatoError::provider(
            ErrorClass::Unavailable,
            format_args!("Anthropic is overloaded: {detail}"),
        ),
        500..=504 => SfumatoError::provider(
            ErrorClass::Retry,
            format_args!("Anthropic returned HTTP {status}: {detail}"),
        ),
        _ => SfumatoError::provider(
            ErrorClass::Permanent,
            format_args!("Anthropic returned HTTP {status}: {detail}"),
        ),
    };
    let mut error = error.with_detail("model", model.to_string());
    if let Some(request_id) = request_id {
        error = error.with_detail("request_id", request_id);
    }
    if let Some(retry_after) = retry_after {
        error = error.with_detail("retry_after_seconds", retry_after.to_string());
    }
    error
}

fn provider_error(error: anyhow::Error, stage: OperationStage) -> SfumatoError {
    if let Some(error) = error.downcast_ref::<SfumatoError>() {
        let mut error = error.clone();
        if error.stage.is_none() {
            error.stage = Some(stage);
        }
        return error;
    }
    if let Some(limit) = error.downcast_ref::<TextGenerationLimitError>() {
        return SfumatoError::from(limit.clone()).at_stage(stage);
    }
    let message = format!("{error:#}");
    let class = if message.contains("Could not reach") || message.contains("timed out") {
        ErrorClass::Retry
    } else {
        ErrorClass::Permanent
    };
    SfumatoError::provider(class, message).at_stage(stage)
}

fn native_result<T>(result: Result<T>) -> SfumatoResult<T> {
    result.map_err(|error| {
        if let Some(error) = error.downcast_ref::<SfumatoError>() {
            return error.clone();
        }
        SfumatoError::provider(ErrorClass::Unavailable, format_args!("{error:#}"))
            .at_stage(OperationStage::Resolve)
    })
}

/// Request body. Deliberately closed: `temperature`, `top_p`, `top_k`, `stream`,
/// `thinking`, and `output_config` are absent by type rather than by a runtime
/// check, because current Claude models reject the sampling parameters outright
/// and the rest should take server defaults. `tool_choice` is sent only to
/// disable tool use on a turn whose tools are replayed placeholders.
#[derive(Debug, Serialize)]
pub(crate) struct MessagesRequest {
    pub model: String,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    pub messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<AnthropicTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum ToolChoice {
    None,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct AnthropicMessage {
    pub role: &'static str,
    pub content: Vec<RequestBlock>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum RequestBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        is_error: bool,
    },
    Image {
        source: ImageSource,
    },
    /// Provider-native block replayed unchanged, such as a thinking block.
    #[serde(untagged)]
    Verbatim(Value),
}

/// How an image block carries its bytes.
///
/// Only the inline form is used: Sfumato reads snapshots off the local
/// filesystem, so there is no URL to hand Anthropic instead.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct ImageSource {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub media_type: String,
    pub data: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct AnthropicTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MessagesResponse {
    #[serde(default)]
    pub content: Vec<Value>,
    #[serde(default)]
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub stop_details: Option<StopDetails>,
    #[serde(default)]
    pub usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct StopDetails {
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub explanation: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Usage {
    #[serde(default)]
    pub output_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ApiErrorEnvelope {
    #[serde(default)]
    request_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ModelsResponse {
    #[serde(default)]
    pub data: Vec<AnthropicModel>,
    #[serde(default)]
    pub has_more: bool,
    #[serde(default)]
    pub last_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct AnthropicModel {
    pub id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    /// The context window. There is no `context_window` field.
    #[serde(default)]
    pub max_input_tokens: Option<u64>,
    /// The output cap, which is not the context window.
    #[serde(default)]
    pub max_tokens: Option<u64>,
    #[serde(default)]
    pub capabilities: Option<ModelCapabilities>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct ModelCapabilities {
    #[serde(default)]
    pub image_input: Supported,
    #[serde(default)]
    pub structured_outputs: Supported,
    #[serde(default)]
    pub thinking: ThinkingCapability,
    #[serde(default)]
    pub effort: EffortCapability,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
pub(crate) struct Supported {
    #[serde(default)]
    pub supported: bool,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct ThinkingCapability {
    #[serde(default)]
    pub types: BTreeMap<String, Value>,
}

/// Effort levels are named fields, not flattened: a `BTreeMap<String, Supported>`
/// would try to deserialize the sibling `supported` bool and fail the catalog.
#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct EffortCapability {
    #[serde(default)]
    pub low: Supported,
    #[serde(default)]
    pub medium: Supported,
    #[serde(default)]
    pub high: Supported,
    #[serde(default)]
    pub xhigh: Supported,
    #[serde(default)]
    pub max: Supported,
}

/// Anthropic paginates with `after_id` cursors, not the `page`/`next_page` scheme.
pub(crate) fn next_page_cursor(page: &ModelsResponse) -> Option<String> {
    page.has_more.then(|| page.last_id.clone()).flatten()
}

pub(crate) fn map_model(model: AnthropicModel) -> ConnectorModelSummary {
    let capabilities = model.capabilities.clone().unwrap_or_default();
    let mut input_modalities = vec!["text".to_string()];
    if capabilities.image_input.supported {
        input_modalities.push("image".to_string());
    }
    let mut metadata = BTreeMap::new();
    if let Some(created_at) = model.created_at {
        metadata.insert("created_at".to_string(), created_at);
    }
    if let Some(max_output) = model.max_tokens {
        metadata.insert("max_output_tokens".to_string(), max_output.to_string());
    }
    if capabilities.structured_outputs.supported {
        metadata.insert("structured_outputs".to_string(), "true".to_string());
    }
    if capabilities
        .thinking
        .types
        .get("adaptive")
        .and_then(|value| value.pointer("/supported"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        metadata.insert("thinking".to_string(), "adaptive".to_string());
    }
    let effort = [
        ("low", capabilities.effort.low.supported),
        ("medium", capabilities.effort.medium.supported),
        ("high", capabilities.effort.high.supported),
        ("xhigh", capabilities.effort.xhigh.supported),
        ("max", capabilities.effort.max.supported),
    ]
    .into_iter()
    .filter_map(|(level, supported)| supported.then_some(level))
    .collect::<Vec<_>>();
    if !effort.is_empty() {
        metadata.insert("effort_levels".to_string(), effort.join(", "));
    }
    ConnectorModelSummary {
        is_default: model.id == DEFAULT_MODEL,
        display_name: model.display_name.unwrap_or_else(|| model.id.clone()),
        id: model.id,
        hidden: false,
        input_modalities,
        // The Messages API only ever returns text.
        output_modalities: vec!["text".to_string()],
        context_length: model.max_input_tokens,
        description: None,
        metadata,
    }
}

#[cfg(test)]
#[path = "../tests/unit/anthropic.rs"]
mod tests;
