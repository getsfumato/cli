//! Filesystem and generated-image tools exposed to text models.

use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sfumato_core::{
    errors::{ErrorClass, OperationStage, SfumatoError, SfumatoResult},
    operation::OperationContext,
    prompts::{PromptCatalog, PromptId, PromptProvenance, PromptRenderRequest, PromptVariables},
    providers::{
        ImageGenerationRequest, SpeechGenerationRequest, ToolDefinition, ToolExecutionRequest,
        ToolExecutor, ToolFunctionDefinition, VideoGenerationRequest,
    },
    resources::narration::{audio_extension, audio_media_type},
    tools::{
        AudioToolConfig, GenerationToolFactory, GenerationToolsRequest, ImageToolConfig, ToolSet,
        VideoToolConfig,
    },
};
use sha2::{Digest, Sha256};

const MAX_DIRECTORY_ENTRIES: usize = 200;
const MAX_FILE_BYTES: u64 = 128 * 1024;
const MAX_GENERATED_IMAGE_BYTES: usize = 64 * 1024 * 1024;
const MAX_GENERATED_VIDEO_BYTES: usize = 512 * 1024 * 1024;
const MAX_GENERATED_AUDIO_BYTES: usize = 64 * 1024 * 1024;
/// Longest passage one tool call may speak, so a runaway request is refused
/// before it is billed rather than after.
const MAX_SPOKEN_CHARACTERS: usize = 5_000;

/// Builds filesystem-backed tools for one generation operation.
#[derive(Clone, Copy, Debug, Default)]
pub struct FilesystemGenerationToolFactory;

#[derive(Clone, Debug)]
struct FilesystemToolExecutor {
    roots: Vec<PathBuf>,
    max_file_bytes: u64,
}

impl FilesystemToolExecutor {
    fn new(roots: Vec<PathBuf>) -> Result<Self> {
        let mut canonical_roots = Vec::new();
        for root in roots {
            let root = root
                .canonicalize()
                .with_context(|| format!("Could not resolve tool root {}", root.display()))?;
            if !canonical_roots.contains(&root) {
                canonical_roots.push(root);
            }
        }
        if canonical_roots.is_empty() {
            bail!("Filesystem tools need at least one readable root");
        }
        Ok(Self {
            roots: canonical_roots,
            max_file_bytes: MAX_FILE_BYTES,
        })
    }

    fn resolve_allowed_path(&self, requested: &str) -> Result<PathBuf> {
        if requested.trim().is_empty() {
            bail!("Tool path cannot be empty");
        }
        let path = PathBuf::from(requested);
        let candidate = if path.is_absolute() {
            path
        } else {
            self.roots[0].join(path)
        };
        let canonical = candidate
            .canonicalize()
            .with_context(|| format!("Could not resolve tool path {}", candidate.display()))?;
        if !self.roots.iter().any(|root| canonical.starts_with(root)) {
            bail!(
                "Refusing to read {} because it is outside the allowed generation roots",
                canonical.display()
            );
        }
        Ok(canonical)
    }

