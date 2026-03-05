use notatok_core::midi::load_midi;
use notatok_core::tokenizer::remi::{RemiConfig, RemiTokenizer};
use notatok_core::tokenizer::Tokenizer;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "notatok", version, about = "Audio/MIDI tokenizer CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Tokenize a MIDI file into a sequence of integer token IDs
    Tokenize {
        /// Input .mid file
        input: PathBuf,

        /// Tokenization scheme
        #[arg(long, default_value = "remi")]
        scheme: String,

        /// Write token IDs as JSON to this file (prints to stdout if omitted)
        #[arg(long, short)]
        output: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Tokenize { input, scheme, output } => {
            let bytes = std::fs::read(&input)
                .with_context(|| format!("failed to read '{}'", input.display()))?;

            let score =
                load_midi(&bytes).with_context(|| "failed to parse MIDI file")?;

            let tokens: Vec<u32> = match scheme.as_str() {
                "remi" => {
                    let tokenizer = RemiTokenizer::new(RemiConfig::default());
                    tokenizer.encode(&score)?
                }
                s => anyhow::bail!("unknown scheme '{}' (supported: remi)", s),
            };

            let json = serde_json::to_string_pretty(&tokens)
                .context("failed to serialise token list")?;

            match output {
                Some(path) => {
                    std::fs::write(&path, &json)
                        .with_context(|| format!("failed to write '{}'", path.display()))?;
                    println!("wrote {} tokens to '{}'", tokens.len(), path.display());
                }
                None => println!("{json}"),
            }
        }
    }

    Ok(())
}
