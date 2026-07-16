//! Persisted schema-v4 configuration documents.

use std::{collections::BTreeMap, path::PathBuf};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use sfumato_core::config::{
    Capability, GlobalConfig, ImageModelOptions, MarpConfig, ModelDefaults, ModelOptions,
    ModelProfile, ModelRole, OpenAiCompatibleConnectorConfig, ProjectConfig, ProjectRegistry,
    RegisteredProject, SecretRef, TextModelOptions, UserConfig,
};

/// Current persisted configuration schema.
pub const CONFIG_SCHEMA_VERSION: u32 = 4;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GlobalConfigDto {
    schema_version: u32,
    user: UserConfigDto,
    connectors: BTreeMap<String, ConnectorDto>,
    models: BTreeMap<String, ModelProfileDto>,
    defaults: BTreeMap<Capability, String>,
    #[serde(default)]
    model_roles: BTreeMap<ModelRole, String>,
    marp: MarpConfigDto,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct UserConfigDto {
    name: Option<String>,
    learning_style: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ConnectorDto {
    base_url: String,
    #[serde(default)]
    credential: Option<SecretRef>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ModelProfileDto {
    connector: String,
    model: String,
    capabilities: Vec<Capability>,
    #[serde(default)]
    options: ModelOptionsDto,
}

/// The v4 file intentionally keeps options flat for human editing. Conversion
/// groups them by capability before they enter the application layer.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ModelOptionsDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_tool_rounds: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    seed: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    quality: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    background: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    aspect_ratio: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    output_format: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct MarpConfigDto {
    pdf: bool,
    #[serde(default)]
    browser_path: Option<PathBuf>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProjectRegistryDto {
    schema_version: u32,
    active: Option<String>,
    projects: BTreeMap<String, RegisteredProjectDto>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RegisteredProjectDto {
    path: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProjectConfigDto {
    schema_version: u32,
    name: String,
    theme: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    publish_dir: Option<PathBuf>,
    #[serde(default)]
    model_defaults: BTreeMap<Capability, String>,
    #[serde(default)]
    model_roles: BTreeMap<ModelRole, String>,
    #[serde(default)]
    marp: Option<MarpConfigDto>,
}

impl GlobalConfigDto {
    pub(crate) fn into_domain(self) -> Result<GlobalConfig> {
        validate_schema(self.schema_version, "global")?;
        let config = GlobalConfig {
            user: UserConfig {
                name: self.user.name,
                learning_style: self.user.learning_style,
            },
            connectors: self
                .connectors
                .into_iter()
                .map(|(name, connector)| {
                    (
                        name,
                        OpenAiCompatibleConnectorConfig {
                            base_url: connector.base_url,
                            credential: connector.credential,
                            headers: connector.headers,
                        },
                    )
                })
                .collect(),
            models: self
                .models
                .into_iter()
                .map(|(name, profile)| {
                    (
                        name,
                        ModelProfile {
                            connector: profile.connector,
                            model: profile.model,
                            capabilities: profile.capabilities,
                            options: profile.options.into(),
                        },
                    )
                })
                .collect(),
            defaults: ModelDefaults(self.defaults),
            model_roles: self.model_roles,
            marp: self.marp.into(),
        };
        config.validate()?;
        Ok(config)
    }

    pub(crate) fn from_domain(config: &GlobalConfig) -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            user: UserConfigDto {
                name: config.user.name.clone(),
                learning_style: config.user.learning_style.clone(),
            },
            connectors: config
                .connectors
                .iter()
                .map(|(name, connector)| {
                    (
                        name.clone(),
                        ConnectorDto {
                            base_url: connector.base_url.clone(),
                            credential: connector.credential.clone(),
                            headers: connector.headers.clone(),
                        },
                    )
                })
                .collect(),
            models: config
                .models
                .iter()
                .map(|(name, profile)| {
                    (
                        name.clone(),
                        ModelProfileDto {
                            connector: profile.connector.clone(),
                            model: profile.model.clone(),
                            capabilities: profile.capabilities.clone(),
                            options: ModelOptionsDto::from(&profile.options),
                        },
                    )
                })
                .collect(),
            defaults: config.defaults.0.clone(),
            model_roles: config.model_roles.clone(),
            marp: MarpConfigDto::from(&config.marp),
        }
    }
}

impl ProjectRegistryDto {
    pub(crate) fn into_domain(self) -> Result<ProjectRegistry> {
        validate_schema(self.schema_version, "project registry")?;
        Ok(ProjectRegistry {
            active: self.active,
            projects: self
                .projects
                .into_iter()
                .map(|(name, project)| (name, RegisteredProject { path: project.path }))
                .collect(),
        })
    }

    pub(crate) fn from_domain(registry: &ProjectRegistry) -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            active: registry.active.clone(),
            projects: registry
                .projects
                .iter()
                .map(|(name, project)| {
                    (
                        name.clone(),
                        RegisteredProjectDto {
                            path: project.path.clone(),
                        },
                    )
                })
                .collect(),
        }
    }
}

impl ProjectConfigDto {
    pub(crate) fn into_domain(self) -> Result<ProjectConfig> {
        validate_schema(self.schema_version, "project")?;
        let project = ProjectConfig {
            name: self.name,
            theme: self.theme,
            publish_dir: self.publish_dir,
            model_defaults: self.model_defaults,
            model_roles: self.model_roles,
            marp: self.marp.map(Into::into),
        };
        project.validate()?;
        Ok(project)
    }

    pub(crate) fn from_domain(project: &ProjectConfig) -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            name: project.name.clone(),
            theme: project.theme.clone(),
            publish_dir: project.publish_dir.clone(),
            model_defaults: project.model_defaults.clone(),
            model_roles: project.model_roles.clone(),
            marp: project.marp.as_ref().map(MarpConfigDto::from),
        }
    }
}

