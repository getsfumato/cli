mod cli;
mod commands;
mod init;
mod tui;

use anyhow::Result;
use clap::{CommandFactory, Parser};
use cli::Cli;
use commands::RunnableCommand;
use std::io::IsTerminal;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(command) => command.run().await,
        None if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() => {
            tui::run().await
        }
        None => {
            Cli::command().print_help()?;
            println!();
            Ok(())
        }
    }
}
