use std::{
    collections::BTreeMap,
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
    pub options: ModelOptions,
}

/// Typed inference options shared by currently implemented capabilities.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tool_rounds: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_format: Option<String>,
}

impl ModelOptions {
    pub fn text_temperature(&self) -> f32 {
        self.temperature.unwrap_or(0.4)
    }

    pub fn text_max_tokens(&self) -> u32 {
        self.max_tokens.unwrap_or(4000)
    }

    pub fn tool_rounds(&self) -> usize {
        self.max_tool_rounds
            .filter(|rounds| *rounds > 0)
            .unwrap_or(8)
    }

    pub fn merge(&mut self, changes: Self) {
        macro_rules! replace_some {
            ($($field:ident),+ $(,)?) => {
                $(if changes.$field.is_some() { self.$field = changes.$field; })+
            };
        }
        replace_some!(
            temperature,
            max_tokens,
            max_tool_rounds,
            top_p,
            seed,
            quality,
            background,
            size,
            aspect_ratio,
            output_format,
        );
    }

    pub fn cli_pairs(&self) -> Vec<String> {
        let mut pairs = Vec::new();
        macro_rules! push_option {
            ($field:ident) => {
                if let Some(value) = &self.$field {
                    pairs.push(format!("{}={value}", stringify!($field)));
                }
            };
        }
        push_option!(temperature);
        push_option!(max_tokens);
        push_option!(max_tool_rounds);
        push_option!(top_p);
        push_option!(seed);
        push_option!(quality);
        push_option!(background);
        push_option!(size);
        push_option!(aspect_ratio);
        push_option!(output_format);
        pairs
    }
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
    pub fn default_config() -> Self {
        let mut models = BTreeMap::new();
        models.insert(
            "local-text".to_string(),
            ModelProfile {
                connector: "ollama".to_string(),
                model: "llama3.2".to_string(),
                capabilities: vec![Capability::Text, Capability::Code],
                options: ModelOptions {
                    temperature: Some(0.4),
                    max_tokens: Some(4000),
                    ..Default::default()
                },
            },
        );
        models.insert(
            "cloud-text".to_string(),
            ModelProfile {
                connector: "openrouter".to_string(),
                model: "openai/gpt-4o-mini".to_string(),
                capabilities: vec![Capability::Text, Capability::Code],
                options: ModelOptions {
                    temperature: Some(0.4),
                    max_tokens: Some(4000),
                    ..Default::default()
                },
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
    pub fn from_parts(
        global: GlobalConfig,
        selected_name: String,
        project_root: PathBuf,
        project: ProjectConfig,
        overrides: ConfigOverrides,
    ) -> Result<Self> {
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

    pub fn publish_root(&self) -> Result<Option<PathBuf>> {
        self.publish_dir
            .as_ref()
            .map(|publish_dir| {
                if publish_dir.is_absolute() {
                    Ok(publish_dir.clone())
                } else {
                    Ok(self.project_root.join(publish_dir))
                }
            })
            .transpose()
    }
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

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../tests/unit/config.rs"]
mod tests;
