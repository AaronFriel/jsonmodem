use jsonmodem::{JsonValueFactory, ParseEvent, ParserOptions};
use jsonmodem_py::{PyFactory, PyStreamingParser};
use pyo3::{
    Python,
    types::{PyAnyMethods, PyDict, PyDictMethods, PyList, PyListMethods},
};

#[test]
fn streaming_parser_smoke() {
    let mut parser = PyStreamingParser::new(ParserOptions::default());
    for ev in parser.feed_with(PyFactory, "{\"a\":[1,2]}") {
        ev.unwrap();
    }
    for ev in parser.finish_with(PyFactory) {
        let event = ev.unwrap();
        if let ParseEvent::ObjectEnd {
            value: Some(obj), ..
        } = event
        {
            let mut f = PyFactory;
            let v = f.build_from_object(obj);
            Python::with_gil(|py| {
                let dict = v.0.bind(py).downcast::<PyDict>().unwrap();
                let item = dict.get_item("a").unwrap().unwrap();
                let list = item.downcast::<PyList>().unwrap();
                assert_eq!(list.len(), 2);
            });
        }
    }
}
