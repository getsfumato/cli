//! Filesystem and generated-image tools exposed to text models.

use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sfumato_core::{
    errors::OperationStage,
    operation::OperationContext,
    prompts::{PromptCatalog, PromptId, PromptProvenance, PromptRenderRequest, PromptVariables},
    providers::{
        ImageGenerationRequest, ToolDefinition, ToolExecutionRequest, ToolExecutor,
        ToolFunctionDefinition,
    },
    tools::{GenerationToolFactory, GenerationToolsRequest, ImageToolConfig, ToolSet},
};
use sha2::{Digest, Sha256};

const MAX_DIRECTORY_ENTRIES: usize = 200;
const MAX_FILE_BYTES: u64 = 128 * 1024;
const MAX_GENERATED_IMAGE_BYTES: usize = 64 * 1024 * 1024;

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
    ) -> Result<String> {
        operation.checkpoint(stage)?;
        match request.name.as_str() {
            "sfumato_list_directory" => {
                let path = string_arg(&request.arguments, "path")?;
                self.list_directory(&path)
            }
            "sfumato_read_file" => {
                let path = string_arg(&request.arguments, "path")?;
                self.read_file(&path)
            }
            _ => bail!("Unknown Sfumato tool '{}'", request.name),
        }
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
            "markdown_path": format!("images/{filename}"),
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
}

#[async_trait]
impl ToolExecutor for GenerationToolExecutor {
    async fn execute(
        &self,
        request: ToolExecutionRequest,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> Result<String> {
        if request.name == "sfumato_image_gen" {
            return self
                .image
                .as_ref()
                .context("No image model is configured for this project")?
                .execute(&request.arguments, operation, stage)
                .await;
        }
        self.filesystem.execute(request, operation, stage).await
    }
}

impl GenerationToolFactory for FilesystemGenerationToolFactory {
    fn create(&self, request: GenerationToolsRequest) -> Result<ToolSet> {
        let GenerationToolsRequest {
            project_root,
            sources,
            image,
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
        Ok(ToolSet {
            definitions,
            executor: Arc::new(GenerationToolExecutor {
                filesystem: FilesystemToolExecutor::new(roots)?,
                image,
            }),
            artifacts,
            prompts,
        })
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
