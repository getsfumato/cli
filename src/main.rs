mod cli;
mod commands;
mod init;
mod presentation;
mod tui;

use anyhow::Result;
use clap::{CommandFactory, Parser};
use cli::Cli;
use commands::RunnableCommand;
use sfumato_adapters::application::production_application;
use std::{io::IsTerminal, sync::Arc};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let application = Arc::new(production_application()?);
    match cli.command {
        Some(command) => command.run(Arc::clone(&application)).await,
        None if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() => {
            tui::run(application).await
        }
        None => {
            Cli::command().print_help()?;
            println!();
            Ok(())
        }
    }
}
