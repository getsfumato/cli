use std::{
    fs,
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Serialize;
use serde_json::{Value, json};
use slug::slugify;

use crate::{
    providers::{
        ImageGenerationProvider, ImageGenerationRequest, ToolDefinition, ToolExecutionRequest,
        ToolExecutor, ToolFunctionDefinition,
    },
    themes::ThemePackage,
};

const MAX_DIRECTORY_ENTRIES: usize = 200;
const MAX_FILE_BYTES: u64 = 128 * 1024;
const MAX_GENERATED_IMAGE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone)]
pub struct ToolSet {
    pub definitions: Vec<ToolDefinition>,
    pub executor: Arc<dyn ToolExecutor>,
    artifacts: Arc<Mutex<Vec<PathBuf>>>,
}

impl ToolSet {
    pub fn generated_artifacts(&self) -> Result<Vec<PathBuf>> {
        self.artifacts
            .lock()
            .map(|artifacts| artifacts.clone())
            .map_err(|_| anyhow::anyhow!("Generated image artifact registry is unavailable"))
    }
}

#[derive(Clone, Debug)]
pub struct FilesystemToolExecutor {
    roots: Vec<PathBuf>,
    max_file_bytes: u64,
}

impl FilesystemToolExecutor {
    pub fn new(roots: Vec<PathBuf>) -> Result<Self> {
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
    async fn execute(&self, request: ToolExecutionRequest) -> Result<String> {
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

pub struct ImageToolConfig {
    pub provider: Arc<dyn ImageGenerationProvider>,
    pub profile_name: String,
    pub output_dir: PathBuf,
    pub theme: ThemePackage,
    pub project_instructions: Option<String>,
}

struct ImageGenerationTool {
    provider: Arc<dyn ImageGenerationProvider>,
    profile_name: String,
    output_dir: PathBuf,
    theme_prompt: String,
    project_instructions: Option<String>,
    artifacts: Arc<Mutex<Vec<PathBuf>>>,
}

impl ImageGenerationTool {
    async fn execute(&self, arguments: &Value) -> Result<String> {
        let prompt = string_arg(arguments, "prompt")?;
        let alt_text = optional_string_arg(arguments, "alt_text")?
            .unwrap_or_else(|| "Generated educational illustration".to_string());
        let project_instructions = self
            .project_instructions
            .as_deref()
            .map(|instructions| {
                format!(
                    "\n\nProject instructions from SFUMATO.md:\n<sfumato_project_instructions>\n{instructions}\n</sfumato_project_instructions>"
                )
            })
            .unwrap_or_default();
        let themed_prompt = format!(
            "{prompt}\n\nSfumato project visual direction:\n{}{project_instructions}\nCreate one clear educational visual suitable for a presentation slide. Keep labels concise and do not add a decorative frame.",
            self.theme_prompt
        );
        let response = self
            .provider
            .generate_image(ImageGenerationRequest {
                prompt: themed_prompt,
            })
            .await?;
        if response.bytes.len() > MAX_GENERATED_IMAGE_BYTES {
            bail!(
                "Generated image is {} bytes; the current limit is {} bytes",
                response.bytes.len(),
                MAX_GENERATED_IMAGE_BYTES
            );
        }
        let extension = image_extension(&response.media_type)?;
        let mut hasher = DefaultHasher::new();
        response.bytes.hash(&mut hasher);
        let content_hash = hasher.finish();
        let prompt_slug = slugify(&prompt);
        let prompt_slug = prompt_slug.chars().take(48).collect::<String>();
        let prompt_slug = if prompt_slug.is_empty() {
            "illustration"
        } else {
            &prompt_slug
        };
        let filename = format!("generated-{prompt_slug}-{content_hash:016x}.{extension}");
        let path = self.output_dir.join(&filename);
        fs::create_dir_all(&self.output_dir)
            .with_context(|| format!("Could not create {}", self.output_dir.display()))?;
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
            "model_profile": self.profile_name,
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
    async fn execute(&self, request: ToolExecutionRequest) -> Result<String> {
        if request.name == "sfumato_image_gen" {
            return self
                .image
                .as_ref()
                .context("No image model is configured for this project")?
                .execute(&request.arguments)
                .await;
        }
        self.filesystem.execute(request).await
    }
}

#[derive(Serialize)]
struct DirectoryEntry {
    name: String,
    path: String,
    kind: String,
    bytes: Option<u64>,
}

pub fn generation_tools(
    project_root: &Path,
    sources: &[PathBuf],
    image: Option<ImageToolConfig>,
) -> Result<ToolSet> {
    let mut roots = vec![project_root.to_path_buf()];
    for source in sources {
        if source.is_file() {
            if let Some(parent) = source.parent() {
                roots.push(parent.to_path_buf());
            }
        } else {
            roots.push(source.to_path_buf());
        }
    }

    let artifacts = Arc::new(Mutex::new(Vec::new()));
    let mut definitions = vec![list_directory_tool(), read_file_tool()];
    let image = image.map(|image| {
        definitions.push(image_generation_tool());
        ImageGenerationTool {
            provider: image.provider,
            profile_name: image.profile_name,
            output_dir: image.output_dir,
            theme_prompt: theme_prompt(&image.theme),
            project_instructions: image.project_instructions,
            artifacts: artifacts.clone(),
        }
    });
    Ok(ToolSet {
        definitions,
        executor: Arc::new(GenerationToolExecutor {
            filesystem: FilesystemToolExecutor::new(roots)?,
            image,
        }),
        artifacts,
    })
}

fn list_directory_tool() -> ToolDefinition {
    ToolDefinition {
        kind: "function".to_string(),
        function: ToolFunctionDefinition {
            name: "sfumato_list_directory".to_string(),
            description:
                "List files and directories inside the allowed Sfumato project/source roots."
                    .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Directory path to list. Must be inside an allowed Sfumato project/source root."
                    }
                },
                "required": ["path"]
            }),
        },
    }
}

