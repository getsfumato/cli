use anyhow::Result;
use async_trait::async_trait;

use crate::{
    cli::{
        Commands, ConfigCommands, ConfigDeleteArgs, ConfigSetArgs, ConfigShowArgs,
        GenerateCommands, InitTarget, SlidesArgs,
    },
    config::{ConfigOverrides, SfumatoConfig},
    config_editor::ConfigService,
    init,
    providers::ProviderKind,
    resources::slides::{GenerateSlidesOptions, generate_slides},
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
            Self::Generate { command } => command.run().await,
        }
    }
}

#[async_trait]
impl RunnableCommand for InitTarget {
    async fn run(self) -> Result<()> {
        let init_service = init::InitService::new()?;

        match self {
            Self::User { yes, force } => init_service.write_user_config(yes, force),
            Self::Project => init_service.write_project_config(),
        }
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
        let service = ConfigService::new()?;
        let rendered = service.show(self.scope, self.key)?;
        println!("{rendered}");
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for ConfigSetArgs {
    async fn run(self) -> Result<()> {
        let service = ConfigService::new()?;
        service.set(self.scope, &self.key, &self.value)?;
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for ConfigDeleteArgs {
    async fn run(self) -> Result<()> {
        let service = ConfigService::new()?;
        service.delete(self.scope, &self.key)?;
        Ok(())
    }
}

#[async_trait]
impl RunnableCommand for SlidesArgs {
    async fn run(self) -> Result<()> {
        let SlidesArgs {
            inputs,
            provider,
            model,
            title,
            out,
            pdf,
            dry_run,
            config,
        } = self;

        let overrides = ConfigOverrides {
            provider,
            model,
            output_dir: out,
            pdf,
            config_path: config,
        };
        let config = SfumatoConfig::load(overrides)?;
        let provider_kind = ProviderKind::from_config(&config)?;

        let options = GenerateSlidesOptions {
            inputs,
            title,
            dry_run,
            provider_kind,
        };

        let result = generate_slides(config, options).await?;
        if dry_run {
            println!("Dry run complete; no files were written.");
            return Ok(());
        }

        println!("Wrote {}", result.markdown_path.display());

        if let Some(pdf_path) = result.pdf_path {
            println!("Wrote {}", pdf_path.display());
        }

        Ok(())
    }
}
