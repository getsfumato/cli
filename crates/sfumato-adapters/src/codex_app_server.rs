//! Codex App Server transport using Codex-owned ChatGPT authentication.

use std::{
    path::{Path, PathBuf},
    process::Stdio,
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sfumato_core::{
    config::{CodexAppServerConnectorConfig, ModelProfile},
    errors::{ErrorClass, OperationStage, SfumatoError, SfumatoResult},
    operation::{OperationContext, OperationEventKind},
    providers::{
        ConnectorStatus, ConnectorStatusField, ImageAttachment, ImageGenerationProvider,
        ImageGenerationRequest, ImageGenerationResponse, TextGenerationEvent,
        TextGenerationProvider, TextGenerationRequest, TextGenerationResponse, ToolDefinition,
        ToolExecutionRequest,
    },
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::Mutex,
};

use crate::runtime::await_operation;

const CLIENT_NAME: &str = "sfumato";
const CLIENT_TITLE: &str = "Sfumato CLI";
const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// One model advertised by the authenticated Codex installation.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexModel {
    /// Stable model picker identifier.
    pub id: String,
    /// Model value accepted by `thread/start`.
    pub model: String,
    /// Human-readable model name.
    pub display_name: String,
    /// Whether Codex recommends this model by default.
    #[serde(default)]
    pub is_default: bool,
    /// Whether the model is hidden from normal pickers.
    #[serde(default)]
    pub hidden: bool,
    /// Supported input modalities.
    #[serde(default)]
    pub input_modalities: Vec<String>,
}

/// Text-generation provider backed by one persistent local App Server process.
pub struct CodexAppServerProvider {
    config: CodexAppServerConnectorConfig,
    profile: ModelProfile,
    project_root: PathBuf,
    process: Mutex<Option<CodexAppServerProcess>>,
}

impl CodexAppServerProvider {
    /// Creates a provider. The App Server starts lazily on the first request.
    pub fn new(
        config: CodexAppServerConnectorConfig,
        profile: ModelProfile,
        project_root: PathBuf,
    ) -> Self {
        Self {
            config,
            profile,
            project_root,
            process: Mutex::new(None),
        }
    }

    /// Discovers models available to the current Codex authentication.
    pub async fn discover_models(
        config: &CodexAppServerConnectorConfig,
        operation: &OperationContext,
    ) -> SfumatoResult<Vec<CodexModel>> {
        let mut process = CodexAppServerProcess::spawn(&config.executable, operation).await?;
        process
            .list_models(operation, OperationStage::Resolve)
            .await
    }

    /// Reads the authenticated Codex account and current rate-limit windows.
    pub async fn discover_status(
        name: &str,
        config: &CodexAppServerConnectorConfig,
        operation: &OperationContext,
    ) -> SfumatoResult<ConnectorStatus> {
        let mut process = CodexAppServerProcess::spawn(&config.executable, operation).await?;
        let account = process
            .request(
                "account/read",
                json!({ "refreshToken": false }),
                operation,
                OperationStage::Resolve,
            )
            .await?;
        let limits = process
            .request(
                "account/rateLimits/read",
                json!({}),
                operation,
                OperationStage::Resolve,
            )
            .await?;
        let mut fields = Vec::new();
        push_pointer(&mut fields, "account_type", &account, "/account/type");
        push_pointer(&mut fields, "email", &account, "/account/email");
        push_pointer(&mut fields, "plan", &account, "/account/planType");
        push_pointer(
            &mut fields,
            "requires_openai_auth",
            &account,
            "/requiresOpenaiAuth",
        );
        push_pointer(
            &mut fields,
            "primary_used_percent",
            &limits,
            "/rateLimits/primary/usedPercent",
        );
        push_pointer(
            &mut fields,
            "primary_resets_at",
            &limits,
            "/rateLimits/primary/resetsAt",
        );
        push_pointer(
            &mut fields,
            "secondary_used_percent",
            &limits,
            "/rateLimits/secondary/usedPercent",
        );
        push_pointer(
            &mut fields,
            "secondary_resets_at",
            &limits,
            "/rateLimits/secondary/resetsAt",
        );
        push_pointer(
            &mut fields,
            "credit_balance",
            &limits,
            "/rateLimits/credits/balance",
        );
        Ok(ConnectorStatus {
            connector: name.into(),
            kind: "codex_app_server".into(),
            fields,
        })
    }

