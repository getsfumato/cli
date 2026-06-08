mod cli;
mod commands;
mod config;
mod config_editor;
mod init;
mod providers;
mod renderers;
mod resources;

use anyhow::Result;
use clap::Parser;
use cli::Cli;
use commands::RunnableCommand;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    cli.command.run().await
}
