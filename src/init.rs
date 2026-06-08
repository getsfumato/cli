use std::{env, fs, path::PathBuf};

use anyhow::{Context, Result, bail};
use indicatif::{ProgressBar, ProgressStyle};
use inquire::{Confirm, Select, Text};

use crate::config::{
    InferenceConfig, MarpConfig, OpenAiLikeProviderConfig, PROJECT_CONFIG_TEMPLATE, PartialConfig,
    ProvidersConfig, SfumatoConfig, USER_CONFIG_TEMPLATE, UserConfig, project_config_path,
    user_config_path,
};

#[derive(Debug)]
pub struct InitService {
    user_config_path: PathBuf,
    project_config_path: PathBuf,
    user_config_template: &'static str,
    project_config_template: &'static str,
}

impl InitService {
    pub fn new() -> Result<Self> {
        let user_config_path =
            user_config_path().context("Could not find a user configuration directory")?;

        Ok(Self {
            user_config_path,
            project_config_path: project_config_path(),
            user_config_template: USER_CONFIG_TEMPLATE,
            project_config_template: PROJECT_CONFIG_TEMPLATE,
        })
    }

    pub fn write_user_config(&self, yes: bool, force: bool) -> Result<()> {
        let path = &self.user_config_path;
        if path.exists() && !force {
            if yes
                || !confirm_with_default(
                    &format!("{} already exists. Overwrite it?", path.display()),
                    false,
                )?
            {
                bail!("User config already exists. Re-run with --force to overwrite it.");
            }
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Could not create {}", parent.display()))?;
        }

        let config = if yes {
            self.user_config_template.to_string()
        } else {
            println!("Let's set up your personal Sfumato preferences.");
            render_user_config_template(&ask_user_preferences()?)
        };

        write_with_spinner(path, config.as_bytes(), "Writing user config")?;
        Ok(())
    }

    pub fn write_project_config(&self) -> Result<()> {
        let path = &self.project_config_path;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Could not create {}", parent.display()))?;
        }

        write_with_spinner(
            path,
            self.project_config_template.as_bytes(),
            "Writing project config",
        )?;
        Ok(())
    }
}

#[derive(Debug)]
struct UserInitAnswers {
    name: String,
    learning_style: Vec<String>,
    theme: String,
    provider: String,
    model: String,
    temperature: f32,
    max_tokens: u32,
    openrouter_api_key_env: String,
    marp_theme: String,
    marp_pdf: bool,
}

fn ask_user_preferences() -> Result<UserInitAnswers> {
    let default_name = env::var("USER").unwrap_or_else(|_| "Alex".to_string());
    let name = prompt_with_default("Name", &default_name)?;
    let learning_style = prompt_list_with_default("Learning styles", &["visual", "step-by-step"])?;
    let theme = prompt_with_default("Sfumato theme", "sfumato-default")?;
    let provider = prompt_provider("Default provider", "ollama")?;
    let default_model = if provider == "openrouter" {
        "openai/gpt-4o-mini"
    } else {
        "llama3.2"
    };
    let model = prompt_with_default("Default model", default_model)?;
    let temperature = prompt_f32_with_default("Temperature", 0.4)?;
    let max_tokens = prompt_u32_with_default("Max tokens", 4000)?;
    let openrouter_api_key_env = prompt_with_default(
        "OpenRouter API key environment variable",
        "OPENROUTER_API_KEY",
    )?;
    let marp_theme = prompt_with_default("Marp theme", "default")?;
    let marp_pdf = confirm_with_default("Export PDFs with Marp by default?", false)?;

    Ok(UserInitAnswers {
        name,
        learning_style,
        theme,
        provider,
        model,
        temperature,
        max_tokens,
        openrouter_api_key_env,
        marp_theme,
        marp_pdf,
    })
}

fn render_user_config_template(answers: &UserInitAnswers) -> String {
    toml::to_string_pretty(&answers.to_user_config())
        .expect("serializing user init answers to TOML should not fail")
}

impl UserInitAnswers {
    fn to_user_config(&self) -> PartialConfig {
        let defaults = SfumatoConfig::default_for_cwd(PathBuf::from("."));

        PartialConfig {
            user: Some(UserConfig {
                name: Some(self.name.clone()),
                learning_style: self.learning_style.clone(),
                theme: self.theme.clone(),
            }),
            project: None,
            inference: Some(InferenceConfig {
                provider: self.provider.clone(),
                model: self.model.clone(),
                temperature: self.temperature,
                max_tokens: self.max_tokens,
            }),
            providers: Some(ProvidersConfig {
                ollama: OpenAiLikeProviderConfig {
                    base_url: defaults.providers.ollama.base_url,
                    api_key: defaults.providers.ollama.api_key,
                    api_key_env: defaults.providers.ollama.api_key_env,
                },
                openrouter: OpenAiLikeProviderConfig {
                    base_url: defaults.providers.openrouter.base_url,
                    api_key: defaults.providers.openrouter.api_key,
                    api_key_env: Some(self.openrouter_api_key_env.clone()),
                },
            }),
            marp: Some(MarpConfig {
                theme: self.marp_theme.clone(),
                pdf: self.marp_pdf,
            }),
        }
    }
}

fn prompt_with_default(label: &str, default: &str) -> Result<String> {
    Text::new(label)
        .with_default(default)
        .prompt()
        .with_context(|| format!("Could not read answer for {label}"))
}

fn prompt_list_with_default(label: &str, default: &[&str]) -> Result<Vec<String>> {
    let default_text = default.join(", ");
    let input = prompt_with_default(label, &default_text)?;
    let values = input
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    if values.is_empty() {
        bail!("{label} must include at least one value.");
    }

    Ok(values)
}

fn prompt_provider(label: &str, default: &str) -> Result<String> {
    let mut options = vec!["ollama".to_string(), "openrouter".to_string()];
    if default == "openrouter" {
        options.reverse();
    }

    Select::new(label, options)
        .prompt()
        .with_context(|| format!("Could not read answer for {label}"))
}

fn prompt_f32_with_default(label: &str, default: f32) -> Result<f32> {
    loop {
        let value = prompt_with_default(label, &default.to_string())?;
        match value.parse::<f32>() {
            Ok(parsed) => return Ok(parsed),
            Err(_) => println!("Please enter a number, for example 0.4."),
        }
    }
}

fn prompt_u32_with_default(label: &str, default: u32) -> Result<u32> {
    loop {
        let value = prompt_with_default(label, &default.to_string())?;
        match value.parse::<u32>() {
            Ok(parsed) => return Ok(parsed),
            Err(_) => println!("Please enter a whole number, for example 4000."),
        }
    }
}

fn confirm_with_default(label: &str, default: bool) -> Result<bool> {
    Confirm::new(label)
        .with_default(default)
        .prompt()
        .with_context(|| format!("Could not read confirmation for {label}"))
}

fn write_with_spinner(path: &PathBuf, contents: &[u8], message: &str) -> Result<()> {
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::with_template("{spinner:.cyan} {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_spinner()),
    );
    spinner.set_message(message.to_string());
    spinner.enable_steady_tick(std::time::Duration::from_millis(80));

    let result =
        fs::write(path, contents).with_context(|| format!("Could not write {}", path.display()));

    match result {
        Ok(()) => {
            spinner.finish_with_message(format!("Wrote {}", path.display()));
            Ok(())
        }
        Err(error) => {
            spinner.finish_and_clear();
            Err(error)
        }
    }
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../tests/unit/init.rs"]
mod tests;
