use std::{collections::BTreeMap, env, sync::Arc};

use anyhow::{Context, Result, bail};
use indicatif::{ProgressBar, ProgressStyle};
use inquire::{Confirm, Select, Text};

use sfumato_core::application::SfumatoApplication;
use sfumato_core::config::{
    Capability, GlobalConfig, ModelDefaults, ModelOptions, ModelProfile, TextModelOptions,
};
use sfumato_core::connectors::ConnectorPreset;

pub struct InitService {
    application: Arc<SfumatoApplication>,
}

impl InitService {
    pub fn new(application: Arc<SfumatoApplication>) -> Self {
        Self { application }
    }

    pub fn write_user_config(&self, yes: bool, force: bool) -> Result<()> {
        if self.application.user_config_exists()
            && !force
            && (yes
                || !Confirm::new(&format!(
                    "{} already exists. Overwrite it?",
                    self.application.user_config_path().display()
                ))
                .with_default(false)
                .prompt()?)
        {
            bail!("User config already exists. Re-run with --force to overwrite it.");
        }

        let config = if yes {
            GlobalConfig::default_config()
        } else {
            println!("Let's set up your personal Sfumato preferences.");
            ask_user_preferences()?
        };

        let spinner = ProgressBar::new_spinner();
        spinner.set_style(
            ProgressStyle::with_template("{spinner:.cyan} {msg}")
                .unwrap_or_else(|_| ProgressStyle::default_spinner()),
        );
        spinner.set_message("Writing user config");
        spinner.enable_steady_tick(std::time::Duration::from_millis(80));
        let result = self.application.setup_user(config)?;
        spinner.finish_with_message(format!("Wrote {}", result.path.display()));
        Ok(())
    }
}

fn ask_user_preferences() -> Result<GlobalConfig> {
    let mut config = GlobalConfig::default_config();
    let default_name = env::var("USER").unwrap_or_else(|_| "Alex".to_string());
    let name = prompt("Name", &default_name)?;
    let learning_style = prompt("Learning styles", "visual, step-by-step")?
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if learning_style.is_empty() {
        bail!("Learning styles must include at least one value");
    }
    config.user.name = Some(name);
    config.user.learning_style = learning_style;
    let preset = Select::new("Default text connector", ConnectorPreset::ALL.to_vec()).prompt()?;
    let connector = preset.default_connector_name().to_string();
    // `default_config` ships only a subset of the presets, and
    // `GlobalConfig::validate` rejects a profile naming an absent connector, so
    // the chosen preset has to be configured before the profile references it.
    config
        .connectors
        .insert(connector.clone(), preset.into_config(&connector, None)?);
    let profile_name = preset.default_text_profile_name();
    let model = prompt("Default text model", preset.default_text_model())?;
    config.models.insert(
        profile_name.to_string(),
        ModelProfile {
            connector,
            model,
            capabilities: vec![Capability::Text, Capability::Code],
            options: ModelOptions {
                text: TextModelOptions {
                    temperature: Some(0.4),
                    max_tokens: Some(4000),
                    ..Default::default()
                },
                ..Default::default()
            },
        },
    );
    config.defaults = ModelDefaults(BTreeMap::from([(
        Capability::Text,
        profile_name.to_string(),
    )]));
    Ok(config)
}

fn prompt(label: &str, default: &str) -> Result<String> {
    Text::new(label)
        .with_default(default)
        .prompt()
        .with_context(|| format!("Could not read answer for {label}"))
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../tests/unit/init.rs"]
mod tests;
