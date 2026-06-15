use std::{collections::BTreeMap, env};

use anyhow::{Context, Result, bail};
use indicatif::{ProgressBar, ProgressStyle};
use inquire::{Confirm, Select, Text};

use sfumato_core::config::{Capability, GlobalConfig, ModelDefaults, ModelProfile};
use sfumato_core::setup::{SetupService, UserSetupRequest};

pub struct InitService {
    setup: SetupService,
}

impl InitService {
    pub fn new() -> Result<Self> {
        Ok(Self {
            setup: SetupService::load()?,
        })
    }

    pub fn write_user_config(&self, yes: bool, force: bool) -> Result<()> {
        if self.setup.user_config_exists()
            && !force
            && (yes
                || !Confirm::new(&format!(
                    "{} already exists. Overwrite it?",
                    self.setup.user_config_path().display()
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
        let result = self.setup.setup_user(UserSetupRequest { config })?;
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
    let connector = Select::new(
        "Default text connector",
        vec!["ollama".to_string(), "openrouter".to_string()],
    )
    .prompt()?;
    let (profile_name, default_model) = if connector == "openrouter" {
        ("cloud-text", "openai/gpt-4o-mini")
    } else {
        ("local-text", "llama3.2")
    };
    let model = prompt("Default text model", default_model)?;
    config.models.insert(
        profile_name.to_string(),
        ModelProfile {
            connector,
            model,
            capabilities: vec![Capability::Text, Capability::Code],
            options: BTreeMap::from([
                ("temperature".to_string(), toml::Value::Float(0.4)),
                ("max_tokens".to_string(), toml::Value::Integer(4000)),
            ]),
        },
    );
    config.defaults = ModelDefaults(BTreeMap::from([(
        Capability::Text,
        profile_name.to_string(),
    )]));
    config
        .connectors
        .get_mut("openrouter")
        .expect("default OpenRouter connector must exist")
        .api_key_env = Some(prompt(
        "OpenRouter API key environment variable",
        "OPENROUTER_API_KEY",
    )?);
    config.marp.pdf = Confirm::new("Export PDFs with Marp by default?")
        .with_default(false)
        .prompt()?;
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
