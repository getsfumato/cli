mod cli;
mod commands;
mod init;
mod presentation;
mod tui;

use anyhow::{Result, bail};
use clap::{CommandFactory, Parser};
use cli::Cli;
use commands::{RunnableCommand, operation::set_timeout};
use sfumato_adapters::application::production_application;
use std::{io::IsTerminal, sync::Arc};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    // Rejected rather than treated as "no deadline": `--timeout 0` reads as an
    // instruction to give up immediately, and silently ignoring it would run an
    // unbounded generation the caller believed was bounded.
    if cli.timeout == Some(0) {
        bail!("--timeout must be at least 1 second; omit it to run without a deadline");
    }
    set_timeout(cli.timeout);
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
