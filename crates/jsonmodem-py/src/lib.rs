mod pyfactory;
mod pyvalue;

use ::jsonmodem::{StreamingParserImpl, StreamingValuesParserImpl};
pub use pyfactory::PyFactory;
pub use pyvalue::PyJsonValue;
pub type PyStreamingParser = StreamingParserImpl<PyJsonValue>;
pub type PyStreamingValuesParser = StreamingValuesParserImpl<PyJsonValue>;

use pyo3::prelude::*;

#[pymodule]
pub fn jsonmodem(_py: Python<'_>, _m: &Bound<'_, PyModule>) -> PyResult<()> {
    Ok(())
}
