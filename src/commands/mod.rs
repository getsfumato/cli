use std::{collections::BTreeMap, str::FromStr};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;

use crate::{
    cli::{
        Commands, ConfigCommands, ConfigDeleteArgs, ConfigSetArgs, ConfigShowArgs,
        ConnectorCommands, ConnectorSetupArgs, ConnectorShowArgs, GenerateCommands,
        InitProjectArgs, InitTarget, ModelAddArgs, ModelCommands, ModelEditArgs, ModelNameArgs,
        ModelUseArgs, ProjectCommands, ProjectNameArgs, ProjectShowArgs, SlidesArgs, ThemeCommands,
        ThemeUseArgs,
    },
    init::InitService,
};
use sfumato_core::{
    config::{Capability, ConfigOverrides, EffectiveConfig},
    config_editor::{ConfigService, ConfigTarget},
    connectors::{ConnectorPreset as CoreConnectorPreset, ConnectorService},
    generation::{GenerationRequest, ResourceKind},
    models::ModelService,
    projects::ProjectService,
    resources::slides::{GenerateSlidesOptions, generate_slides},
    themes::ThemeService,
};

#[async_trait]
pub trait RunnableCommand {
    async fn run(self) -> Result<()>;
}

#[async_trait]
impl RunnableCommand for Commands {
    async fn run(self) -> Result<()> {
        match self {
            Self::Init { target } => target.run().await,
            Self::Config { command } => command.run().await,
            Self::Project { command } => command.run().await,
            Self::Connector { command } => command.run().await,
            Self::Model { command } => command.run().await,
            Self::Theme { command } => command.run().await,
            Self::Generate { command } => command.run().await,
        }
    }
}

#[async_trait]
impl RunnableCommand for ModelCommands {
    async fn run(self) -> Result<()> {
        match self {
            Self::Add(args) => args.run().await,
            Self::Edit(args) => args.run().await,
            Self::List => {
                let models = ModelService::load()?.list();
                if models.is_empty() {
                    println!("No registered model profiles.");
                }
                for model in models {
                    let capabilities = model
                        .capabilities
                        .iter()
                        .map(|capability| capability.as_str())
                        .collect::<Vec<_>>()
                        .join(",");
                    println!(
                        "{}\t{}\t{}\t{capabilities}",
                        model.name, model.connector, model.model
                    );
                }
                Ok(())
            }
            Self::Show(args) => args.run().await,
            Self::Remove(args) => {
                let mut service = ModelService::load()?;
                let name = service.remove(&args.name)?;
                println!("Removed model profile '{name}'");
                Ok(())
            }
            Self::Use(args) => args.run().await,
        }
    }
}

