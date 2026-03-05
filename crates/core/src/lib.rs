use thiserror::Error;

pub mod midi;
pub mod tokenizer;

pub use midi::{Note, Score, TempoChange, TimeSignatureChange, Track};
pub use tokenizer::Tokenizer;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("MIDI parse error: {0}")]
    MidiParse(String),

    #[error("tokenizer error: {0}")]
    Tokenizer(String),

    #[error("unknown token id: {0}")]
    UnknownTokenId(u32),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, CoreError>;