    async fn generate(
        &self,
        request: &TextGenerationRequest,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<TextGenerationResponse> {
        let mut process = self.process.lock().await;
        if process.is_none() {
            *process =
                Some(CodexAppServerProcess::spawn(&self.config.executable, operation).await?);
        }
        let result = process
            .as_mut()
            .expect("App Server initialized above")
            .generate(&self.profile, &self.project_root, request, operation, stage)
            .await;
        if result.is_err()
            && let Some(mut failed) = process.take()
        {
            let _ = failed.child.start_kill();
        }
        result
    }
}

fn push_pointer(fields: &mut Vec<ConnectorStatusField>, name: &str, value: &Value, pointer: &str) {
    if let Some(value) = value.pointer(pointer) {
        let rendered = value
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| value.to_string());
        if rendered != "null" {
            fields.push(ConnectorStatusField {
                name: name.into(),
                value: rendered,
            });
        }
    }
}

#[async_trait]
impl TextGenerationProvider for CodexAppServerProvider {
    async fn generate_text(
        &self,
        request: TextGenerationRequest,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<TextGenerationResponse> {
        operation.checkpoint(stage)?;
        self.generate(&request, operation, stage).await
    }
}

/// Image generation backed by the Codex-native image tool.
///
/// Indirect by nature, and that is the whole caveat: the protocol exposes no
/// "generate this image" call. An agent turn is started, the model decides to
/// invoke its own image tool, and the generated PNG arrives as a saved file path.
/// A model that answers in prose instead has produced no image, which this reports
/// as a tool failure rather than as an empty success. The payoff is that it runs on
/// Codex's own authentication instead of a metered image endpoint.
pub struct CodexAppServerImageProvider {
    config: CodexAppServerConnectorConfig,
    profile: ModelProfile,
    project_root: PathBuf,
}

impl CodexAppServerImageProvider {
    /// Creates a provider. The App Server starts on each request.
    pub fn new(
        config: CodexAppServerConnectorConfig,
        profile: ModelProfile,
        project_root: PathBuf,
    ) -> Self {
        Self {
            config,
            profile,
            project_root,
        }
    }
}

#[async_trait]
impl ImageGenerationProvider for CodexAppServerImageProvider {
    async fn generate_image(
        &self,
        request: ImageGenerationRequest,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<ImageGenerationResponse> {
        operation.checkpoint(stage)?;
        let mut process = CodexAppServerProcess::spawn(&self.config.executable, operation).await?;
        let result = process
            .generate_image(
                &self.profile,
                &self.project_root,
                &request,
                operation,
                stage,
            )
            .await;
        let _ = process.child.start_kill();
        result
    }
}

/// The instruction that asks for one image and nothing else.
///
/// Explicit about the single deliverable because the turn is a conversation: a
/// model left to its own judgement narrates what it would draw.
fn image_turn_prompt(prompt: &str) -> String {
    format!(
        "Generate exactly one image using your image generation tool, then stop.\n\n\
         Image to generate:\n{prompt}\n\n\
         Do not describe the image, do not ask a question, and do not propose \
         alternatives. Invoke the tool."
    )
}

/// Explains a turn that ended without an image, with what to do instead.
fn missing_image_error(model: &str, answer: &str, stage: OperationStage) -> SfumatoError {
    let quoted = if answer.trim().is_empty() {
        "it returned no message at all".to_string()
    } else {
        format!("it answered: \"{}\"", excerpt(answer.trim(), 300))
    };
    SfumatoError::provider(
        ErrorClass::InvalidOutput,
        format!(
            "Codex model '{model}' did not generate an image; {quoted}. \
             Codex generates images only when the model chooses to invoke its own \
             image tool during a turn, so a refusal or a written description \
             produces no file. Either retry, or configure an image profile on a \
             connector with a direct image endpoint, for example:\n\n\
             [models.gpt-image]\nconnector = \"openrouter\"\n\
             model = \"openai/gpt-image-2\"\ncapabilities = [\"image\"]"
        ),
    )
    .at_stage(stage)
}

/// Shortens a model answer for an error message without splitting a character.
fn excerpt(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let kept: String = value.chars().take(max_chars).collect();
    format!("{kept}…")
}

struct CodexAppServerProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_request_id: u64,
    models: Option<Vec<CodexModel>>,
}

impl CodexAppServerProcess {
    async fn spawn(executable: &PathBuf, operation: &OperationContext) -> SfumatoResult<Self> {
        operation.checkpoint(OperationStage::Resolve)?;
        let mut command = Command::new(executable);
        command
            .arg("app-server")
            .arg("--listen")
            .arg("stdio://")
            .kill_on_drop(true)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command
            .spawn()
            .map_err(|error| codex_process_error(error, executable, OperationStage::Resolve))?;
        let stdin = child.stdin.take().ok_or_else(|| {
            SfumatoError::provider(
                ErrorClass::Unavailable,
                "Codex App Server stdin was not available",
            )
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            SfumatoError::provider(
                ErrorClass::Unavailable,
                "Codex App Server stdout was not available",
            )
        })?;
        let mut process = Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_request_id: 1,
            models: None,
        };
        process.initialize(operation).await?;
        Ok(process)
    }

