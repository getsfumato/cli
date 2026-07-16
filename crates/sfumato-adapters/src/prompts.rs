//! Layered MiniJinja prompt templates.

use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
    str::FromStr,
};

use include_dir::{Dir, include_dir};
use minijinja::{AutoEscape, Environment, UndefinedBehavior};
use serde::Deserialize;
use sfumato_core::prompts::{
    PromptCatalog, PromptError, PromptId, PromptOrigin, PromptProvenance, PromptRenderRequest,
    RenderedPrompt,
};
use sha2::{Digest, Sha256};

static BUNDLED_PROMPTS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/assets/prompts");
const MAX_PROMPT_BYTES: u64 = 64 * 1024;

/// Metadata describing one bundled prompt template.
#[derive(Clone, Debug)]
pub struct PromptTemplateInfo {
    /// Stable identifier.
    pub id: PromptId,
    /// Relative template path.
    pub path: PathBuf,
    /// Variables declared as required by the prompt bundle.
    pub required: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct PromptManifest {
    schema_version: u32,
    prompts: Vec<PromptManifestEntry>,
}

#[derive(Debug, Deserialize)]
struct PromptManifestEntry {
    id: String,
    path: PathBuf,
    #[serde(default)]
    required: Vec<String>,
}

#[derive(Clone)]
struct TemplateSource {
    text: String,
    origin: PromptOrigin,
}

/// Prompt catalog resolving project, user, and bundled templates in precedence order.
#[derive(Clone, Debug)]
pub struct LayeredPromptCatalog {
    project_root: Option<PathBuf>,
    user_root: Option<PathBuf>,
}

impl LayeredPromptCatalog {
    /// Creates a catalog for a selected project using the platform user config directory.
    pub fn for_project(project_root: impl Into<PathBuf>) -> Result<Self, PromptError> {
        let user_root = dirs::config_dir().map(|path| path.join("sfumato").join("prompts"));
        Ok(Self::new(Some(project_root.into()), user_root))
    }

    /// Creates a catalog with explicit roots, primarily for alternate frontends and tests.
    pub fn new(project_root: Option<PathBuf>, user_root: Option<PathBuf>) -> Self {
        Self {
            project_root,
            user_root,
        }
    }

    /// Returns metadata for all supported prompts.
    pub fn list(&self) -> Result<Vec<PromptTemplateInfo>, PromptError> {
        let manifest = manifest()?;
        manifest
            .prompts
            .into_iter()
            .map(|entry| {
                Ok(PromptTemplateInfo {
                    id: PromptId::from_str(&entry.id)?,
                    path: safe_relative(&entry.path)?.to_path_buf(),
                    required: entry.required,
                })
            })
            .collect()
    }

    /// Returns the resolved source and provenance without rendering it.
    pub fn source(&self, id: PromptId) -> Result<(String, PromptProvenance), PromptError> {
        let info = self
            .list()?
            .into_iter()
            .find(|info| info.id == id)
            .ok_or(PromptError::Missing(id))?;
        let templates = self.load_templates()?;
        let source = templates.get(&info.path).ok_or(PromptError::Missing(id))?;
        Ok((
            source.text.clone(),
            provenance(id, manifest()?.schema_version, source),
        ))
    }

    /// Copies the bundled source into the requested override scope.
    pub fn customize(
        &self,
        id: PromptId,
        scope: PromptOverrideScope,
    ) -> Result<PathBuf, PromptError> {
        let info = self
            .list()?
            .into_iter()
            .find(|info| info.id == id)
            .ok_or(PromptError::Missing(id))?;
        let destination_root = match scope {
            PromptOverrideScope::User => {
                self.user_root.clone().ok_or_else(|| PromptError::Load {
                    id,
                    message: "the user configuration directory is unavailable".to_string(),
                })?
            }
            PromptOverrideScope::Project => self
                .project_root
                .as_ref()
                .map(|root| root.join(".sfumato").join("prompts"))
                .ok_or_else(|| PromptError::Load {
                    id,
                    message: "no project was selected".to_string(),
                })?,
        };
        let destination = destination_root.join(&info.path);
        if destination.exists() {
            return Err(PromptError::Load {
                id,
                message: format!("override already exists at {}", destination.display()),
            });
        }
        let bundled = bundled_file(&info.path).ok_or(PromptError::Missing(id))?;
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|error| PromptError::Load {
                id,
                message: error.to_string(),
            })?;
        }
        fs::write(&destination, bundled).map_err(|error| PromptError::Load {
            id,
            message: error.to_string(),
        })?;
        Ok(destination)
    }

    fn load_templates(&self) -> Result<BTreeMap<PathBuf, TemplateSource>, PromptError> {
        let mut templates = BTreeMap::new();
        load_bundled(&mut templates)?;
        if let Some(root) = &self.user_root {
            load_override_root(root, false, &mut templates)?;
        }
        if let Some(project) = &self.project_root {
            load_override_root(
                &project.join(".sfumato").join("prompts"),
                true,
                &mut templates,
            )?;
        }
        Ok(templates)
    }

    fn environment(
        &self,
    ) -> Result<(Environment<'static>, BTreeMap<PathBuf, TemplateSource>), PromptError> {
        let sources = self.load_templates()?;
        let mut environment = Environment::new();
        environment.set_undefined_behavior(UndefinedBehavior::Strict);
        environment.set_auto_escape_callback(|_| AutoEscape::None);
        for (path, source) in &sources {
            let name = prompt_name(path)?;
            environment
                .add_template_owned(name, source.text.clone())
                .map_err(|error| PromptError::Load {
                    id: prompt_id_for_path(path).unwrap_or(PromptId::SlidesDraftSystem),
                    message: error.to_string(),
                })?;
        }
        Ok((environment, sources))
    }
}

