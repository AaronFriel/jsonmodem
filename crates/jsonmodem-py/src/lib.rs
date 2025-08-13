use pyo3::prelude::*;

/// jsonmodem Python bindings
#[pymodule]
#[pyo3(name = "_jsonmodem")]
fn jsonmodem(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Expose package version from Cargo metadata
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;

    Ok(())
}
