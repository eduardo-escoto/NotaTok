use notatok_core as core;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

/// Greet someone by name.
#[pyfunction]
fn greet(name: &str) -> PyResult<String> {
    core::greet(name).map_err(|e| PyValueError::new_err(e.to_string()))
}

/// notatok Python extension module.
#[pymodule]
fn notatok(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(greet, m)?)?;
    Ok(())
}