    fn list_directory(&self, path: &str) -> Result<String> {
        let path = self.resolve_allowed_path(path)?;
        if !path.is_dir() {
            bail!("{} is not a directory", path.display());
        }
        let mut entries = fs::read_dir(&path)
            .with_context(|| format!("Could not read directory {}", path.display()))?
            .map(|entry| {
                let entry = entry?;
                let metadata = entry.metadata()?;
                Ok(DirectoryEntry {
                    name: entry.file_name().to_string_lossy().to_string(),
                    path: entry.path().display().to_string(),
                    kind: if metadata.is_dir() {
                        "directory"
                    } else {
                        "file"
                    }
                    .to_string(),
                    bytes: metadata.is_file().then_some(metadata.len()),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        entries.sort_by(|left, right| left.path.cmp(&right.path));
        let truncated = entries.len() > MAX_DIRECTORY_ENTRIES;
        entries.truncate(MAX_DIRECTORY_ENTRIES);
        serde_json::to_string(&json!({
            "path": path,
            "entries": entries,
            "truncated": truncated,
        }))
        .context("Could not serialize directory listing")
    }

    fn read_file(&self, path: &str) -> Result<String> {
        let path = self.resolve_allowed_path(path)?;
        if !path.is_file() {
            bail!("{} is not a file", path.display());
        }
        let metadata = path
            .metadata()
            .with_context(|| format!("Could not inspect {}", path.display()))?;
        if metadata.len() > self.max_file_bytes {
            bail!(
                "{} is {} bytes; the current read limit is {} bytes",
                path.display(),
                metadata.len(),
                self.max_file_bytes
            );
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Could not read {}", path.display()))?;
        serde_json::to_string(&json!({
            "path": path,
            "content": content,
        }))
        .context("Could not serialize file content")
    }
}

#[async_trait]
impl ToolExecutor for FilesystemToolExecutor {
    async fn execute(
        &self,
        request: ToolExecutionRequest,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<String> {
        operation.checkpoint(stage)?;
        let result: Result<String> = (|| match request.name.as_str() {
            "sfumato_list_directory" => {
                let path = string_arg(&request.arguments, "path")?;
                self.list_directory(&path)
            }
            "sfumato_read_file" => {
                let path = string_arg(&request.arguments, "path")?;
                self.read_file(&path)
            }
            _ => bail!("Unknown Sfumato tool '{}'", request.name),
        })();
        result.map_err(|error| tool_error(error, stage))
    }
}

struct ImageGenerationTool {
    config: ImageToolConfig,
    prompt_catalog: Arc<dyn PromptCatalog>,
    artifacts: Arc<Mutex<Vec<PathBuf>>>,
    prompts: Arc<Mutex<Vec<PromptProvenance>>>,
}

#[derive(Serialize)]
struct ImagePromptContext<'a> {
    requested_prompt: &'a str,
    theme_name: &'a str,
    theme_colors: String,
    theme_fonts: String,
    project_instructions: &'a str,
}

impl ImageGenerationTool {
    async fn execute(
        &self,
        arguments: &Value,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> Result<String> {
        operation.checkpoint(stage)?;
        let prompt = string_arg(arguments, "prompt")?;
        let alt_text = optional_string_arg(arguments, "alt_text")?
            .unwrap_or_else(|| "Generated educational illustration".to_string());
        let context = ImagePromptContext {
            requested_prompt: &prompt,
            theme_name: &self.config.theme.manifest.name,
            theme_colors: format_tokens(&self.config.theme.manifest.tokens.colors),
            theme_fonts: format_tokens(&self.config.theme.manifest.tokens.fonts),
            project_instructions: self
                .config
                .project_instructions
                .as_deref()
                .unwrap_or_default(),
        };
        let rendered = self.prompt_catalog.render(PromptRenderRequest {
            id: PromptId::ImageGenerationUser,
            variables: PromptVariables::from_serializable(&context)?,
        })?;
        self.prompts
            .lock()
            .map_err(|_| anyhow::anyhow!("Generated image prompt registry is unavailable"))?
            .push(rendered.provenance);
        let response = self
            .config
            .provider
            .generate_image(
                ImageGenerationRequest {
                    prompt: rendered.text,
                },
                operation,
                stage,
            )
            .await?;
        operation.checkpoint(stage)?;
        if response.bytes.len() > MAX_GENERATED_IMAGE_BYTES {
            bail!(
                "Generated image is {} bytes; the current limit is {} bytes",
                response.bytes.len(),
                MAX_GENERATED_IMAGE_BYTES
            );
        }
        let extension = image_extension(&response.media_type)?;
        let content_hash = format!("{:x}", Sha256::digest(&response.bytes));
        let filename = format!("image-{}.{extension}", &content_hash[..24]);
        let path = self.config.output_dir.join(&filename);
        fs::create_dir_all(&self.config.output_dir)
            .with_context(|| format!("Could not create {}", self.config.output_dir.display()))?;
        fs::write(&path, response.bytes)
            .with_context(|| format!("Could not write generated image {}", path.display()))?;
        self.artifacts
            .lock()
            .map_err(|_| anyhow::anyhow!("Generated image artifact registry is unavailable"))?
            .push(path.clone());
        Ok(json!({
            "path": path,
            "markdown_path": format!("{}/{filename}", self.config.reference_prefix),
            "alt_text": alt_text,
            "media_type": response.media_type,
            "model_profile": self.config.profile_name,
        })
        .to_string())
    }
}

struct GenerationToolExecutor {
    filesystem: FilesystemToolExecutor,
    image: Option<ImageGenerationTool>,
    video: Option<VideoGenerationTool>,
    audio: Option<AudioGenerationTool>,
}

/// Speaks text and returns the audio plus the word timings behind it.
///
/// The timings are written beside the audio rather than returned inline: a
/// paragraph produces hundreds of them, which would crowd out the rest of the
/// model's context for a file it can read when it actually needs to caption.
struct AudioGenerationTool {
    config: AudioToolConfig,
    artifacts: Arc<Mutex<Vec<PathBuf>>>,
}

impl AudioGenerationTool {
    async fn execute(
        &self,
        arguments: &Value,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> Result<String> {
        operation.checkpoint(stage)?;
        let text = string_arg(arguments, "text")?;
        if text.trim().is_empty() {
            bail!("sfumato_audio_gen needs text to speak");
        }
        if text.chars().count() > MAX_SPOKEN_CHARACTERS {
            bail!(
                "Requested narration is {} characters; the per-call limit is {MAX_SPOKEN_CHARACTERS}",
                text.chars().count()
            );
        }
        let response = self
            .config
            .provider
            .generate_speech(
                SpeechGenerationRequest {
                    text: text.clone(),
                    voice: optional_string_arg(arguments, "voice")?
                        .or_else(|| self.config.options.voice.clone()),
                    previous_text: None,
                    next_text: None,
                },
                operation,
                stage,
            )
            .await?;
        operation.checkpoint(stage)?;
        if response.bytes.len() > MAX_GENERATED_AUDIO_BYTES {
            bail!(
                "Generated audio is {} bytes; the current limit is {MAX_GENERATED_AUDIO_BYTES} bytes",
                response.bytes.len()
            );
        }
        let format = self.config.options.output_format.as_deref();
        let extension = audio_extension(format);
        let content_hash = format!("{:x}", Sha256::digest(&response.bytes));
        let stem = format!("audio-{}", &content_hash[..24]);
        let filename = format!("{stem}.{extension}");
        fs::create_dir_all(&self.config.output_dir)
            .with_context(|| format!("Could not create {}", self.config.output_dir.display()))?;
        let path = self.config.output_dir.join(&filename);
        fs::write(&path, &response.bytes)
            .with_context(|| format!("Could not write generated audio {}", path.display()))?;
        self.artifacts
            .lock()
            .map_err(|_| anyhow::anyhow!("Generated audio artifact registry is unavailable"))?
            .push(path.clone());
        let mut timings_reference = None;
        if !response.words.is_empty() {
            let timings_name = format!("{stem}.words.json");
            let timings_path = self.config.output_dir.join(&timings_name);
            fs::write(&timings_path, serde_json::to_vec_pretty(&response.words)?)?;
            self.artifacts
                .lock()
                .map_err(|_| anyhow::anyhow!("Generated audio artifact registry is unavailable"))?
                .push(timings_path);
            timings_reference = Some(format!("{}/{timings_name}", self.config.reference_prefix));
        }
        Ok(json!({
            "path": path,
            "html_path": format!("{}/{filename}", self.config.reference_prefix),
            "word_timings_path": timings_reference,
            "duration_seconds": response.duration_seconds,
            "media_type": audio_media_type(format),
            "model_profile": self.config.profile_name,
        })
        .to_string())
    }
}

struct VideoGenerationTool {
    config: VideoToolConfig,
    prompt_catalog: Arc<dyn PromptCatalog>,
    artifacts: Arc<Mutex<Vec<PathBuf>>>,
    prompts: Arc<Mutex<Vec<PromptProvenance>>>,
    used: AtomicBool,
}

#[derive(Serialize)]
struct VideoPromptContext<'a> {
    requested_prompt: &'a str,
    accessible_description: &'a str,
    theme_name: &'a str,
    theme_colors: String,
    theme_fonts: String,
    project_instructions: &'a str,
}

impl VideoGenerationTool {
    async fn execute(
        &self,
        arguments: &Value,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> Result<String> {
        if self.used.swap(true, Ordering::AcqRel) {
            bail!("sfumato_video_gen can be called at most once per page generation");
        }
        let requested_prompt = string_arg(arguments, "prompt")?;
        let accessible_description = string_arg(arguments, "accessible_description")?;
        let context = VideoPromptContext {
            requested_prompt: &requested_prompt,
            accessible_description: &accessible_description,
            theme_name: &self.config.theme.manifest.name,
            theme_colors: format_tokens(&self.config.theme.manifest.tokens.colors),
            theme_fonts: format_tokens(&self.config.theme.manifest.tokens.fonts),
            project_instructions: self
                .config
                .project_instructions
                .as_deref()
                .unwrap_or_default(),
        };
        let rendered = self.prompt_catalog.render(PromptRenderRequest {
            id: PromptId::VideoGenerationUser,
            variables: PromptVariables::from_serializable(&context)?,
        })?;
        self.prompts
            .lock()
            .map_err(|_| anyhow::anyhow!("Generated video prompt registry is unavailable"))?
            .push(rendered.provenance);
        let options = &self.config.options;
        let response = self
            .config
            .provider
            .generate_video(
                VideoGenerationRequest {
                    prompt: rendered.text,
                    duration_seconds: options.duration_seconds.unwrap_or(5),
                    resolution: options.resolution.clone().unwrap_or_else(|| "720p".into()),
                    aspect_ratio: options
                        .aspect_ratio
                        .clone()
                        .unwrap_or_else(|| "16:9".into()),
                    generate_audio: options
                        .audio
                        .map(|audio| !matches!(audio, sfumato_core::config::VideoAudioMode::Off)),
                    seed: options.seed,
                    references: self.config.references.clone(),
                },
                operation,
                stage,
            )
            .await?;
        if response.bytes.len() > MAX_GENERATED_VIDEO_BYTES {
            bail!(
                "Generated video exceeds the {} byte limit",
                MAX_GENERATED_VIDEO_BYTES
            );
        }
        if response.media_type != "video/mp4" && response.media_type != "application/octet-stream" {
            bail!(
                "Unsupported generated video media type '{}'",
                response.media_type
            );
        }
        let content_hash = format!("{:x}", Sha256::digest(&response.bytes));
        let filename = format!("video-{}.mp4", &content_hash[..24]);
        let path = self.config.output_dir.join(&filename);
        fs::create_dir_all(&self.config.output_dir)?;
        fs::write(&path, response.bytes)?;
        self.artifacts
            .lock()
            .map_err(|_| anyhow::anyhow!("Generated video artifact registry is unavailable"))?
            .push(path.clone());
        Ok(json!({
            "path": path,
            "html_path": format!("{}/{filename}", self.config.reference_prefix),
            "accessible_description": accessible_description,
            "media_type": "video/mp4",
            "model_profile": self.config.profile_name,
        })
        .to_string())
    }
}

#[async_trait]
impl ToolExecutor for GenerationToolExecutor {
    async fn execute(
        &self,
        request: ToolExecutionRequest,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<String> {
        if request.name == "sfumato_image_gen" {
            return self
                .image
                .as_ref()
                .ok_or_else(|| {
                    SfumatoError::tool(
                        ErrorClass::Permanent,
                        "No image model is configured for this project",
                    )
                    .at_stage(stage)
                })?
                .execute(&request.arguments, operation, stage)
                .await
                .map_err(|error| tool_error(error, stage));
        }
        if request.name == "sfumato_audio_gen" {
            return self
                .audio
                .as_ref()
                .ok_or_else(|| {
                    SfumatoError::tool(
                        ErrorClass::Permanent,
                        "No speech model is configured for this project",
                    )
                    .at_stage(stage)
                })?
                .execute(&request.arguments, operation, stage)
                .await
                .map_err(|error| tool_error(error, stage));
        }
        if request.name == "sfumato_video_gen" {
            return self
                .video
                .as_ref()
                .ok_or_else(|| {
                    SfumatoError::tool(
                        ErrorClass::Permanent,
                        "No video model is configured for this page",
                    )
                    .at_stage(stage)
                })?
                .execute(&request.arguments, operation, stage)
                .await
                .map_err(|error| tool_error(error, stage));
        }
        self.filesystem.execute(request, operation, stage).await
    }
}

fn tool_error(error: anyhow::Error, stage: OperationStage) -> SfumatoError {
    if let Some(error) = error.downcast_ref::<SfumatoError>() {
        let mut error = error.clone();
        if error.stage.is_none() {
            error.stage = Some(stage);
        }
        return error;
    }
    SfumatoError::tool(ErrorClass::Permanent, format_args!("{error:#}")).at_stage(stage)
}

impl GenerationToolFactory for FilesystemGenerationToolFactory {
    fn create(&self, request: GenerationToolsRequest) -> SfumatoResult<ToolSet> {
        let result: Result<ToolSet> = (|| {
            let GenerationToolsRequest {
                project_root,
                sources,
                image,
                video,
                audio,
                prompt_catalog,
            } = request;
            let mut roots = vec![project_root];
            for source in sources {
                if source.is_file() {
                    if let Some(parent) = source.parent() {
                        roots.push(parent.to_path_buf());
                    }
                } else {
                    roots.push(source);
                }
            }

            let artifacts = Arc::new(Mutex::new(Vec::new()));
            let rendered_descriptions = prompt_catalog.render(PromptRenderRequest {
                id: PromptId::ToolsGenerationDescriptions,
                variables: PromptVariables::default(),
            })?;
            let descriptions: ToolDescriptions = serde_json::from_str(&rendered_descriptions.text)
                .context("Generation tool description prompt must render a JSON object")?;
            let prompts = Arc::new(Mutex::new(vec![rendered_descriptions.provenance]));
            let mut definitions = vec![
                list_directory_tool(&descriptions),
                read_file_tool(&descriptions),
            ];
            let image = image.map(|config| {
                definitions.push(image_generation_tool(&descriptions));
                ImageGenerationTool {
                    config,
                    prompt_catalog: prompt_catalog.clone(),
                    artifacts: artifacts.clone(),
                    prompts: prompts.clone(),
                }
            });
            let video = video.map(|config| {
                definitions.push(video_generation_tool(&descriptions));
                VideoGenerationTool {
                    config,
                    prompt_catalog: prompt_catalog.clone(),
                    artifacts: artifacts.clone(),
                    prompts: prompts.clone(),
                    used: AtomicBool::new(false),
                }
            });
            let audio = audio.map(|config| {
                definitions.push(audio_generation_tool(&descriptions));
                AudioGenerationTool {
                    config,
                    artifacts: artifacts.clone(),
                }
            });
            Ok(ToolSet {
                definitions,
                executor: Arc::new(GenerationToolExecutor {
                    filesystem: FilesystemToolExecutor::new(roots)?,
                    image,
                    video,
                    audio,
                }),
                artifacts,
                prompts,
            })
        })();
        result.map_err(|error| SfumatoError::tool(ErrorClass::Permanent, format_args!("{error:#}")))
    }
}

#[derive(Serialize)]
struct DirectoryEntry {
    name: String,
    path: String,
    kind: String,
    bytes: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolDescriptions {
    list_directory: String,
    list_directory_path: String,
    read_file: String,
    read_file_path: String,
    image_generation: String,
    image_prompt: String,
    image_alt_text: String,
    video_generation: String,
    video_prompt: String,
    video_accessible_description: String,
    audio_generation: String,
    audio_text: String,
    audio_voice: String,
}

fn list_directory_tool(descriptions: &ToolDescriptions) -> ToolDefinition {
    ToolDefinition {
        kind: "function".to_string(),
        function: ToolFunctionDefinition {
            name: "sfumato_list_directory".to_string(),
            description: descriptions.list_directory.clone(),
            parameters: json!({
                "type": "object",
                "properties": { "path": {
                    "type": "string",
                    "description": descriptions.list_directory_path
                }},
                "required": ["path"]
            }),
        },
    }
}

fn read_file_tool(descriptions: &ToolDescriptions) -> ToolDefinition {
    ToolDefinition {
        kind: "function".to_string(),
        function: ToolFunctionDefinition {
            name: "sfumato_read_file".to_string(),
            description: descriptions.read_file.clone(),
            parameters: json!({
                "type": "object",
                "properties": { "path": {
                    "type": "string",
                    "description": descriptions.read_file_path
                }},
                "required": ["path"]
            }),
        },
    }
}

fn image_generation_tool(descriptions: &ToolDescriptions) -> ToolDefinition {
    ToolDefinition {
        kind: "function".to_string(),
        function: ToolFunctionDefinition {
            name: "sfumato_image_gen".to_string(),
            description: descriptions.image_generation.clone(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "prompt": {
                        "type": "string",
                        "description": descriptions.image_prompt
                    },
                    "alt_text": {
                        "type": "string",
                        "description": descriptions.image_alt_text
                    }
                },
                "required": ["prompt"]
            }),
        },
    }
}

fn video_generation_tool(descriptions: &ToolDescriptions) -> ToolDefinition {
    ToolDefinition {
        kind: "function".to_string(),
        function: ToolFunctionDefinition {
            name: "sfumato_video_gen".to_string(),
            description: descriptions.video_generation.clone(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "prompt": { "type": "string", "description": descriptions.video_prompt },
                    "accessible_description": {
                        "type": "string",
                        "description": descriptions.video_accessible_description
                    }
                },
                "required": ["prompt", "accessible_description"]
            }),
        },
    }
}

fn audio_generation_tool(descriptions: &ToolDescriptions) -> ToolDefinition {
    ToolDefinition {
        kind: "function".to_string(),
        function: ToolFunctionDefinition {
            name: "sfumato_audio_gen".to_string(),
            description: descriptions.audio_generation.clone(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": descriptions.audio_text },
                    "voice": { "type": "string", "description": descriptions.audio_voice }
                },
                "required": ["text"]
            }),
        },
    }
}

fn string_arg(arguments: &Value, key: &str) -> Result<String> {
    let arguments = normalized_arguments(arguments)?;
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .with_context(|| format!("Tool argument '{key}' must be a string"))
}

fn optional_string_arg(arguments: &Value, key: &str) -> Result<Option<String>> {
    let arguments = normalized_arguments(arguments)?;
    match arguments.get(key) {
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => bail!("Tool argument '{key}' must be a string"),
        None => Ok(None),
    }
}

fn normalized_arguments(arguments: &Value) -> Result<Value> {
    match arguments {
        Value::String(raw) => serde_json::from_str(raw)
            .with_context(|| format!("Tool arguments were not valid JSON: {raw}")),
        other => Ok(other.clone()),
    }
}

fn image_extension(media_type: &str) -> Result<&'static str> {
    match media_type {
        "image/png" => Ok("png"),
        "image/jpeg" | "image/jpg" => Ok("jpg"),
        "image/webp" => Ok("webp"),
        other => bail!("Unsupported generated image media type '{other}'"),
    }
}

fn format_tokens(tokens: &std::collections::BTreeMap<String, String>) -> String {
    if tokens.is_empty() {
        return "unspecified".to_string();
    }
    tokens
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
#[path = "../tests/unit/tools.rs"]
mod tests;
