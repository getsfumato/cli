use std::{collections::BTreeMap, path::PathBuf, str::FromStr};

use anyhow::{Context, Result, bail};

use crate::config::{
    Capability, GlobalConfig, ModelProfile, ProjectRegistry, load_project_config,
    project_config_path, projects_registry_path, user_config_path, write_toml,
};
use crate::themes::DEFAULT_THEME;

#[derive(Debug)]
pub struct ModelService {
    config: GlobalConfig,
    config_path: PathBuf,
    registry_path: PathBuf,
}

impl ModelService {
    pub fn load() -> Result<Self> {
        Ok(Self {
            config: GlobalConfig::load()?,
            config_path: user_config_path().context("Could not find user configuration path")?,
            registry_path: projects_registry_path()
                .context("Could not find project registry path")?,
        })
    }

    #[cfg(test)]
    fn load_from(config: GlobalConfig, config_path: PathBuf, registry_path: PathBuf) -> Self {
        Self {
            config,
            config_path,
            registry_path,
        }
    }

    pub fn list(&self) {
        if self.config.models.is_empty() {
            println!("No registered model profiles.");
            return;
        }
        for (name, profile) in &self.config.models {
            let capabilities = profile
                .capabilities
                .iter()
                .map(|capability| capability.as_str())
                .collect::<Vec<_>>()
                .join(",");
            println!(
                "{name}\t{}\t{}\t{capabilities}",
                profile.connector, profile.model
            );
        }
    }

    pub fn show(&self, name: &str) -> Result<String> {
        let profile = self
            .config
            .models
            .get(name)
            .with_context(|| format!("Model profile '{name}' was not found"))?;
        toml::to_string_pretty(profile).context("Could not render model profile")
    }

    pub fn profile(&self, name: &str) -> Result<ModelProfile> {
        self.config
            .models
            .get(name)
            .cloned()
            .with_context(|| format!("Model profile '{name}' was not found"))
    }

    pub fn add(
        &mut self,
        name: String,
        connector: String,
        model_id: String,
        capabilities: Vec<String>,
        options: Vec<String>,
    ) -> Result<()> {
        validate_profile_name(&name)?;
        if self.config.models.contains_key(&name) {
            bail!("Model profile '{name}' already exists");
        }
        if !self.config.connectors.contains_key(&connector) {
            bail!("Connector '{connector}' was not found");
        }
        if model_id.trim().is_empty() {
            bail!("Model ID cannot be empty");
        }
        let capabilities = parse_capabilities(&capabilities)?;
        let options = parse_options(&options)?;
        self.config.models.insert(
            name.clone(),
            ModelProfile {
                connector,
                model: model_id,
                capabilities,
                options,
            },
        );
        self.save()?;
        println!("Added model profile '{name}'");
        Ok(())
    }

    pub fn remove(&mut self, name: &str) -> Result<()> {
        if !self.config.models.contains_key(name) {
            bail!("Model profile '{name}' was not found");
        }
        if let Some(capability) = self
            .config
            .defaults
            .0
            .iter()
            .find_map(|(capability, profile)| (profile == name).then_some(*capability))
        {
            bail!(
                "Model profile '{name}' is the user default for '{}'; select another default first",
                capability.as_str()
            );
        }
        let registry = ProjectRegistry::load_from(&self.registry_path)?;
        for (project_name, registered) in registry.projects {
            let project =
                load_project_config(&project_config_path(&registered.path), DEFAULT_THEME)?;
            if let Some(capability) = project
                .model_defaults
                .iter()
                .find_map(|(capability, profile)| (profile == name).then_some(*capability))
            {
                bail!(
                    "Model profile '{name}' is the '{}' default for project '{project_name}'; select another default first",
                    capability.as_str()
                );
            }
        }
        self.config.models.remove(name);
        self.save()?;
        println!("Removed model profile '{name}'");
        Ok(())
    }