    async fn initialize(&mut self, operation: &OperationContext) -> SfumatoResult<()> {
        self.request(
            "initialize",
            json!({
                "clientInfo": {
                    "name": CLIENT_NAME,
                    "title": CLIENT_TITLE,
                    "version": CLIENT_VERSION,
                },
                "capabilities": {
                    "experimentalApi": true,
                },
            }),
            operation,
            OperationStage::Resolve,
        )
        .await?;
        self.notify("initialized", json!({}), operation, OperationStage::Resolve)
            .await
    }

    async fn list_models(
        &mut self,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<Vec<CodexModel>> {
        if let Some(models) = &self.models {
            return Ok(models.clone());
        }
        let mut models = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let result = self
                .request(
                    "model/list",
                    json!({
                        "cursor": cursor,
                        "limit": 100,
                        "includeHidden": true,
                    }),
                    operation,
                    stage,
                )
                .await?;
            let page: ModelListResponse = serde_json::from_value(result).map_err(|error| {
                protocol_error(stage, format_args!("Invalid model/list response: {error}"))
            })?;
            models.extend(page.data);
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        self.models = Some(models.clone());
        Ok(models)
    }

    async fn generate(
        &mut self,
        profile: &ModelProfile,
        project_root: &PathBuf,
        request: &TextGenerationRequest,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<TextGenerationResponse> {
        let models = self.list_models(operation, stage).await?;
        let model = resolve_model(&models, &profile.model, stage)?;
        request.emit(TextGenerationEvent::ModelSelected {
            model: model.model.clone(),
            display_name: model.display_name.clone(),
        });
        operation.emit(
            stage,
            OperationEventKind::Progress,
            [
                ("activity".to_string(), "model_selected".to_string()),
                ("model".to_string(), model.model.clone()),
            ]
            .into(),
        );

        let dynamic_tools = dynamic_tools(&request.tools);
        let thread = self
            .request(
                "thread/start",
                json!({
                    "model": model.model,
                    "cwd": project_root,
                    "approvalPolicy": "never",
                    "sandbox": "read-only",
                    "ephemeral": true,
                    "serviceName": CLIENT_NAME,
                    "baseInstructions": request.system_prompt,
                    "dynamicTools": dynamic_tools,
                }),
                operation,
                stage,
            )
            .await?;
        let thread_id = thread
            .pointer("/thread/id")
            .and_then(Value::as_str)
            .ok_or_else(|| protocol_error(stage, "thread/start response omitted thread.id"))?
            .to_string();

        request.emit(TextGenerationEvent::RequestStarted { round: 1 });
        let turn = self
            .request(
                "turn/start",
                json!({
                    "threadId": thread_id,
                    "input": turn_input(&request.user_prompt, &request.images, model, stage)?,
                    "model": model.model,
                    "cwd": project_root,
                    "approvalPolicy": "never",
                    "sandboxPolicy": { "type": "readOnly" },
                }),
                operation,
                stage,
            )
            .await?;
        let turn_id = turn
            .pointer("/turn/id")
            .and_then(Value::as_str)
            .ok_or_else(|| protocol_error(stage, "turn/start response omitted turn.id"))?
            .to_string();

        self.drive_turn(&thread_id, &turn_id, request, operation, stage)
            .await
    }

    /// Runs one turn whose only purpose is to produce an image file.
    async fn generate_image(
        &mut self,
        profile: &ModelProfile,
        project_root: &Path,
        request: &ImageGenerationRequest,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<ImageGenerationResponse> {
        let models = self.list_models(operation, stage).await?;
        let model = resolve_model(&models, &profile.model, stage)?.clone();
        let project_root = project_root.display().to_string();
        let thread = self
            .request(
                "thread/start",
                json!({ "cwd": project_root }),
                operation,
                stage,
            )
            .await?;
        let thread_id = thread
            .pointer("/thread/id")
            .and_then(Value::as_str)
            .ok_or_else(|| protocol_error(stage, "thread/start response omitted thread.id"))?
            .to_string();
        let turn = self
            .request(
                "turn/start",
                json!({
                    "threadId": thread_id,
                    "input": [{ "type": "text", "text": image_turn_prompt(&request.prompt) }],
                    "model": model.model,
                    "cwd": project_root,
                    "approvalPolicy": "never",
                    // The image tool writes its own artifact, so the turn needs to
                    // be allowed to put a file on disk.
                    "sandboxPolicy": { "type": "workspaceWrite" },
                }),
                operation,
                stage,
            )
            .await?;
        let turn_id = turn
            .pointer("/turn/id")
            .and_then(Value::as_str)
            .ok_or_else(|| protocol_error(stage, "turn/start response omitted turn.id"))?
            .to_string();

        let mut saved_path = None;
        let mut answer = String::new();
        loop {
            let message = self.read_message(operation, stage).await?;
            match message.get("method").and_then(Value::as_str) {
                Some("item/completed") => {
                    if let Some(path) = generated_image_path(&message) {
                        saved_path = Some(PathBuf::from(path));
                    }
                    if let Some(text) = completed_agent_text(&message) {
                        answer = text.to_string();
                    }
                }
                Some("turn/completed") => {
                    let params = message.get("params").cloned().unwrap_or(Value::Null);
                    if params.get("threadId").and_then(Value::as_str) != Some(thread_id.as_str())
                        || params.pointer("/turn/id").and_then(Value::as_str)
                            != Some(turn_id.as_str())
                    {
                        continue;
                    }
                    let status = params
                        .pointer("/turn/status")
                        .and_then(Value::as_str)
                        .unwrap_or("failed");
                    if status != "completed" {
                        return Err(turn_error(&params, status, stage));
                    }
                    let path = saved_path
                        .ok_or_else(|| missing_image_error(&model.model, &answer, stage))?;
                    let bytes = std::fs::read(&path).map_err(|error| {
                        SfumatoError::provider(
                            ErrorClass::InvalidOutput,
                            format!(
                                "Codex reported a generated image at {} that could not be read: {error}",
                                path.display()
                            ),
                        )
                        .at_stage(stage)
                    })?;
                    return Ok(ImageGenerationResponse {
                        bytes,
                        // The image tool writes PNG artifacts.
                        media_type: "image/png".to_string(),
                    });
                }
                // A request from the server, which this turn exposes no tools for.
                Some(method) if message.get("id").is_some() => {
                    self.respond_error(
                        message.get("id").cloned().unwrap_or(Value::Null),
                        -32601,
                        &format!("Sfumato does not support App Server request '{method}'"),
                        operation,
                        stage,
                    )
                    .await?;
                }
                _ => {}
            }
        }
    }

    async fn drive_turn(
        &mut self,
        thread_id: &str,
        turn_id: &str,
        request: &TextGenerationRequest,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<TextGenerationResponse> {
        let mut final_text = None;
        let mut tool_calls = 0usize;
        loop {
            let message = self.read_message(operation, stage).await?;
            if message.get("method").and_then(Value::as_str) == Some("item/tool/call")
                && message.get("id").is_some()
            {
                tool_calls += 1;
                if tool_calls > tool_call_ceiling(request.max_tool_rounds) {
                    return Err(SfumatoError::provider(
                        ErrorClass::InvalidOutput,
                        format!(
                            "Codex App Server kept requesting tools through {} refusals after Sfumato's limit of {} was reported",
                            TOOL_REFUSALS_BEFORE_FAILING, request.max_tool_rounds
                        ),
                    )
                    .at_stage(stage));
                }
                self.handle_tool_call(&message, tool_calls, request, operation, stage)
                    .await?;
                continue;
            }
            match message.get("method").and_then(Value::as_str) {
                Some("item/completed") => {
                    if let Some(text) = completed_agent_text(&message) {
                        final_text = Some(text.to_string());
                    }
                }
                Some("turn/completed") => {
                    let params = message.get("params").cloned().unwrap_or(Value::Null);
                    let completed_thread = params
                        .get("threadId")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let completed_turn = params
                        .pointer("/turn/id")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if completed_thread != thread_id || completed_turn != turn_id {
                        continue;
                    }
                    let status = params
                        .pointer("/turn/status")
                        .and_then(Value::as_str)
                        .unwrap_or("failed");
                    if status != "completed" {
                        return Err(turn_error(&params, status, stage));
                    }
                    if final_text.is_none() {
                        final_text = final_agent_text_from_turn(&params).map(str::to_string);
                    }
                    let text = final_text.unwrap_or_default().trim().to_string();
                    if text.is_empty() {
                        return Err(SfumatoError::provider(
                            ErrorClass::InvalidOutput,
                            "Codex App Server completed without a final agent message",
                        )
                        .at_stage(stage));
                    }
                    request.emit(TextGenerationEvent::ResponseCompleted);
                    return Ok(TextGenerationResponse { text });
                }
                Some(method) if message.get("id").is_some() => {
                    self.respond_error(
                        message.get("id").cloned().unwrap_or(Value::Null),
                        -32601,
                        &format!("Sfumato does not support App Server request '{method}'"),
                        operation,
                        stage,
                    )
                    .await?;
                }
                _ => {}
            }
        }
    }

    async fn handle_tool_call(
        &mut self,
        message: &Value,
        tool_calls: usize,
        request: &TextGenerationRequest,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<()> {
        let id = message.get("id").cloned().unwrap_or(Value::Null);
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let name = params
            .get("tool")
            .and_then(Value::as_str)
            .ok_or_else(|| protocol_error(stage, "item/tool/call omitted params.tool"))?
            .to_string();
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        request.emit(TextGenerationEvent::ToolCallRequested {
            name: name.clone(),
            arguments: arguments.clone(),
        });
        operation.emit(
            stage,
            OperationEventKind::Progress,
            [
                ("activity".to_string(), "tool_call".to_string()),
                ("tool".to_string(), name.clone()),
            ]
            .into(),
        );

        if tool_calls > request.max_tool_rounds {
            let error = request.tool_exhausted_prompt.as_deref().unwrap_or(
                "Sfumato's tool-call limit was reached. Finish with the available context.",
            );
            request.emit(TextGenerationEvent::ToolCallFailed {
                name,
                error: error.to_string(),
            });
            return self
                .respond_dynamic_tool(id, error.to_string(), false, operation, stage)
                .await;
        }

        let Some(executor) = request.tool_executor.as_ref() else {
            let error = "No Sfumato tool executor is available".to_string();
            request.emit(TextGenerationEvent::ToolCallFailed {
                name: name.clone(),
                error: error.clone(),
            });
            return self
                .respond_dynamic_tool(id, error, false, operation, stage)
                .await;
        };
        match executor
            .execute(
                ToolExecutionRequest {
                    name: name.clone(),
                    arguments,
                },
                operation,
                stage,
            )
            .await
        {
            Ok(result) => {
                request.emit(TextGenerationEvent::ToolCallSucceeded {
                    name,
                    result: result.clone(),
                });
                self.respond_dynamic_tool(id, result, true, operation, stage)
                    .await
            }
            Err(error) if error.class == ErrorClass::Cancelled => Err(error),
            Err(error) => {
                let error = error.to_string();
                request.emit(TextGenerationEvent::ToolCallFailed {
                    name,
                    error: error.clone(),
                });
                self.respond_dynamic_tool(id, error, false, operation, stage)
                    .await
            }
        }
    }

    async fn respond_dynamic_tool(
        &mut self,
        id: Value,
        text: String,
        success: bool,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<()> {
        self.write_message(
            &json!({
                "id": id,
                "result": {
                    "contentItems": [{ "type": "inputText", "text": text }],
                    "success": success,
                }
            }),
            operation,
            stage,
        )
        .await
    }

    async fn request(
        &mut self,
        method: &str,
        params: Value,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<Value> {
        let id = self.next_request_id;
        self.next_request_id += 1;
        self.write_message(
            &json!({ "id": id, "method": method, "params": params }),
            operation,
            stage,
        )
        .await?;
        loop {
            let message = self.read_message(operation, stage).await?;
            if message.get("id").and_then(Value::as_u64) != Some(id) {
                if message.get("id").is_some() && message.get("method").is_some() {
                    self.respond_error(
                        message.get("id").cloned().unwrap_or(Value::Null),
                        -32601,
                        "Sfumato received an unexpected App Server request",
                        operation,
                        stage,
                    )
                    .await?;
                }
                continue;
            }
            if let Some(error) = message.get("error") {
                return Err(json_rpc_error(method, error, stage));
            }
            return message.get("result").cloned().ok_or_else(|| {
                protocol_error(stage, format_args!("{method} response omitted result"))
            });
        }
    }

    async fn notify(
        &mut self,
        method: &str,
        params: Value,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<()> {
        self.write_message(
            &json!({ "method": method, "params": params }),
            operation,
            stage,
        )
        .await
    }

    async fn respond_error(
        &mut self,
        id: Value,
        code: i64,
        message: &str,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<()> {
        self.write_message(
            &json!({ "id": id, "error": { "code": code, "message": message } }),
            operation,
            stage,
        )
        .await
    }

    async fn write_message(
        &mut self,
        message: &Value,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<()> {
        let mut bytes = serde_json::to_vec(message).map_err(|error| {
            protocol_error(stage, format_args!("Could not encode JSON-RPC: {error}"))
        })?;
        bytes.push(b'\n');
        await_operation(operation, stage, async {
            self.stdin.write_all(&bytes).await?;
            self.stdin.flush().await
        })
        .await
        .map_err(|error| runtime_error(error, stage, "write to"))
    }

    async fn read_message(
        &mut self,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<Value> {
        loop {
            let mut line = String::new();
            let bytes = await_operation(operation, stage, self.stdout.read_line(&mut line))
                .await
                .map_err(|error| runtime_error(error, stage, "read from"))?;
            if bytes == 0 {
                return Err(SfumatoError::provider(
                    ErrorClass::Unavailable,
                    "Codex App Server closed its output unexpectedly",
                )
                .at_stage(stage));
            }
            if line.trim().is_empty() {
                continue;
            }
            return serde_json::from_str(&line).map_err(|error| {
                protocol_error(
                    stage,
                    format_args!("Invalid App Server JSON-RPC message: {error}"),
                )
            });
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelListResponse {
    data: Vec<CodexModel>,
    next_cursor: Option<String>,
}

fn dynamic_tools(tools: &[ToolDefinition]) -> Vec<Value> {
    tools
        .iter()
        .map(|tool| {
            json!({
                "type": "function",
                "name": tool.function.name,
                "description": tool.function.description,
                "inputSchema": tool.function.parameters,
            })
        })
        .collect()
}

/// Refused tool calls tolerated after the budget is reported spent.
///
/// This transport cannot take a model's tools away. The HTTP connectors send the
/// exhaustion notice and then simply omit the tool list, so the model physically
/// cannot ask again; here the tools are registered for the whole turn and the only
/// available answer is to refuse each further call. Failing on the first refused
/// call punished a model for work it had already queued before it could read the
/// notice, and killed the whole generation for it.
const TOOL_REFUSALS_BEFORE_FAILING: usize = 8;

/// The call number past which the model is ignoring the notice rather than
/// draining a queue.
fn tool_call_ceiling(max_tool_rounds: usize) -> usize {
    max_tool_rounds.saturating_add(TOOL_REFUSALS_BEFORE_FAILING)
}

/// Builds one turn's input items, attaching any images the request carries.
///
/// The protocol takes a local path and reads the file itself, so snapshots travel
/// as `localImage` with no encoding. Each label precedes its own image because the
/// protocol carries no caption field, and a bare run of frames leaves the model
/// unable to say which one is wrong.
fn turn_input(
    prompt: &str,
    images: &[ImageAttachment],
    model: &CodexModel,
    stage: OperationStage,
) -> SfumatoResult<Value> {
    let mut input = vec![json!({ "type": "text", "text": prompt })];
    if images.is_empty() {
        return Ok(Value::Array(input));
    }
    // Refused on the model's own declaration rather than on the connector kind.
    // An older catalog omits the field entirely, which the protocol says to read
    // as accepting both, so an absent list is not a refusal.
    if !model.input_modalities.is_empty()
        && !model
            .input_modalities
            .iter()
            .any(|modality| modality.eq_ignore_ascii_case("image"))
    {
        return Err(SfumatoError::config(format_args!(
            "Codex model '{}' accepts {} only; select an image-capable model for this step",
            model.model,
            model.input_modalities.join(", ")
        ))
        .at_stage(stage));
    }
    for image in images {
        input.push(json!({ "type": "text", "text": image.label }));
        input.push(json!({ "type": "localImage", "path": image.path }));
    }
    Ok(Value::Array(input))
}

fn resolve_model<'a>(
    models: &'a [CodexModel],
    requested: &str,
    stage: OperationStage,
) -> SfumatoResult<&'a CodexModel> {
    let model = if requested == "default" {
        models.iter().find(|model| model.is_default)
    } else {
        models
            .iter()
            .find(|model| model.id == requested || model.model == requested)
    };
    model.ok_or_else(|| {
        let available = models
            .iter()
            .filter(|model| !model.hidden)
            .map(|model| model.model.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        SfumatoError::config(format_args!(
            "Codex model '{requested}' is not available. Available models: {available}"
        ))
        .at_stage(stage)
    })
}

fn completed_agent_text(message: &Value) -> Option<&str> {
    let item = message.pointer("/params/item")?;
    (item.get("type")?.as_str()? == "agentMessage")
        .then(|| item.get("text").and_then(Value::as_str))
        .flatten()
}

/// The file the Codex image tool wrote, when this item reports one.
///
/// Measured against the protocol: a completed `imageGeneration` item carries the
/// artifact's absolute path. A completed item without one means the tool ran and
/// produced nothing usable, which is the same outcome as never running.
fn generated_image_path(message: &Value) -> Option<&str> {
    let item = message.pointer("/params/item")?;
    (item.get("type")?.as_str()? == "imageGeneration")
        .then(|| item.get("savedPath").and_then(Value::as_str))
        .flatten()
}

fn final_agent_text_from_turn(params: &Value) -> Option<&str> {
    params
        .pointer("/turn/items")?
        .as_array()?
        .iter()
        .rev()
        .find_map(|item| {
            (item.get("type").and_then(Value::as_str) == Some("agentMessage"))
                .then(|| item.get("text").and_then(Value::as_str))
                .flatten()
        })
}

fn turn_error(params: &Value, status: &str, stage: OperationStage) -> SfumatoError {
    let message = params
        .pointer("/turn/error/message")
        .and_then(Value::as_str)
        .unwrap_or("Codex turn did not complete successfully");
    let info = params.pointer("/turn/error/codexErrorInfo");
    let class = match info.and_then(Value::as_str) {
        Some("contextWindowExceeded") => ErrorClass::ContextLimit,
        Some("usageLimitExceeded" | "unauthorized" | "badRequest") => ErrorClass::Permanent,
        Some("serverOverloaded" | "internalServerError") => ErrorClass::Retry,
        _ if status == "interrupted" => ErrorClass::Cancelled,
        _ => ErrorClass::Unavailable,
    };
    SfumatoError::provider(class, message).at_stage(stage)
}

fn json_rpc_error(method: &str, error: &Value, stage: OperationStage) -> SfumatoError {
    let code = error
        .get("code")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("unknown JSON-RPC error");
    SfumatoError::provider(
        ErrorClass::Permanent,
        format_args!("Codex App Server {method} failed ({code}): {message}"),
    )
    .at_stage(stage)
}

fn protocol_error(stage: OperationStage, message: impl std::fmt::Display) -> SfumatoError {
    SfumatoError::provider(ErrorClass::InvalidOutput, message).at_stage(stage)
}

fn runtime_error(error: anyhow::Error, stage: OperationStage, action: &str) -> SfumatoError {
    if let Some(error) = error.downcast_ref::<SfumatoError>() {
        return error.clone();
    }
    SfumatoError::provider(
        ErrorClass::Unavailable,
        format_args!("Could not {action} Codex App Server: {error}"),
    )
    .at_stage(stage)
}

fn codex_process_error(
    error: std::io::Error,
    executable: &Path,
    stage: OperationStage,
) -> SfumatoError {
    let message = if error.kind() == std::io::ErrorKind::NotFound {
        format!(
            "Codex executable '{}' was not found. Install Codex and run `codex login`.",
            executable.display()
        )
    } else {
        format!("Could not start Codex App Server: {error}")
    };
    SfumatoError::provider(ErrorClass::Unavailable, message).at_stage(stage)
}

#[cfg(test)]
#[path = "../tests/unit/codex_app_server.rs"]
mod tests;
