use notatok_core::midi::{load_midi, save_midi};
use notatok_core::tokenizer::abc::{AbcConfig, AbcTokenizer};
use notatok_core::tokenizer::compound::{CompoundConfig, CompoundTokenizer};
use notatok_core::tokenizer::midi_like::{MidiLikeConfig, MidiLikeTokenizer};
use notatok_core::tokenizer::remi::{RemiConfig, RemiTokenizer};
use notatok_core::tokenizer::Tokenizer;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

fn make_tokenizer(scheme: &str) -> PyResult<Box<dyn Tokenizer>> {
    match scheme {
        "remi" => Ok(Box::new(RemiTokenizer::new(RemiConfig::default()))),
        "abc" => Ok(Box::new(AbcTokenizer::new(AbcConfig::default()))),
        "midi-like" => Ok(Box::new(MidiLikeTokenizer::new(MidiLikeConfig::default()))),
        "compound" => Ok(Box::new(CompoundTokenizer::new(CompoundConfig::default()))),
        s => Err(PyValueError::new_err(format!(
            "unknown scheme '{s}' (supported: remi, abc, midi-like, compound)"
        ))),
    }
}

/// Tokenize raw MIDI bytes into a flat list of integer token IDs.
///
/// Parameters
/// ----------
/// midi_bytes : bytes
///     Raw content of a `.mid` file.
/// scheme : str, optional
///     Tokenization scheme. One of ``"remi"`` (default), ``"abc"``,
///     ``"midi-like"``, or ``"compound"``.
///
/// Returns
/// -------
/// list[int]
///     Flat sequence of token IDs.
#[pyfunction]
#[pyo3(signature = (midi_bytes, scheme = "remi"))]
fn encode(midi_bytes: &[u8], scheme: &str) -> PyResult<Vec<u32>> {
    let score = load_midi(midi_bytes).map_err(|e| PyValueError::new_err(e.to_string()))?;
    let tokenizer = make_tokenizer(scheme)?;
    tokenizer.encode(&score).map_err(|e| PyValueError::new_err(e.to_string()))
}

/// Decode a token sequence back into raw MIDI bytes.
///
/// Decoding is approximate: quantisation and velocity binning are not
/// reversible, and multi-track information is not preserved. The returned
/// MIDI file uses 480 ticks/beat and 4/4 time.
///
/// Parameters
/// ----------
/// tokens : list[int]
///     Flat sequence of token IDs produced by :func:`encode`.
/// scheme : str, optional
///     Tokenization scheme used during encoding. Must match the scheme
///     passed to :func:`encode`. Defaults to ``"remi"``.
///
/// Returns
/// -------
/// bytes
///     Raw content of a valid `.mid` file (Format 0).
#[pyfunction]
#[pyo3(signature = (tokens, scheme = "remi"))]
fn decode(tokens: Vec<u32>, scheme: &str) -> PyResult<Vec<u8>> {
    let tokenizer = make_tokenizer(scheme)?;
    let score = tokenizer.decode(&tokens).map_err(|e| PyValueError::new_err(e.to_string()))?;
    save_midi(&score).map_err(|e| PyValueError::new_err(e.to_string()))
}

/// notatok — MIDI tokenization library.
///
/// Functions
/// ---------
/// encode(midi_bytes, scheme="remi") -> list[int]
/// decode(tokens, scheme="remi") -> bytes
#[pymodule]
fn notatok(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(encode, m)?)?;
    m.add_function(wrap_pyfunction!(decode, m)?)?;
    Ok(())
}
