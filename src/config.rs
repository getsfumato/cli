use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default)]
pub struct ConfigOverrides {
    pub project: Option<String>,
    pub model_overrides: BTreeMap<Capability, String>,
    pub output_dir: Option<PathBuf>,
    pub pdf: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GlobalConfig {
    pub user: UserConfig,
    pub connectors: BTreeMap<String, OpenAiCompatibleConnectorConfig>,
    pub models: BTreeMap<String, ModelProfile>,
    pub defaults: ModelDefaults,
    pub marp: MarpConfig,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UserConfig {
    pub name: Option<String>,
    pub learning_style: Vec<String>,
    pub theme: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectRegistry {
    pub active: Option<String>,
    pub projects: BTreeMap<String, RegisteredProject>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RegisteredProject {
    pub path: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    pub name: String,
    pub output_dir: PathBuf,
    #[serde(default)]
    pub model_defaults: BTreeMap<Capability, String>,
    #[serde(default)]
    pub marp: Option<MarpConfig>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProfile {
    pub connector: String,
    pub model: String,
    pub capabilities: Vec<Capability>,
    #[serde(default)]
    pub options: BTreeMap<String, toml::Value>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(transparent)]
pub struct ModelDefaults(pub BTreeMap<Capability, String>);

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Capability {
    Text,
    Code,
    Image,
    Video,
    Speech,
    Embedding,
}

impl Capability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Code => "code",
            Self::Image => "image",
            Self::Video => "video",
            Self::Speech => "speech",
            Self::Embedding => "embedding",
        }
    }
}

impl FromStr for Capability {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value.to_lowercase().as_str() {
            "text" => Ok(Self::Text),
            "code" => Ok(Self::Code),
            "image" => Ok(Self::Image),
            "video" => Ok(Self::Video),
            "speech" => Ok(Self::Speech),
            "embedding" => Ok(Self::Embedding),
            _ => bail!(
                "Unknown capability '{value}'. Use text, code, image, video, speech, or embedding."
            ),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OpenAiCompatibleConnectorConfig {
    pub base_url: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MarpConfig {
    pub theme: String,
    pub pdf: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct EffectiveConfig {
    pub user: UserConfig,
    pub project_name: String,
    pub project_root: PathBuf,
    pub output_dir: PathBuf,
    pub connectors: BTreeMap<String, OpenAiCompatibleConnectorConfig>,
    pub models: BTreeMap<String, ModelProfile>,
    pub model_defaults: BTreeMap<Capability, String>,
    pub marp: MarpConfig,
}

impl GlobalConfig {
    pub fn load() -> Result<Self> {
        let path = user_config_path().context("Could not find a user configuration directory")?;
        if !path.exists() {
            return Ok(Self::default_config());
        }

        read_toml(&path).with_context(|| {
            format!(
                "Could not load {}. Sfumato's v0.1 config format is no longer supported; run `sfumato init user --force` to reset it.",
                path.display()
            )
        })
    }

    pub fn default_config() -> Self {
        let mut models = BTreeMap::new();
        models.insert(
            "local-text".to_string(),
            ModelProfile {
                connector: "ollama".to_string(),
                model: "llama3.2".to_string(),
                capabilities: vec![Capability::Text, Capability::Code],
                options: BTreeMap::from([
                    ("temperature".to_string(), toml::Value::Float(0.4)),
                    ("max_tokens".to_string(), toml::Value::Integer(4000)),
                ]),
            },
        );
        models.insert(
            "cloud-text".to_string(),
            ModelProfile {
                connector: "openrouter".to_string(),
                model: "openai/gpt-4o-mini".to_string(),
                capabilities: vec![Capability::Text, Capability::Code],
                options: BTreeMap::from([
                    ("temperature".to_string(), toml::Value::Float(0.4)),
                    ("max_tokens".to_string(), toml::Value::Integer(4000)),
                ]),
            },
        );

        Self {
            user: UserConfig {
                name: None,
                learning_style: vec!["visual".to_string(), "step-by-step".to_string()],
                theme: "sfumato-default".to_string(),
            },
            connectors: BTreeMap::from([
                (
                    "ollama".to_string(),
                    OpenAiCompatibleConnectorConfig {
                        base_url: "http://localhost:11434/v1".to_string(),
                        api_key: Some("ollama".to_string()),
                        api_key_env: None,
                        headers: BTreeMap::new(),
                    },
                ),
                (
                    "openrouter".to_string(),
                    OpenAiCompatibleConnectorConfig {
                        base_url: "https://openrouter.ai/api/v1".to_string(),
                        api_key: None,
                        api_key_env: Some("OPENROUTER_API_KEY".to_string()),
                        headers: BTreeMap::new(),
                    },
                ),
            ]),
            models,
            defaults: ModelDefaults(BTreeMap::from([(
                Capability::Text,
                "local-text".to_string(),
            )])),
            marp: MarpConfig {
                theme: "default".to_string(),
                pdf: false,
            },
        }
    }
}

impl ProjectRegistry {
    pub fn load() -> Result<Self> {
        let path = projects_registry_path()
            .context("Could not find a user configuration directory for projects")?;
        Self::load_from(&path)
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        read_toml(path)
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        write_toml(path, self)
    }

    pub fn selected(&self, requested: Option<&str>) -> Result<(String, PathBuf)> {
        let name = requested
            .map(ToOwned::to_owned)
            .or_else(|| self.active.clone())
            .context("No active project. Run `sfumato init project <name>` or `sfumato project use <name>`.")?;
        let project = self
            .projects
            .get(&name)
            .with_context(|| format!("Project '{name}' is not registered"))?;
        Ok((name, project.path.clone()))
    }
}

impl EffectiveConfig {
    pub fn load(overrides: ConfigOverrides) -> Result<Self> {
        let global = GlobalConfig::load()?;
        let registry = ProjectRegistry::load()?;
        let (selected_name, project_root) = registry.selected(overrides.project.as_deref())?;
        let project_path = project_config_path(&project_root);
        let project: ProjectConfig = read_toml(&project_path)
            .with_context(|| format!("Could not load project config {}", project_path.display()))?;

        if project.name != selected_name {
            bail!(
                "Registered project name '{selected_name}' does not match project config name '{}'",
                project.name
            );
        }

        let model_defaults = merge_model_defaults(
            global.defaults.0.clone(),
            project.model_defaults.clone(),
            overrides.model_overrides,
        );

        let output_dir = overrides
            .output_dir
            .unwrap_or_else(|| project.output_dir.clone());
        let marp = project.marp.unwrap_or_else(|| global.marp.clone());
        let marp = MarpConfig {
            theme: marp.theme,
            pdf: marp.pdf || overrides.pdf,
        };

        Ok(Self {
            user: global.user,
            project_name: project.name,
            project_root,
            output_dir,
            connectors: global.connectors,
            models: global.models,
            model_defaults,
            marp,
        })
    }

    pub fn resolve_model(&self, capability: Capability) -> Result<(&str, &ModelProfile)> {
        let profile_name = self.model_defaults.get(&capability).with_context(|| {
            format!(
                "No model profile configured for '{}' capability",
                capability.as_str()
            )
        })?;
        let profile = self
            .models
            .get(profile_name)
            .with_context(|| format!("Model profile '{profile_name}' was not found"))?;
        if !profile.capabilities.contains(&capability) {
            bail!(
                "Model profile '{profile_name}' does not support '{}' capability",
                capability.as_str()
            );
        }
        Ok((profile_name, profile))
    }

    pub fn output_root(&self) -> Result<PathBuf> {
        let root = absolutize(&self.project_root)?;
        let output = if self.output_dir.is_absolute() {
            self.output_dir.clone()
        } else {
            root.join(&self.output_dir)
        };
        let output = absolutize(&output)?;
        if !output.starts_with(&root) {
            bail!(
                "Configured output directory {} is outside project root {}",
                output.display(),
                root.display()
            );
        }
        Ok(output)
    }
}

pub fn user_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("sfumato/config.toml"))
}

pub fn projects_registry_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("sfumato/projects.toml"))
}

pub fn project_config_path(project_root: &Path) -> PathBuf {
    project_root.join(".sfumato/project.toml")
}

pub fn read_toml<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let text =
        fs::read_to_string(path).with_context(|| format!("Could not read {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("Could not parse {}", path.display()))
}

pub fn write_toml<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Could not create {}", parent.display()))?;
    }
    let rendered = toml::to_string_pretty(value).context("Could not render config as TOML")?;
    fs::write(path, rendered).with_context(|| format!("Could not write {}", path.display()))
}

fn absolutize(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(env::current_dir()
            .context("Could not read current directory")?
            .join(path))
    }
}

fn merge_model_defaults(
    mut user: BTreeMap<Capability, String>,
    project: BTreeMap<Capability, String>,
    command: BTreeMap<Capability, String>,
) -> BTreeMap<Capability, String> {
    user.extend(project);
    user.extend(command);
    user
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../tests/unit/config.rs"]
mod tests;
