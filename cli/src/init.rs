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

/// The current user's name, as the setup wizard's default.
///
/// `USER` is set by a login shell on Unix, so a missing value is invisible during
/// development — but it is absent on Windows, where the variable is `USERNAME`, in
/// most containers, and in several CI runners. There the wizard used to pre-fill the
/// maintainer's own name, presenting a stranger's name as the user's default.
///
/// Falls back to an empty string rather than another guess: an empty default asks
/// the question, which is what a wizard is for.
pub(crate) fn default_user_name() -> String {
    env::var("USER")
        .or_else(|_| env::var("USERNAME"))
        .or_else(|_| env::var("LOGNAME"))
        .unwrap_or_default()
}

fn ask_user_preferences() -> Result<GlobalConfig> {
    let mut config = GlobalConfig::default_config();
    let default_name = default_user_name();
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
    // Only presets that can draft: a speech-only connector chosen here would
    // leave the configuration without a text default.
    let preset = Select::new("Default text connector", ConnectorPreset::text_capable()).prompt()?;
    let connector = preset.default_connector_name().to_string();
    // `default_config` ships only a subset of the presets, and
    // `GlobalConfig::validate` rejects a profile naming an absent connector, so
    // the chosen preset has to be configured before the profile references it.
    config
        .connectors
        .insert(connector.clone(), preset.into_config(&connector, None)?);
    let profile_name = preset.default_profile_name();
    let connector_name = connector.clone();
    let model = prompt("Default text model", preset.default_model())?;
    config.models.insert(
        profile_name.to_string(),
        ModelProfile {
            connector,
            model,
            capabilities: preset.default_capabilities().to_vec(),
            options: ModelOptions {
                // Preset-derived rather than hardcoded: Anthropic rejects
                // sampling parameters and shares `max_tokens` with thinking, so
                // a 4000-token cap there returns no text on the first run.
                text: TextModelOptions {
                    temperature: preset.default_text_temperature(),
                    max_tokens: preset.default_text_max_tokens(),
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
    if preset.requires_stored_login() {
        println!("Run `sfumato connector login {connector_name}` to store its API key.");
    }
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
