mod cli;
mod commands;
mod init;
mod tui;

use anyhow::Result;
use clap::Parser;
use cli::Cli;
use commands::RunnableCommand;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(command) => command.run().await,
        None => tui::run().await,
    }
}