impl PromptCatalog for LayeredPromptCatalog {
    fn render(&self, request: PromptRenderRequest) -> Result<RenderedPrompt, PromptError> {
        let info = self
            .list()?
            .into_iter()
            .find(|info| info.id == request.id)
            .ok_or(PromptError::Missing(request.id))?;
        for required in &info.required {
            if !request.variables.0.contains_key(required) {
                return Err(PromptError::Render {
                    id: request.id,
                    message: format!("required variable '{required}' was not supplied"),
                });
            }
        }
        let (environment, sources) = self.environment()?;
        let name = prompt_name(&info.path)?;
        let template = environment
            .get_template(&name)
            .map_err(|error| PromptError::Render {
                id: request.id,
                message: error.to_string(),
            })?;
        let text = template
            .render(ValueContext(request.variables.0))
            .map_err(|error| PromptError::Render {
                id: request.id,
                message: error.to_string(),
            })?;
        let source = sources
            .get(&info.path)
            .ok_or(PromptError::Missing(request.id))?;
        Ok(RenderedPrompt {
            text: text.trim().to_string(),
            provenance: provenance(request.id, manifest()?.schema_version, source),
        })
    }

    fn validate(&self) -> Result<Vec<PromptProvenance>, PromptError> {
        let (environment, sources) = self.environment()?;
        let manifest = manifest()?;
        let mut resolved = Vec::new();
        for entry in manifest.prompts {
            let id = PromptId::from_str(&entry.id)?;
            let path = safe_relative(&entry.path)?;
            environment
                .get_template(&prompt_name(path)?)
                .map_err(|error| PromptError::Render {
                    id,
                    message: error.to_string(),
                })?;
            let source = sources.get(path).ok_or(PromptError::Missing(id))?;
            resolved.push(provenance(id, manifest.schema_version, source));
        }
        Ok(resolved)
    }
}

/// Scope into which a bundled prompt should be copied for customization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PromptOverrideScope {
    /// User-global prompt override.
    User,
    /// Selected-project prompt override.
    Project,
}

#[derive(serde::Serialize)]
struct ValueContext(serde_json::Map<String, serde_json::Value>);

fn manifest() -> Result<PromptManifest, PromptError> {
    let source = BUNDLED_PROMPTS
        .get_file("manifest.toml")
        .and_then(|file| file.contents_utf8())
        .ok_or_else(|| PromptError::Load {
            id: PromptId::SlidesDraftSystem,
            message: "bundled prompt manifest is missing or not UTF-8".to_string(),
        })?;
    let manifest: PromptManifest = toml::from_str(source).map_err(|error| PromptError::Load {
        id: PromptId::SlidesDraftSystem,
        message: format!("invalid bundled prompt manifest: {error}"),
    })?;
    if manifest.schema_version != 1 {
        return Err(PromptError::Load {
            id: PromptId::SlidesDraftSystem,
            message: format!(
                "unsupported bundled prompt schema {}",
                manifest.schema_version
            ),
        });
    }
    Ok(manifest)
}

fn load_bundled(templates: &mut BTreeMap<PathBuf, TemplateSource>) -> Result<(), PromptError> {
    collect_bundled_dir(&BUNDLED_PROMPTS, templates)
}

