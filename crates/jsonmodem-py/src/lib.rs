use pyo3::prelude::*;

#[pymodule]
pub fn jsonmodem(_py: Python<'_>, _m: &Bound<'_, PyModule>) -> PyResult<()> {
    // empty for now
    Ok(())
}
