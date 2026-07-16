use std::{
    collections::BTreeMap,
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sfumato_domain::SecretRef;

#[derive(Clone, Debug, Default)]
pub struct ConfigOverrides {
    pub project: Option<String>,
    pub theme: Option<String>,
    pub model_overrides: BTreeMap<Capability, String>,
    pub reviewer_model: Option<String>,
    pub publish_dir: Option<PathBuf>,
    pub pdf: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GlobalConfig {
    pub schema_version: u32,
    pub user: UserConfig,
    pub connectors: BTreeMap<String, OpenAiCompatibleConnectorConfig>,
    pub models: BTreeMap<String, ModelProfile>,
    pub defaults: ModelDefaults,
    #[serde(default)]
    pub model_roles: BTreeMap<ModelRole, String>,
    pub marp: MarpConfig,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UserConfig {
    pub name: Option<String>,
    pub learning_style: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectRegistry {
    pub schema_version: u32,
    pub active: Option<String>,
    pub projects: BTreeMap<String, RegisteredProject>,
}

impl Default for ProjectRegistry {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            active: None,
            projects: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RegisteredProject {
    pub path: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    pub schema_version: u32,
    pub name: String,
    pub theme: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publish_dir: Option<PathBuf>,
    #[serde(default)]
    pub model_defaults: BTreeMap<Capability, String>,
    #[serde(default)]
    pub model_roles: BTreeMap<ModelRole, String>,
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

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum ModelRole {
    Reviewer,
}

impl ModelRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reviewer => "reviewer",
        }
    }

    pub fn required_capability(self) -> Capability {
        match self {
            Self::Reviewer => Capability::Text,
        }
    }
}

impl FromStr for ModelRole {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value.to_lowercase().as_str() {
            "reviewer" => Ok(Self::Reviewer),
            _ => bail!("Unknown model role '{value}'. Use reviewer."),
        }
    }
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
    pub credential: Option<SecretRef>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MarpConfig {
    pub pdf: bool,
    #[serde(default)]
    pub browser_path: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize)]
pub struct EffectiveConfig {
    pub user: UserConfig,
    pub project_name: String,
    pub project_root: PathBuf,
    pub publish_dir: Option<PathBuf>,
    pub theme: String,
    pub connectors: BTreeMap<String, OpenAiCompatibleConnectorConfig>,
    pub models: BTreeMap<String, ModelProfile>,
    pub model_defaults: BTreeMap<Capability, String>,
    pub model_roles: BTreeMap<ModelRole, String>,
    pub marp: MarpConfig,
}

impl GlobalConfig {
    pub fn load() -> Result<Self> {
        let path = user_config_path().context("Could not find a user configuration directory")?;
        Self::load_from(&path)
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default_config());
        }

        ensure_config_version(path, "global")?;
        read_toml(path).with_context(|| {
            format!(
                "Could not load {}. Run `sfumato init user --force` to create a v0.2 configuration.",
                path.display()
            )
        })
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        write_toml(path, self)
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
            schema_version: CONFIG_SCHEMA_VERSION,
            user: UserConfig {
                name: None,
                learning_style: vec!["visual".to_string(), "step-by-step".to_string()],
            },
            connectors: BTreeMap::from([
                (
                    "ollama".to_string(),
                    OpenAiCompatibleConnectorConfig {
                        base_url: "http://localhost:11434/v1".to_string(),
                        credential: None,
                        headers: BTreeMap::new(),
                    },
                ),
                (
                    "openrouter".to_string(),
                    OpenAiCompatibleConnectorConfig {
                        base_url: "https://openrouter.ai/api/v1".to_string(),
                        credential: Some(
                            SecretRef::environment("OPENROUTER_API_KEY")
                                .expect("the bundled environment reference is valid"),
                        ),
                        headers: BTreeMap::new(),
                    },
                ),
            ]),
            models,
            defaults: ModelDefaults(BTreeMap::from([(
                Capability::Text,
                "local-text".to_string(),
            )])),
            model_roles: BTreeMap::new(),
            marp: MarpConfig {
                pdf: true,
                browser_path: None,
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
        ensure_config_version(path, "project registry")?;
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
        let project: ProjectConfig =
            load_project_config(&project_path, crate::themes::DEFAULT_THEME).with_context(
                || format!("Could not load project config {}", project_path.display()),
            )?;

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
        let model_roles = merge_model_roles(
            global.model_roles.clone(),
            project.model_roles.clone(),
            overrides.reviewer_model,
        );

        let publish_dir = overrides.publish_dir.or(project.publish_dir);
        let theme = resolve_theme_name(&project.theme, overrides.theme);
        let marp = project.marp.unwrap_or_else(|| global.marp.clone());
        let marp = MarpConfig {
            pdf: marp.pdf || overrides.pdf,
            browser_path: marp.browser_path,
        };

        Ok(Self {
            user: global.user,
            project_name: project.name,
            project_root,
            publish_dir,
            theme,
            connectors: global.connectors,
            models: global.models,
            model_defaults,
            model_roles,
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

    pub fn resolve_model_role(&self, role: ModelRole) -> Result<(&str, &ModelProfile)> {
        let fallback;
        let profile_name = if let Some(profile_name) = self.model_roles.get(&role) {
            profile_name.as_str()
        } else {
            fallback = self.resolve_model(role.required_capability())?.0;
            fallback
        };
        let profile = self
            .models
            .get(profile_name)
            .with_context(|| format!("Model profile '{profile_name}' was not found"))?;
        if !self.connectors.contains_key(&profile.connector) {
            bail!(
                "Model profile '{profile_name}' selected for '{}' references missing connector '{}'",
                role.as_str(),
                profile.connector
            );
        }
        let required = role.required_capability();
        if !profile.capabilities.contains(&required) {
            bail!(
                "Model profile '{profile_name}' selected for '{}' does not support '{}' capability",
                role.as_str(),
                required.as_str()
            );
        }
        Ok((profile_name, profile))
    }

    pub fn artifact_root(&self) -> Result<PathBuf> {
        project_artifact_root(&self.project_name)
    }

    pub fn publish_root(&self) -> Result<Option<PathBuf>> {
        self.publish_dir
            .as_ref()
            .map(|publish_dir| {
                if publish_dir.is_absolute() {
                    Ok(publish_dir.clone())
                } else {
                    Ok(absolutize(&self.project_root)?.join(publish_dir))
                }
            })
            .transpose()
    }
}

pub fn user_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("sfumato/config.toml"))
}

pub fn projects_registry_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("sfumato/projects.toml"))
}