fn collect_bundled_dir(
    directory: &Dir<'_>,
    templates: &mut BTreeMap<PathBuf, TemplateSource>,
) -> Result<(), PromptError> {
    for file in directory.files() {
        let path = safe_relative(file.path())?;
        if path == Path::new("manifest.toml") {
            continue;
        }
        let Some(text) = file.contents_utf8() else {
            return Err(PromptError::UnsafePath(path.display().to_string()));
        };
        if text.len() as u64 > MAX_PROMPT_BYTES {
            return Err(PromptError::Load {
                id: prompt_id_for_path(path).unwrap_or(PromptId::SlidesDraftSystem),
                message: format!("template exceeds the {MAX_PROMPT_BYTES}-byte limit"),
            });
        }
        templates.insert(
            path.to_path_buf(),
            TemplateSource {
                text: text.to_string(),
                origin: PromptOrigin::Bundled,
            },
        );
    }
    for child in directory.dirs() {
        collect_bundled_dir(child, templates)?;
    }
    Ok(())
}

fn load_override_root(
    root: &Path,
    project: bool,
    templates: &mut BTreeMap<PathBuf, TemplateSource>,
) -> Result<(), PromptError> {
    if !root.exists() {
        return Ok(());
    }
    collect_override_files(root, root, project, templates)
}

fn collect_override_files(
    root: &Path,
    directory: &Path,
    project: bool,
    templates: &mut BTreeMap<PathBuf, TemplateSource>,
) -> Result<(), PromptError> {
    for entry in fs::read_dir(directory).map_err(|error| PromptError::Load {
        id: PromptId::SlidesDraftSystem,
        message: format!("could not read {}: {error}", directory.display()),
    })? {
        let entry = entry.map_err(|error| PromptError::Load {
            id: PromptId::SlidesDraftSystem,
            message: error.to_string(),
        })?;
        let file_type = entry.file_type().map_err(|error| PromptError::Load {
            id: PromptId::SlidesDraftSystem,
            message: error.to_string(),
        })?;
        if file_type.is_symlink() {
            return Err(PromptError::UnsafePath(entry.path().display().to_string()));
        }
        if file_type.is_dir() {
            collect_override_files(root, &entry.path(), project, templates)?;
            continue;
        }
        let path = entry
            .path()
            .strip_prefix(root)
            .map_err(|_| PromptError::UnsafePath(entry.path().display().to_string()))?
            .to_path_buf();
        safe_relative(&path)?;
        if path.extension().and_then(|value| value.to_str()) != Some("j2") {
            continue;
        }
        let bytes = entry
            .metadata()
            .map_err(|error| PromptError::Load {
                id: prompt_id_for_path(&path).unwrap_or(PromptId::SlidesDraftSystem),
                message: error.to_string(),
            })?
            .len();
        if bytes > MAX_PROMPT_BYTES {
            return Err(PromptError::Load {
                id: prompt_id_for_path(&path).unwrap_or(PromptId::SlidesDraftSystem),
                message: format!("template exceeds the {MAX_PROMPT_BYTES}-byte limit"),
            });
        }
        let text = fs::read_to_string(entry.path()).map_err(|error| PromptError::Load {
            id: prompt_id_for_path(&path).unwrap_or(PromptId::SlidesDraftSystem),
            message: error.to_string(),
        })?;
        let origin = if project {
            PromptOrigin::Project(entry.path())
        } else {
            PromptOrigin::User(entry.path())
        };
        templates.insert(path, TemplateSource { text, origin });
    }
    Ok(())
}

fn bundled_file(path: &Path) -> Option<&'static str> {
    BUNDLED_PROMPTS
        .get_file(path)
        .and_then(|file| file.contents_utf8())
}

fn safe_relative(path: &Path) -> Result<&Path, PromptError> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(PromptError::UnsafePath(path.display().to_string()));
    }
    Ok(path)
}

fn prompt_name(path: &Path) -> Result<String, PromptError> {
    safe_relative(path)?;
    Ok(path.to_string_lossy().replace('\\', "/"))
}

fn prompt_id_for_path(path: &Path) -> Option<PromptId> {
    manifest().ok()?.prompts.into_iter().find_map(|entry| {
        (entry.path == path)
            .then(|| PromptId::from_str(&entry.id).ok())
            .flatten()
    })
}

fn provenance(id: PromptId, version: u32, source: &TemplateSource) -> PromptProvenance {
    let content_hash = format!("{:x}", Sha256::digest(source.text.as_bytes()));
    PromptProvenance {
        id,
        origin: source.origin.clone(),
        version,
        content_hash,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsafe_relative_paths() {
        assert!(safe_relative(Path::new("../prompt.md.j2")).is_err());
        assert!(safe_relative(Path::new("/tmp/prompt.md.j2")).is_err());
    }
}
