use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(name = "sfumato")]
#[command(about = "Generate Obsidian-friendly learning resources with local or cloud models.")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
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
    Project {
        #[command(subcommand)]
        command: ProjectCommands,
    },
    Connector {
        #[command(subcommand)]
        command: ConnectorCommands,
    },
    Model {
        #[command(subcommand)]
        command: ModelCommands,
    },
    Theme {
        #[command(subcommand)]
        command: ThemeCommands,
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
    Project(InitProjectArgs),
}

#[derive(Debug, Subcommand)]
pub enum GenerateCommands {
    Slides(SlidesArgs),
}

#[derive(Debug, Subcommand)]
pub enum ProjectCommands {
    List,
    Show(ProjectShowArgs),
    Use(ProjectNameArgs),
    Remove(ProjectNameArgs),
}

#[derive(Debug, Subcommand)]
pub enum ConnectorCommands {
    List,
    Show(ConnectorShowArgs),
    Setup(ConnectorSetupArgs),
}

#[derive(Debug, Subcommand)]
pub enum ModelCommands {
    Add(ModelAddArgs),
    Edit(ModelEditArgs),
    List,
    Show(ModelNameArgs),
    Remove(ModelNameArgs),
    Use(ModelUseArgs),
}

#[derive(Debug, Args)]
pub struct ModelEditArgs {
    pub name: String,

    #[arg(long)]
    pub connector: Option<String>,

    #[arg(long = "id", help = "Provider-specific model ID")]
    pub model_id: Option<String>,

    #[arg(long = "capability", help = "Replace the profile capabilities")]
    pub capabilities: Vec<String>,

    #[arg(
        long = "option",
        value_name = "KEY=VALUE",
        help = "Set or replace model options"
    )]
    pub options: Vec<String>,
}

#[derive(Debug, Args)]
pub struct ModelAddArgs {
    pub name: String,

    #[arg(long)]
    pub connector: String,

    #[arg(long = "id", help = "Provider-specific model ID")]
    pub model_id: String,

    #[arg(long = "capability", required = true)]
    pub capabilities: Vec<String>,

    #[arg(long = "option", value_name = "KEY=VALUE")]
    pub options: Vec<String>,
}

#[derive(Debug, Args)]
pub struct ModelNameArgs {
    pub name: String,
}

#[derive(Debug, Args)]
pub struct ModelUseArgs {
    #[arg(help = "Capability or model role, for example text or reviewer")]
    pub selector: String,
    pub profile: String,

    #[arg(long, help = "Assign the default to this project instead of the user")]
    pub project: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum ThemeCommands {
    Create(ThemeNameArgs),
    List,
    Show(ThemeNameArgs),
    Use(ThemeUseArgs),
}

#[derive(Debug, Args)]
pub struct ThemeNameArgs {
    pub name: String,
}

#[derive(Debug, Args)]
pub struct ThemeUseArgs {
    pub name: String,

    #[arg(long)]
    pub project: Option<String>,
}

#[derive(Debug, Args)]
pub struct ConnectorShowArgs {
    pub name: String,
}

#[derive(Debug, Args)]
pub struct ConnectorSetupArgs {
    #[arg(value_enum)]
    pub preset: ConnectorPreset,

    #[arg(long, help = "Connector name; defaults to the preset name")]
    pub name: Option<String>,

    #[arg(long, default_value = "OPENROUTER_API_KEY")]
    pub api_key_env: String,
}

#[derive(Debug, Args)]
pub struct InitProjectArgs {
    pub name: String,

    #[arg(long, default_value = ".")]
    pub path: PathBuf,

    #[arg(long)]
    pub no_activate: bool,
}

#[derive(Debug, Args)]
pub struct ProjectShowArgs {
    pub name: Option<String>,
}

#[derive(Debug, Args)]
pub struct ProjectNameArgs {
    pub name: String,
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
    #[arg(help = "Optional dotted key, for example defaults.text")]
    pub key: Option<String>,

    #[arg(long, value_enum, default_value_t = ConfigScope::Effective, help = "Config scope to read from")]
    pub scope: ConfigScope,

    #[arg(long, help = "Project name when reading project or effective config")]
    pub project: Option<String>,
}

#[derive(Debug, Args)]
pub struct ConfigSetArgs {
    #[arg(help = "Dotted key to set, for example inference.model")]
    pub key: String,
    #[arg(help = "TOML value, or a plain string if TOML parsing fails")]
    pub value: String,

    #[arg(long, value_enum, default_value_t = ConfigScope::User, help = "Config file to edit")]
    pub scope: ConfigScope,

    #[arg(long, help = "Project name when editing project config")]
    pub project: Option<String>,
}

#[derive(Debug, Args)]
pub struct ConfigDeleteArgs {
    #[arg(help = "Dotted key to delete, for example defaults.text")]
    pub key: String,

    #[arg(long, value_enum, default_value_t = ConfigScope::User, help = "Config file to edit")]
    pub scope: ConfigScope,

    #[arg(long, help = "Project name when editing project config")]
    pub project: Option<String>,
}

#[derive(Clone, Debug, Args)]
pub struct SlidesArgs {
    pub inputs: Vec<PathBuf>,

    #[arg(long, required = true)]
    pub instruction: String,

    #[arg(long)]
    pub title: Option<String>,

    #[arg(
        long,
        help = "Publish the rendered PDF to this folder; working artifacts remain in Sfumato's project workspace"
    )]
    pub out: Option<PathBuf>,

    #[arg(long, hide = true)]
    pub pdf: bool,

    #[arg(long)]
    pub dry_run: bool,

    #[arg(long)]
    pub project: Option<String>,

    #[arg(long)]
    pub theme: Option<String>,

    #[arg(long = "model", value_name = "CAPABILITY=PROFILE")]
    pub model_overrides: Vec<String>,

    #[arg(long, value_name = "PROFILE")]
    pub review_model: Option<String>,

    #[arg(long)]
    pub no_review: bool,

    #[arg(long)]
    pub json: bool,
}

#[derive(Clone, Debug, ValueEnum)]
pub enum ConfigScope {
    User,
    Project,
    Effective,
}

#[derive(Clone, Debug, ValueEnum)]
pub enum ConnectorPreset {
    Ollama,
    Openrouter,
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests inspect private CLI types.
#[path = "../tests/unit/cli.rs"]
mod tests;