fn read_file_tool() -> ToolDefinition {
    ToolDefinition {
        kind: "function".to_string(),
        function: ToolFunctionDefinition {
            name: "sfumato_read_file".to_string(),
            description: "Read a UTF-8 text file inside the allowed Sfumato project/source roots."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path to read. Must be inside an allowed Sfumato project/source root."
                    }
                },
                "required": ["path"]
            }),
        },
    }
}

fn image_generation_tool() -> ToolDefinition {
    ToolDefinition {
        kind: "function".to_string(),
        function: ToolFunctionDefinition {
            name: "sfumato_image_gen".to_string(),
            description: "Generate a themed educational image and save it beside the slide deck. Use the returned markdown_path in a Markdown image link."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "prompt": {
                        "type": "string",
                        "description": "A concrete visual description including subject, composition, relevant labels, and learning purpose. Do not describe the project theme; Sfumato injects it."
                    },
                    "alt_text": {
                        "type": "string",
                        "description": "Concise accessible description of the requested image."
                    }
                },
                "required": ["prompt"]
            }),
        },
    }
}

fn string_arg(arguments: &Value, key: &str) -> Result<String> {
    let arguments = match arguments {
        Value::String(raw) => serde_json::from_str(raw)
            .with_context(|| format!("Tool arguments were not valid JSON: {raw}"))?,
        other => other.clone(),
    };
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

fn theme_prompt(theme: &ThemePackage) -> String {
    let colors = theme
        .manifest
        .tokens
        .colors
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join(", ");
    let fonts = theme
        .manifest
        .tokens
        .fonts
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "Theme: {}. Semantic colors: {}. Typography: {}.",
        theme.manifest.name,
        if colors.is_empty() {
            "unspecified"
        } else {
            &colors
        },
        if fonts.is_empty() {
            "unspecified"
        } else {
            &fonts
        },
    )
}

#[cfg(test)]
#[path = "../tests/unit/tools.rs"]
mod tests;