impl From<ModelOptionsDto> for ModelOptions {
    fn from(options: ModelOptionsDto) -> Self {
        Self {
            text: TextModelOptions {
                temperature: options.temperature,
                max_tokens: options.max_tokens,
                max_tool_rounds: options.max_tool_rounds,
                top_p: options.top_p,
                seed: options.seed,
            },
            image: ImageModelOptions {
                quality: options.quality,
                background: options.background,
                size: options.size,
                aspect_ratio: options.aspect_ratio,
                output_format: options.output_format,
            },
        }
    }
}

impl From<&ModelOptions> for ModelOptionsDto {
    fn from(options: &ModelOptions) -> Self {
        Self {
            temperature: options.text.temperature,
            max_tokens: options.text.max_tokens,
            max_tool_rounds: options.text.max_tool_rounds,
            top_p: options.text.top_p,
            seed: options.text.seed,
            quality: options.image.quality.clone(),
            background: options.image.background.clone(),
            size: options.image.size.clone(),
            aspect_ratio: options.image.aspect_ratio.clone(),
            output_format: options.image.output_format.clone(),
        }
    }
}

impl From<MarpConfigDto> for MarpConfig {
    fn from(config: MarpConfigDto) -> Self {
        Self {
            pdf: config.pdf,
            browser_path: config.browser_path,
        }
    }
}

impl From<&MarpConfig> for MarpConfigDto {
    fn from(config: &MarpConfig) -> Self {
        Self {
            pdf: config.pdf,
            browser_path: config.browser_path.clone(),
        }
    }
}

fn validate_schema(actual: u32, kind: &str) -> Result<()> {
    if actual != CONFIG_SCHEMA_VERSION {
        bail!(
            "Unsupported {kind} config schema {actual}; Sfumato v0.2 requires schema {CONFIG_SCHEMA_VERSION}"
        );
    }
    Ok(())
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private persistence DTOs.
#[path = "../tests/unit/config_dto.rs"]
mod tests;
