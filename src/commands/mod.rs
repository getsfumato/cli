use std::{collections::BTreeMap, str::FromStr};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;

use crate::{
    cli::{
        Commands, ConfigCommands, ConfigDeleteArgs, ConfigSetArgs, ConfigShowArgs,
        ConnectorCommands, ConnectorSetupArgs, ConnectorShowArgs, GenerateCommands,
        InitProjectArgs, InitTarget, ProjectCommands, ProjectNameArgs, ProjectShowArgs, SlidesArgs,
        ThemeCommands, ThemeUseArgs,
    },
    config::{Capability, ConfigOverrides, EffectiveConfig},
    config_editor::ConfigService,
    connectors::ConnectorService,
    generation::{GenerationRequest, ResourceKind},
    init::InitService,
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
            Self::Theme { command } => command.run().await,
            Self::Generate { command } => command.run().await,
        }
    }
}

#[async_trait]
impl RunnableCommand for ThemeCommands {
    async fn run(self) -> Result<()> {
        match self {
            Self::Create(args) => ThemeService::load()?.create(&args.name),
            Self::List => ThemeService::load()?.list(),
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
        ThemeService::load()?.use_for_project(&self.name, self.project.as_deref())
    }
}

#[async_trait]
impl RunnableCommand for ConnectorCommands {
    async fn run(self) -> Result<()> {
        match self {
            Self::List => {
                ConnectorService::load()?.list();
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
        ConnectorService::load()?.setup(self.preset, self.name, self.api_key_env)
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
        ProjectService::load()?.init(self.name, self.path, !self.no_activate)
    }
}

#[async_trait]
impl RunnableCommand for ProjectCommands {
    async fn run(self) -> Result<()> {
        match self {
            Self::List => {
                ProjectService::load()?.list();
                Ok(())
            }
            Self::Show(args) => args.run().await,
            Self::Use(args) => args.run().await,
            Self::Remove(args) => {
                let mut service = ProjectService::load()?;
                service.remove(&args.name)
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
        ProjectService::load()?.use_project(&self.name)
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
        let rendered = ConfigService::new()?.show(self.scope, self.project, self.key)?;
        println!("{rendered}");
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for ConfigSetArgs {
    async fn run(self) -> Result<()> {
        ConfigService::new()?.set(self.scope, self.project, &self.key, &self.value)
    }
}

#[async_trait]
impl RunnableCommand for ConfigDeleteArgs {
    async fn run(self) -> Result<()> {
        ConfigService::new()?.delete(self.scope, self.project, &self.key)
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
