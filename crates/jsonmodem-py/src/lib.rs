mod pyfactory;
mod pyvalue;

pub use pyfactory::PyFactory;
pub use pyvalue::PyJsonValue;

use ::jsonmodem::StreamingParserImpl;
pub type PyStreamingParser = StreamingParserImpl<PyJsonValue>;

use pyo3::prelude::*;

#[pymodule]
pub fn jsonmodem(_py: Python<'_>, _m: &Bound<'_, PyModule>) -> PyResult<()> {
    Ok(())
}
