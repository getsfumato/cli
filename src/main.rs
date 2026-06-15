mod cli;
mod commands;
mod config;
mod config_editor;
mod connectors;
mod generation;
mod init;
mod menu;
mod models;
mod projects;
mod providers;
mod renderers;
mod resources;
mod themes;

use anyhow::Result;
use clap::Parser;
use cli::Cli;
use commands::RunnableCommand;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(command) => command.run().await,
        None => {
            menu::welcome();
            while let Some(command) = menu::next_command()? {
                if let Err(error) = command.run().await {
                    eprintln!("Error: {error:#}\n");
                } else {
                    println!();
                }
            }
            Ok(())
        }
    }
}
