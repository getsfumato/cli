use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::cli::ProviderArg;

#[derive(Debug, Default)]
pub struct ConfigOverrides {
    pub provider: Option<ProviderArg>,
    pub model: Option<String>,
    pub output_dir: Option<PathBuf>,
    pub pdf: bool,
    pub config_path: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct PartialConfig {
    pub user: Option<UserConfig>,
    pub project: Option<ProjectConfig>,
    pub inference: Option<InferenceConfig>,
    pub providers: Option<ProvidersConfig>,
    pub marp: Option<MarpConfig>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SfumatoConfig {
    pub user: UserConfig,
    pub project: ProjectConfig,
    pub inference: InferenceConfig,
    pub providers: ProvidersConfig,
    pub marp: MarpConfig,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UserConfig {
    pub name: Option<String>,
    pub learning_style: Vec<String>,
    pub theme: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProjectConfig {
    pub name: String,
    pub vault_root: PathBuf,
    pub output_dir: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InferenceConfig {
    pub provider: String,
    pub model: String,
    pub temperature: f32,
    pub max_tokens: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProvidersConfig {
    pub ollama: OpenAiLikeProviderConfig,
    pub openrouter: OpenAiLikeProviderConfig,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct OpenAiLikeProviderConfig {
    pub base_url: String,
    pub api_key: Option<String>,
    pub api_key_env: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MarpConfig {
    pub theme: String,
    pub pdf: bool,
}

impl SfumatoConfig {
    pub fn load(overrides: ConfigOverrides) -> Result<Self> {
        let mut partial = PartialConfig::default();

        if let Some(user_path) = user_config_path() {
            merge_if_exists(&mut partial, &user_path)?;
        }

        let project_path = overrides
            .config_path
            .clone()
            .unwrap_or_else(|| PathBuf::from(".sfumato/project.toml"));
        merge_if_exists(&mut partial, &project_path)?;

        let mut config = Self::from_partial(partial)?;
        config.apply_overrides(overrides);
        Ok(config)
    }

    fn from_partial(partial: PartialConfig) -> Result<Self> {
        let cwd = env::current_dir().context("Could not read current directory")?;

        let defaults = Self::default_for_cwd(cwd);

        Ok(Self {
            user: partial.user.unwrap_or(defaults.user),
            project: partial.project.unwrap_or(defaults.project),
            inference: partial.inference.unwrap_or(defaults.inference),
            providers: partial.providers.unwrap_or(defaults.providers),
            marp: partial.marp.unwrap_or(defaults.marp),
        })
    }

    pub fn default_for_cwd(cwd: PathBuf) -> Self {
        Self {
            user: UserConfig {
                name: None,
                learning_style: vec!["visual".to_string(), "step-by-step".to_string()],
                theme: "sfumato-default".to_string(),
            },
            project: ProjectConfig {
                name: "sfumato-project".to_string(),
                vault_root: cwd,
                output_dir: PathBuf::from("Resources/Sfumato"),
            },
            inference: InferenceConfig {
                provider: "ollama".to_string(),
                model: "llama3.2".to_string(),
                temperature: 0.4,
                max_tokens: 4000,
            },
            providers: ProvidersConfig {
                ollama: OpenAiLikeProviderConfig {
                    base_url: "http://localhost:11434/v1".to_string(),
                    api_key: Some("ollama".to_string()),
                    api_key_env: None,
                },
                openrouter: OpenAiLikeProviderConfig {
                    base_url: "https://openrouter.ai/api/v1".to_string(),
                    api_key: None,
                    api_key_env: Some("OPENROUTER_API_KEY".to_string()),
                },
            },
            marp: MarpConfig {
                theme: "default".to_string(),
                pdf: false,
            },
        }
    }

    fn apply_overrides(&mut self, overrides: ConfigOverrides) {
        if let Some(provider) = overrides.provider {
            self.inference.provider = match provider {
                ProviderArg::Ollama => "ollama".to_string(),
                ProviderArg::Openrouter => "openrouter".to_string(),
            };
        }

        if let Some(model) = overrides.model {
            self.inference.model = model;
        }

        if let Some(output_dir) = overrides.output_dir {
            self.project.output_dir = output_dir;
        }

        if overrides.pdf {
            self.marp.pdf = true;
        }
    }

    pub fn output_root(&self) -> Result<PathBuf> {
        let vault_root = absolutize(&self.project.vault_root)?;
        let output_root = if self.project.output_dir.is_absolute() {
            self.project.output_dir.clone()
        } else {
            vault_root.join(&self.project.output_dir)
        };

        let output_root = absolutize(&output_root)?;
        if !output_root.starts_with(&vault_root) {
            bail!(
                "Configured output directory {} is outside vault root {}",
                output_root.display(),
                vault_root.display()
            );
        }

        Ok(output_root)
    }
}

pub fn user_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("sfumato/config.toml"))
}

pub fn project_config_path() -> PathBuf {
    PathBuf::from(".sfumato/project.toml")
}

fn merge_if_exists(base: &mut PartialConfig, path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

    let text = fs::read_to_string(path)
        .with_context(|| format!("Could not read config file {}", path.display()))?;
    let next: PartialConfig = toml::from_str(&text)
        .with_context(|| format!("Could not parse config file {}", path.display()))?;

    merge_partial(base, next);
    Ok(())
}

fn merge_partial(base: &mut PartialConfig, next: PartialConfig) {
    if next.user.is_some() {
        base.user = next.user;
    }
    if next.project.is_some() {
        base.project = next.project;
    }
    if next.inference.is_some() {
        base.inference = next.inference;
    }
    if next.providers.is_some() {
        base.providers = next.providers;
    }
    if next.marp.is_some() {
        base.marp = next.marp;
    }
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

pub const USER_CONFIG_TEMPLATE: &str = r#"[user]
name = "Alex"
learning_style = ["visual", "step-by-step"]
theme = "sfumato-default"

[inference]
provider = "ollama"
model = "llama3.2"
temperature = 0.4
max_tokens = 4000

[providers.ollama]
base_url = "http://localhost:11434/v1"
api_key = "ollama"

[providers.openrouter]
base_url = "https://openrouter.ai/api/v1"
api_key_env = "OPENROUTER_API_KEY"

[marp]
theme = "default"
pdf = false
"#;

pub const PROJECT_CONFIG_TEMPLATE: &str = r#"[project]
name = "university"
vault_root = "."
output_dir = "Resources/Sfumato"
"#;

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../tests/unit/config.rs"]
mod tests;
