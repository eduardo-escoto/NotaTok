use notatok_core::midi::load_midi;
use notatok_core::tokenizer::remi::{RemiConfig, RemiTokenizer};
use notatok_core::tokenizer::Tokenizer;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

/// Tokenize raw MIDI bytes using the default REMI scheme.
///
/// Returns a list of integer token IDs.
#[pyfunction]
fn encode(midi_bytes: &[u8]) -> PyResult<Vec<u32>> {
    let score =
        load_midi(midi_bytes).map_err(|e| PyValueError::new_err(e.to_string()))?;
    let tokenizer = RemiTokenizer::new(RemiConfig::default());
    tokenizer
        .encode(&score)
        .map_err(|e| PyValueError::new_err(e.to_string()))
}

/// notatok Python extension module.
#[pymodule]
fn notatok(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(encode, m)?)?;
    Ok(())
}
