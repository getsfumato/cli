use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(name = "sfumato")]
#[command(about = "Generate Obsidian-friendly learning resources with local or cloud models.")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    Init {
        #[command(subcommand)]
        target: InitTarget,
    },
    #[command(about = "Show and edit Sfumato configuration")]
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
    Generate {
        #[command(subcommand)]
        command: GenerateCommands,
    },
}

#[derive(Debug, Subcommand)]
pub enum InitTarget {
    User {
        #[arg(long, help = "Use default values without asking setup questions")]
        yes: bool,

        #[arg(long, help = "Overwrite an existing user config")]
        force: bool,
    },
    Project,
}

#[derive(Debug, Subcommand)]
pub enum GenerateCommands {
    Slides(SlidesArgs),
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommands {
    #[command(about = "Show the effective, user, or project config")]
    Show(ConfigShowArgs),
    #[command(about = "Set a config value by dotted key")]
    Set(ConfigSetArgs),
    #[command(about = "Delete a config value by dotted key")]
    Delete(ConfigDeleteArgs),
}

#[derive(Debug, Args)]
pub struct ConfigShowArgs {
    #[arg(help = "Optional dotted key, for example user.theme")]
    pub key: Option<String>,

    #[arg(long, value_enum, default_value_t = ConfigScope::Effective, help = "Config scope to read from")]
    pub scope: ConfigScope,
}

#[derive(Debug, Args)]
pub struct ConfigSetArgs {
    #[arg(help = "Dotted key to set, for example inference.model")]
    pub key: String,
    #[arg(help = "TOML value, or a plain string if TOML parsing fails")]
    pub value: String,

    #[arg(long, value_enum, default_value_t = ConfigScope::User, help = "Config file to edit")]
    pub scope: ConfigScope,
}

#[derive(Debug, Args)]
pub struct ConfigDeleteArgs {
    #[arg(help = "Dotted key to delete, for example user.theme")]
    pub key: String,

    #[arg(long, value_enum, default_value_t = ConfigScope::User, help = "Config file to edit")]
    pub scope: ConfigScope,
}

#[derive(Debug, Args)]
pub struct SlidesArgs {
    #[arg(required = true)]
    pub inputs: Vec<PathBuf>,

    #[arg(long, value_enum)]
    pub provider: Option<ProviderArg>,

    #[arg(long)]
    pub model: Option<String>,

    #[arg(long)]
    pub title: Option<String>,

    #[arg(long)]
    pub out: Option<PathBuf>,

    #[arg(long)]
    pub pdf: bool,

    #[arg(long)]
    pub dry_run: bool,

    #[arg(long)]
    pub config: Option<PathBuf>,
}

#[derive(Clone, Debug, ValueEnum)]
pub enum ProviderArg {
    Ollama,
    Openrouter,
}

#[derive(Clone, Debug, ValueEnum)]
pub enum ConfigScope {
    User,
    Project,
    Effective,
}