    pub fn edit(
        &mut self,
        name: &str,
        connector: Option<String>,
        model_id: Option<String>,
        capabilities: Vec<String>,
        options: Vec<String>,
    ) -> Result<()> {
        if connector.is_none()
            && model_id.is_none()
            && capabilities.is_empty()
            && options.is_empty()
        {
            bail!("No model profile changes were provided");
        }
        if let Some(connector) = &connector
            && !self.config.connectors.contains_key(connector)
        {
            bail!("Connector '{connector}' was not found");
        }
        if model_id
            .as_ref()
            .is_some_and(|model_id| model_id.trim().is_empty())
        {
            bail!("Model ID cannot be empty");
        }
        let parsed_capabilities = (!capabilities.is_empty())
            .then(|| parse_capabilities(&capabilities))
            .transpose()?;
        let parsed_options = parse_options(&options)?;
        let mut profile = self
            .config
            .models
            .get(name)
            .cloned()
            .with_context(|| format!("Model profile '{name}' was not found"))?;
        if let Some(connector) = connector {
            profile.connector = connector;
        }
        if let Some(model_id) = model_id {
            profile.model = model_id;
        }
        if let Some(capabilities) = parsed_capabilities {
            profile.capabilities = capabilities;
        }
        profile.options.extend(parsed_options);
        let mut updated_config = self.config.clone();
        updated_config.models.insert(name.to_string(), profile);
        validate_selected_capabilities(&updated_config, name, &self.registry_path)?;
        self.config = updated_config;
        self.save()?;
        println!("Updated model profile '{name}'");
        Ok(())
    }

    pub fn use_default(
        &mut self,
        capability: &str,
        profile_name: &str,
        project: Option<&str>,
    ) -> Result<()> {
        let capability = Capability::from_str(capability)?;
        let profile = self
            .config
            .models
            .get(profile_name)
            .with_context(|| format!("Model profile '{profile_name}' was not found"))?;
        if !profile.capabilities.contains(&capability) {
            bail!(
                "Model profile '{profile_name}' does not support '{}' capability",
                capability.as_str()
            );
        }

        if let Some(project_name) = project {
            let registry = ProjectRegistry::load_from(&self.registry_path)?;
            let (_, root) = registry.selected(Some(project_name))?;
            let path = project_config_path(&root);
            let mut project_config = load_project_config(&path, DEFAULT_THEME)?;
            project_config
                .model_defaults
                .insert(capability, profile_name.to_string());
            write_toml(&path, &project_config)?;
            println!(
                "Project '{}' now uses model profile '{profile_name}' for '{}'",
                project_config.name,
                capability.as_str()
            );
        } else {
            self.config
                .defaults
                .0
                .insert(capability, profile_name.to_string());
            self.save()?;
            println!(
                "User default for '{}' is now model profile '{profile_name}'",
                capability.as_str()
            );
        }
        Ok(())
    }

    fn save(&self) -> Result<()> {
        write_toml(&self.config_path, &self.config)
    }
}

fn validate_selected_capabilities(
    config: &GlobalConfig,
    profile_name: &str,
    registry_path: &std::path::Path,
) -> Result<()> {
    let profile = config
        .models
        .get(profile_name)
        .context("Edited model profile was not found")?;
    for (capability, selected_profile) in &config.defaults.0 {
        if selected_profile == profile_name && !profile.capabilities.contains(capability) {
            bail!(
                "Cannot remove '{}' capability because '{profile_name}' is the user default for it",
                capability.as_str()
            );
        }
    }
    let registry = ProjectRegistry::load_from(registry_path)?;
    for (project_name, registered) in registry.projects {
        let project = load_project_config(&project_config_path(&registered.path), DEFAULT_THEME)?;
        for (capability, selected_profile) in project.model_defaults {
            if selected_profile == profile_name && !profile.capabilities.contains(&capability) {
                bail!(
                    "Cannot remove '{}' capability because '{profile_name}' is the default for project '{project_name}'",
                    capability.as_str()
                );
            }
        }
    }
    Ok(())
}

fn validate_profile_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
        || name.starts_with('-')
        || name.ends_with('-')
    {
        bail!("Invalid model profile name '{name}'. Use lowercase letters, numbers, and hyphens.");
    }
    Ok(())
}

fn parse_capabilities(values: &[String]) -> Result<Vec<Capability>> {
    let mut parsed = values
        .iter()
        .map(|value| Capability::from_str(value.trim()))
        .collect::<Result<Vec<_>>>()?;
    parsed.sort();
    parsed.dedup();
    if parsed.is_empty() {
        bail!("Model profile must support at least one capability");
    }
    Ok(parsed)
}

fn parse_options(values: &[String]) -> Result<BTreeMap<String, toml::Value>> {
    values
        .iter()
        .map(|value| {
            let (key, raw) = value
                .split_once('=')
                .with_context(|| format!("Invalid model option '{value}'. Use key=value."))?;
            if key.trim().is_empty() {
                bail!("Model option key cannot be empty");
            }
            let parsed = raw
                .trim()
                .parse::<toml::Value>()
                .unwrap_or_else(|_| toml::Value::String(raw.trim().to_string()));
            Ok((key.trim().to_string(), parsed))
        })
        .collect()
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../tests/unit/models.rs"]
mod tests;
