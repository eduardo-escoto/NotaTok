use anyhow::Result;
use clap::{Parser, Subcommand};
use notatok_core as core;

#[derive(Parser)]
#[command(name = "notatok", version, about = "notatok CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Greet someone
    Greet {
        /// Name to greet
        name: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Greet { name } => {
            let msg = core::greet(&name)?;
            println!("{msg}");
        }
    }

    Ok(())
}
