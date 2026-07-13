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
    pub theme: Option<String>,
    pub model_overrides: BTreeMap<Capability, String>,
    pub reviewer_model: Option<String>,
    pub output_dir: Option<PathBuf>,
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
    pub output_dir: PathBuf,
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
    pub api_key: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
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
    pub output_dir: PathBuf,
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
        crate::themes::ThemeService::load()?.install_default()?;
        Self::load_from(&path)
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default_config());
        }

        migrate_global_config(path)?;
        read_toml(path).with_context(|| {
            format!(
                "Could not load {}. Sfumato's v0.1 config format is no longer supported; run `sfumato init user --force` to reset it.",
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
            model_roles: BTreeMap::new(),
            marp: MarpConfig {
                pdf: false,
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
        migrate_registry_config(path)?;
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
        let legacy_theme = installed_legacy_user_theme();
        let global = GlobalConfig::load()?;
        let registry = ProjectRegistry::load()?;
        let (selected_name, project_root) = registry.selected(overrides.project.as_deref())?;
        let project_path = project_config_path(&project_root);
        let project: ProjectConfig = load_project_config(
            &project_path,
            legacy_theme
                .as_deref()
                .unwrap_or(crate::themes::DEFAULT_THEME),
        )
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
        let model_roles = merge_model_roles(
            global.model_roles.clone(),
            project.model_roles.clone(),
            overrides.reviewer_model,
        );

        let output_dir = overrides
            .output_dir
            .unwrap_or_else(|| project.output_dir.clone());
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
            output_dir,
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

pub fn themes_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("sfumato/themes"))
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

pub fn load_project_config(path: &Path, fallback_theme: &str) -> Result<ProjectConfig> {
    migrate_project_config(path, fallback_theme)?;
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

pub const CONFIG_SCHEMA_VERSION: u32 = 2;

fn migrate_global_config(path: &Path) -> Result<()> {
    let mut value = read_toml_value(path)?;
    let table = value
        .as_table_mut()
        .context("Global config must contain a TOML table")?;
    let current_version = table
        .get("schema_version")
        .and_then(toml::Value::as_integer)
        == Some(CONFIG_SCHEMA_VERSION.into());
    let has_legacy_shape = table.contains_key("inference")
        || table.contains_key("providers")
        || table.contains_key("diagrams")
        || table
            .get("user")
            .and_then(toml::Value::as_table)
            .is_some_and(|user| user.contains_key("theme"))
        || table
            .get("marp")
            .and_then(toml::Value::as_table)
            .is_some_and(|marp| marp.contains_key("theme"));
    if current_version && !has_legacy_shape {
        return Ok(());
    }
    let legacy_theme = table
        .get("user")
        .and_then(toml::Value::as_table)
        .and_then(|user| user.get("theme"))
        .and_then(toml::Value::as_str)
        .filter(|name| {
            themes_dir()
                .map(|root| root.join(name).join("theme.toml").is_file())
                .unwrap_or(false)
        })
        .unwrap_or(crate::themes::DEFAULT_THEME)
        .to_string();
    if user_config_path().as_deref() == Some(path) {
        migrate_registered_projects(&legacy_theme)?;
    }
    if let Some(user) = table.get_mut("user").and_then(toml::Value::as_table_mut) {
        user.remove("theme");
    }
    if let Some(marp) = table.get_mut("marp").and_then(toml::Value::as_table_mut) {
        marp.remove("theme");
    }
    table.remove("diagrams");
    migrate_legacy_providers(table)?;
    migrate_legacy_inference(table)?;
    table.insert(
        "schema_version".to_string(),
        toml::Value::Integer(CONFIG_SCHEMA_VERSION.into()),
    );
    write_migrated_toml(path, &value)
}

fn migrate_legacy_providers(table: &mut toml::Table) -> Result<()> {
    let Some(providers) = table.remove("providers") else {
        return Ok(());
    };
    let providers = providers
        .as_table()
        .context("Legacy providers config must contain a TOML table")?;
    let connectors = table
        .entry("connectors".to_string())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .context("Connectors config must contain a TOML table")?;
    for (name, provider) in providers {
        connectors
            .entry(name.clone())
            .or_insert_with(|| provider.clone());
    }
    Ok(())
}

fn migrate_legacy_inference(table: &mut toml::Table) -> Result<()> {
    let Some(inference) = table.remove("inference") else {
        return Ok(());
    };
    let inference = inference
        .as_table()
        .context("Legacy inference config must contain a TOML table")?;
    let connector = inference
        .get("provider")
        .and_then(toml::Value::as_str)
        .unwrap_or("ollama");
    let profile_name = if connector == "ollama" {
        "local-text".to_string()
    } else if connector == "openrouter" {
        "cloud-text".to_string()
    } else {
        format!("{connector}-text")
    };
    let model = inference
        .get("model")
        .and_then(toml::Value::as_str)
        .unwrap_or("llama3.2");
    let mut options = toml::Table::new();
    if let Some(temperature) = inference.get("temperature") {
        options.insert("temperature".to_string(), temperature.clone());
    }
    if let Some(max_tokens) = inference.get("max_tokens") {
        options.insert("max_tokens".to_string(), max_tokens.clone());
    }
    let profile = toml::Value::Table(toml::Table::from_iter([
        (
            "connector".to_string(),
            toml::Value::String(connector.to_string()),
        ),
        ("model".to_string(), toml::Value::String(model.to_string())),
        (
            "capabilities".to_string(),
            toml::Value::Array(vec![
                toml::Value::String("text".to_string()),
                toml::Value::String("code".to_string()),
            ]),
        ),
        ("options".to_string(), toml::Value::Table(options)),
    ]));
    table
        .entry("models".to_string())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .context("Models config must contain a TOML table")?
        .entry(profile_name.clone())
        .or_insert(profile);
    table
        .entry("defaults".to_string())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .context("Model defaults must contain a TOML table")?
        .entry("text".to_string())
        .or_insert_with(|| toml::Value::String(profile_name));
    Ok(())
}

fn migrate_registered_projects(fallback_theme: &str) -> Result<()> {
    let Some(registry_path) = projects_registry_path().filter(|path| path.exists()) else {
        return Ok(());
    };
    let registry = ProjectRegistry::load_from(&registry_path)?;
    for project in registry.projects.values() {
        let path = project_config_path(&project.path);
        if path.exists() {
            migrate_project_config(&path, fallback_theme)?;
        }
    }
    Ok(())
}

fn migrate_project_config(path: &Path, fallback_theme: &str) -> Result<()> {
    let mut value = read_toml_value(path)?;
    let table = value
        .as_table_mut()
        .context("Project config must contain a TOML table")?;
    if table
        .get("schema_version")
        .and_then(toml::Value::as_integer)
        == Some(CONFIG_SCHEMA_VERSION.into())
        && table.contains_key("theme")
    {
        return Ok(());
    }
    table
        .entry("theme".to_string())
        .or_insert_with(|| toml::Value::String(fallback_theme.to_string()));
    if let Some(marp) = table.get_mut("marp").and_then(toml::Value::as_table_mut) {
        marp.remove("theme");
    }
    table.insert(
        "schema_version".to_string(),
        toml::Value::Integer(CONFIG_SCHEMA_VERSION.into()),
    );
    write_migrated_toml(path, &value)
}

fn migrate_registry_config(path: &Path) -> Result<()> {
    let mut value = read_toml_value(path)?;
    let table = value
        .as_table_mut()
        .context("Project registry must contain a TOML table")?;
    if table
        .get("schema_version")
        .and_then(toml::Value::as_integer)
        == Some(CONFIG_SCHEMA_VERSION.into())
    {
        return Ok(());
    }
    table.insert(
        "schema_version".to_string(),
        toml::Value::Integer(CONFIG_SCHEMA_VERSION.into()),
    );
    write_migrated_toml(path, &value)
}

fn installed_legacy_user_theme() -> Option<String> {
    let config_path = user_config_path()?;
    let value = read_toml_value(&config_path).ok()?;
    let name = value.get("user")?.get("theme")?.as_str()?.to_string();
    let manifest = themes_dir()?.join(&name).join("theme.toml");
    manifest.is_file().then_some(name)
}

fn read_toml_value(path: &Path) -> Result<toml::Value> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("Could not read config file {}", path.display()))?;
    toml::from_str::<toml::Value>(&text)
        .with_context(|| format!("Could not parse config file {}", path.display()))
}

fn write_migrated_toml(path: &Path, value: &toml::Value) -> Result<()> {
    let backup = PathBuf::from(format!("{}.bak", path.display()));
    fs::copy(path, &backup)
        .with_context(|| format!("Could not back up config to {}", backup.display()))?;
    let temporary = PathBuf::from(format!("{}.tmp", path.display()));
    let rendered = toml::to_string_pretty(value).context("Could not render migrated config")?;
    fs::write(&temporary, rendered)
        .with_context(|| format!("Could not write {}", temporary.display()))?;
    fs::rename(&temporary, path)
        .with_context(|| format!("Could not replace migrated config {}", path.display()))
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../tests/unit/config.rs"]
mod tests;
