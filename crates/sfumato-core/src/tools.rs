use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::{Value, json};

use crate::providers::{
    ToolDefinition, ToolExecutionRequest, ToolExecutor, ToolFunctionDefinition,
};

const MAX_DIRECTORY_ENTRIES: usize = 200;
const MAX_FILE_BYTES: u64 = 128 * 1024;

#[derive(Clone)]
pub struct ToolSet {
    pub definitions: Vec<ToolDefinition>,
    pub executor: Arc<dyn ToolExecutor>,
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

impl ToolExecutor for FilesystemToolExecutor {
    fn execute(&self, request: ToolExecutionRequest) -> Result<String> {
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

#[derive(Serialize)]
struct DirectoryEntry {
    name: String,
    path: String,
    kind: String,
    bytes: Option<u64>,
}

pub fn default_filesystem_tools(project_root: &Path, sources: &[PathBuf]) -> Result<ToolSet> {
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

    Ok(ToolSet {
        definitions: vec![list_directory_tool(), read_file_tool()],
        executor: Arc::new(FilesystemToolExecutor::new(roots)?),
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

#[cfg(test)]
#[path = "../tests/unit/tools.rs"]
mod tests;