#[async_trait]
impl RunnableCommand for ModelEditArgs {
    async fn run(self) -> Result<()> {
        ModelService::load()?.edit(
            &self.name,
            self.connector,
            self.model_id,
            self.capabilities,
            self.options,
        )?;
        println!("Updated model profile '{}'", self.name);
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for ModelAddArgs {
    async fn run(self) -> Result<()> {
        let name = self.name.clone();
        ModelService::load()?.add(
            self.name,
            self.connector,
            self.model_id,
            self.capabilities,
            self.options,
        )?;
        println!("Added model profile '{name}'");
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for ModelNameArgs {
    async fn run(self) -> Result<()> {
        println!("{}", ModelService::load()?.show(&self.name)?);
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for ModelUseArgs {
    async fn run(self) -> Result<()> {
        let changed = ModelService::load()?.use_default(
            &self.capability,
            &self.profile,
            self.project.as_deref(),
        )?;
        if let Some(project) = changed.project {
            println!(
                "Project '{project}' now uses model profile '{}' for '{}'",
                changed.profile,
                changed.capability.as_str()
            );
        } else {
            println!(
                "User default for '{}' is now model profile '{}'",
                changed.capability.as_str(),
                changed.profile
            );
        }
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for ThemeCommands {
    async fn run(self) -> Result<()> {
        match self {
            Self::Create(args) => {
                let theme = ThemeService::load()?.create(&args.name)?;
                println!(
                    "Created theme '{}' at {}",
                    theme.manifest.name,
                    theme.root.display()
                );
                Ok(())
            }
            Self::List => {
                for theme in ThemeService::load()?.list()? {
                    println!("{}", theme.name);
                }
                Ok(())
            }
            Self::Show(args) => {
                println!("{}", ThemeService::load()?.show(&args.name)?);
                Ok(())
            }
            Self::Use(args) => args.run().await,
        }
    }
}

#[async_trait]
impl RunnableCommand for ThemeUseArgs {
    async fn run(self) -> Result<()> {
        let project = ThemeService::load()?.use_for_project(&self.name, self.project.as_deref())?;
        println!(
            "Project '{}' now uses theme '{}'",
            project.name, project.theme
        );
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for ConnectorCommands {
    async fn run(self) -> Result<()> {
        match self {
            Self::List => {
                for connector in ConnectorService::load()?.list() {
                    println!("{}\t{}", connector.name, connector.base_url);
                }
                Ok(())
            }
            Self::Show(args) => args.run().await,
            Self::Setup(args) => args.run().await,
        }
    }
}

#[async_trait]
impl RunnableCommand for ConnectorShowArgs {
    async fn run(self) -> Result<()> {
        println!("{}", ConnectorService::load()?.show(&self.name)?);
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for ConnectorSetupArgs {
    async fn run(self) -> Result<()> {
        let preset = match self.preset {
            crate::cli::ConnectorPreset::Ollama => CoreConnectorPreset::Ollama,
            crate::cli::ConnectorPreset::Openrouter => CoreConnectorPreset::Openrouter,
        };
        let connector = ConnectorService::load()?.setup(preset, self.name, self.api_key_env)?;
        println!(
            "Configured OpenAI-compatible connector '{}'",
            connector.name
        );
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for InitTarget {
    async fn run(self) -> Result<()> {
        match self {
            Self::User { yes, force } => InitService::new()?.write_user_config(yes, force),
            Self::Project(args) => args.run().await,
        }
    }
}

#[async_trait]
impl RunnableCommand for InitProjectArgs {
    async fn run(self) -> Result<()> {
        let project = ProjectService::load()?.init(self.name, self.path, !self.no_activate)?;
        println!("Initialized project '{}'", project.name);
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for ProjectCommands {
    async fn run(self) -> Result<()> {
        match self {
            Self::List => {
                let projects = ProjectService::load()?.list()?;
                if projects.is_empty() {
                    println!("No registered projects.");
                }
                for project in projects {
                    let marker = if project.active { "*" } else { " " };
                    println!("{marker} {}\t{}", project.name, project.path.display());
                }
                Ok(())
            }
            Self::Show(args) => args.run().await,
            Self::Use(args) => args.run().await,
            Self::Remove(args) => {
                let service = ProjectService::load()?;
                let removed = service.remove(&args.name)?;
                println!(
                    "Removed project '{}' from the registry; project files were kept.",
                    removed.name
                );
                Ok(())
            }
        }
    }
}

#[async_trait]
impl RunnableCommand for ProjectShowArgs {
    async fn run(self) -> Result<()> {
        println!("{}", ProjectService::load()?.show(self.name.as_deref())?);
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for ProjectNameArgs {
    async fn run(self) -> Result<()> {
        let name = ProjectService::load()?.use_project(&self.name)?;
        println!("Active project: {name}");
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for ConfigCommands {
    async fn run(self) -> Result<()> {
        match self {
            Self::Show(args) => args.run().await,
            Self::Set(args) => args.run().await,
            Self::Delete(args) => args.run().await,
        }
    }
}

#[async_trait]
impl RunnableCommand for ConfigShowArgs {
    async fn run(self) -> Result<()> {
        let rendered =
            ConfigService::new()?.show(config_target(self.scope), self.project, self.key)?;
        println!("{rendered}");
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for ConfigSetArgs {
    async fn run(self) -> Result<()> {
        let path = ConfigService::new()?.set(
            config_target(self.scope),
            self.project,
            &self.key,
            &self.value,
        )?;
        println!("Set {} in {}", self.key, path.display());
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for ConfigDeleteArgs {
    async fn run(self) -> Result<()> {
        let path =
            ConfigService::new()?.delete(config_target(self.scope), self.project, &self.key)?;
        println!("Deleted {} from {}", self.key, path.display());
        Ok(())
    }
}

fn config_target(scope: crate::cli::ConfigScope) -> ConfigTarget {
    match scope {
        crate::cli::ConfigScope::User => ConfigTarget::User,
        crate::cli::ConfigScope::Project => ConfigTarget::Project,
        crate::cli::ConfigScope::Effective => ConfigTarget::Effective,
    }
}

#[async_trait]
impl RunnableCommand for GenerateCommands {
    async fn run(self) -> Result<()> {
        match self {
            Self::Slides(args) => args.run().await,
        }
    }
}

#[async_trait]
impl RunnableCommand for SlidesArgs {
    async fn run(self) -> Result<()> {
        let json = self.json;
        match self.execute().await {
            Ok(()) => Ok(()),
            Err(error) if json => {
                println!("{}", serde_json::json!({ "error": format!("{error:#}") }));
                Err(error)
            }
            Err(error) => Err(error),
        }
    }
}

impl SlidesArgs {
    async fn execute(self) -> Result<()> {
        if self.instruction.trim().is_empty() {
            bail!("Instruction cannot be empty");
        }
        let model_overrides = parse_model_overrides(&self.model_overrides)?;
        let config = EffectiveConfig::load(ConfigOverrides {
            project: self.project.clone(),
            theme: self.theme,
            model_overrides: model_overrides.clone(),
            output_dir: self.out,
            pdf: self.pdf,
        })?;
        let request = GenerationRequest {
            instruction: self.instruction,
            sources: self.inputs,
            resource_kind: ResourceKind::Slides,
            project: self.project,
            model_overrides,
        };
        let result = generate_slides(
            config,
            request,
            GenerateSlidesOptions {
                title: self.title,
                dry_run: self.dry_run,
            },
        )
        .await?;
        for warning in &result.warnings {
            eprintln!("{warning}");
        }

        if self.json {
            println!("{}", serde_json::to_string_pretty(&result.output)?);
        } else if self.dry_run {
            if let Some(prompt) = result.prompt_preview {
                println!("{prompt}");
            }
            println!("Dry run complete; no files were written.");
        } else {
            println!("Wrote {}", result.markdown_path.display());
            if let Some(pdf_path) = result.pdf_path {
                println!("Wrote {}", pdf_path.display());
            }
        }
        Ok(())
    }
}

fn parse_model_overrides(values: &[String]) -> Result<BTreeMap<Capability, String>> {
    values
        .iter()
        .map(|value| {
            let (capability, profile) = value.split_once('=').with_context(|| {
                format!("Invalid model override '{value}'. Use capability=profile.")
            })?;
            if profile.trim().is_empty() {
                bail!("Model profile cannot be empty in '{value}'");
            }
            Ok((
                Capability::from_str(capability.trim())?,
                profile.trim().to_string(),
            ))
        })
        .collect()
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../../tests/unit/commands.rs"]
mod tests;
