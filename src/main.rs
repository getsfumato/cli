mod cli;
mod commands;
mod init;
mod menu;

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
