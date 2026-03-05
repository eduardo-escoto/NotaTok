use notatok_core::midi::{load_midi, save_midi};
use notatok_core::tokenizer::abc::{AbcConfig, AbcTokenizer};
use notatok_core::tokenizer::compound::{CompoundConfig, CompoundTokenizer};
use notatok_core::tokenizer::midi_like::{MidiLikeConfig, MidiLikeTokenizer};
use notatok_core::tokenizer::remi::{RemiConfig, RemiTokenizer};
use notatok_core::tokenizer::Tokenizer;

use std::io::Write;
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

        /// Tokenization scheme (remi, abc, midi-like, compound)
        #[arg(long, default_value = "remi")]
        scheme: String,

        /// Write token IDs as JSON to this file (prints to stdout if omitted)
        #[arg(long, short)]
        output: Option<PathBuf>,
    },

    /// Decode a JSON token file back into a MIDI file
    Decode {
        /// Input JSON file containing a flat array of token IDs
        input: PathBuf,

        /// Tokenization scheme used when encoding (remi, abc, midi-like, compound)
        #[arg(long, default_value = "remi")]
        scheme: String,

        /// Write the decoded MIDI to this file (writes binary to stdout if omitted)
        #[arg(long, short)]
        output: Option<PathBuf>,
    },
}

/// Build a tokenizer and run `encode` for the given scheme.
fn encode_score(
    score: &notatok_core::midi::Score,
    scheme: &str,
) -> Result<Vec<u32>> {
    let tokens = match scheme {
        "remi" => RemiTokenizer::new(RemiConfig::default()).encode(score)?,
        "abc" => AbcTokenizer::new(AbcConfig::default()).encode(score)?,
        "midi-like" => MidiLikeTokenizer::new(MidiLikeConfig::default()).encode(score)?,
        "compound" => CompoundTokenizer::new(CompoundConfig::default()).encode(score)?,
        s => anyhow::bail!("unknown scheme '{s}' (supported: remi, abc, midi-like, compound)"),
    };
    Ok(tokens)
}

/// Build a tokenizer and run `decode` for the given scheme.
fn decode_tokens(
    tokens: &[u32],
    scheme: &str,
) -> Result<notatok_core::midi::Score> {
    let score = match scheme {
        "remi" => RemiTokenizer::new(RemiConfig::default()).decode(tokens)?,
        "abc" => AbcTokenizer::new(AbcConfig::default()).decode(tokens)?,
        "midi-like" => MidiLikeTokenizer::new(MidiLikeConfig::default()).decode(tokens)?,
        "compound" => CompoundTokenizer::new(CompoundConfig::default()).decode(tokens)?,
        s => anyhow::bail!("unknown scheme '{s}' (supported: remi, abc, midi-like, compound)"),
    };
    Ok(score)
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Tokenize { input, scheme, output } => {
            let bytes = std::fs::read(&input)
                .with_context(|| format!("failed to read '{}'", input.display()))?;

            let score = load_midi(&bytes).context("failed to parse MIDI file")?;
            let tokens = encode_score(&score, &scheme)?;

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

        Command::Decode { input, scheme, output } => {
            let json = std::fs::read_to_string(&input)
                .with_context(|| format!("failed to read '{}'", input.display()))?;

            let tokens: Vec<u32> = serde_json::from_str(&json)
                .context("failed to parse token JSON (expected a flat array of integers)")?;

            let score = decode_tokens(&tokens, &scheme)?;
            let midi_bytes = save_midi(&score).context("failed to serialise MIDI")?;

            match output {
                Some(path) => {
                    std::fs::write(&path, &midi_bytes)
                        .with_context(|| format!("failed to write '{}'", path.display()))?;
                    println!(
                        "decoded {} tokens → {} bytes → '{}'",
                        tokens.len(),
                        midi_bytes.len(),
                        path.display()
                    );
                }
                None => {
                    std::io::stdout()
                        .write_all(&midi_bytes)
                        .context("failed to write MIDI to stdout")?;
                }
            }
        }
    }

    Ok(())
}