pub fn themes_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("sfumato/themes"))
}

pub fn projects_artifact_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|dir| dir.join(".sfumato/Projects"))
}

pub fn project_artifact_root(project_name: &str) -> Result<PathBuf> {
    validate_project_name(project_name)?;
    Ok(projects_artifact_dir()
        .context("Could not find the user home directory for Sfumato artifacts")?
        .join(project_name))
}

pub fn validate_project_name(name: &str) -> Result<()> {
    let path = Path::new(name);
    if name.trim().is_empty() {
        bail!("Project name cannot be empty");
    }
    if path.components().count() != 1
        || !matches!(
            path.components().next(),
            Some(std::path::Component::Normal(_))
        )
    {
        bail!("Project name '{name}' cannot contain path separators or traversal");
    }
    Ok(())
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
    let parent = path
        .parent()
        .context("Config path must have a parent directory")?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).with_context(|| {
        format!(
            "Could not create a temporary config in {}",
            parent.display()
        )
    })?;
    temporary
        .write_all(rendered.as_bytes())
        .with_context(|| format!("Could not write temporary config for {}", path.display()))?;
    temporary
        .as_file()
        .sync_all()
        .with_context(|| format!("Could not sync temporary config for {}", path.display()))?;
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("Could not atomically replace {}", path.display()))?;
    Ok(())
}

pub fn load_project_config(path: &Path, _fallback_theme: &str) -> Result<ProjectConfig> {
    ensure_config_version(path, "project")?;
    read_toml(path)
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

fn merge_model_roles(
    mut user: BTreeMap<ModelRole, String>,
    project: BTreeMap<ModelRole, String>,
    reviewer: Option<String>,
) -> BTreeMap<ModelRole, String> {
    user.extend(project);
    if let Some(profile) = reviewer {
        user.insert(ModelRole::Reviewer, profile);
    }
    user
}

fn resolve_theme_name(project_theme: &str, command_theme: Option<String>) -> String {
    command_theme.unwrap_or_else(|| project_theme.to_string())
}

pub const CONFIG_SCHEMA_VERSION: u32 = 4;

fn read_toml_value(path: &Path) -> Result<toml::Value> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("Could not read config file {}", path.display()))?;
    toml::from_str::<toml::Value>(&text)
        .with_context(|| format!("Could not parse config file {}", path.display()))
}

fn ensure_config_version(path: &Path, kind: &str) -> Result<()> {
    let value = read_toml_value(path)?;
    let version = value
        .get("schema_version")
        .and_then(toml::Value::as_integer)
        .with_context(|| format!("{kind} config {} is missing schema_version", path.display()))?;
    if version != i64::from(CONFIG_SCHEMA_VERSION) {
        bail!(
            "Unsupported {kind} config schema {version} at {}; Sfumato v0.2 requires schema {}. Reinitialize the configuration instead of migrating it in place.",
            path.display(),
            CONFIG_SCHEMA_VERSION
        );
    }
    Ok(())
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../tests/unit/config.rs"]
mod tests;
