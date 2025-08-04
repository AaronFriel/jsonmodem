use jsonmodem::{
    JsonValueFactory, NonScalarValueMode, ParserOptions, StreamingValuesParser, StringValueMode,
    Value,
};
use jsonmodem_py::{PyFactory, PyJsonValue};
use pyo3::{
    Python,
    types::{PyAnyMethods, PyDict, PyDictMethods},
};

const MEDIUM_JSON: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../jsonmodem/benches/jiter_data/medium_response.json"
);

const LARGE_JSON: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../jsonmodem/benches/jiter_data/response_large.json"
);

fn value_to_py(v: Value, f: &mut PyFactory) -> PyJsonValue {
    match v {
        Value::Null => f.build_from_null(()),
        Value::Boolean(b) => f.build_from_bool(b),
        Value::Number(n) => f.build_from_num(n),
        Value::String(s) => f.build_from_str(s),
        Value::Array(a) => {
            let items = a.into_iter().map(|v| value_to_py(v, f)).collect();
            f.build_from_array(items)
        }
        Value::Object(o) => {
            let items = o.into_iter().map(|(k, v)| (k, value_to_py(v, f))).collect();
            f.build_from_object(items)
        }
    }
}

fn parse_payload(payload: &str) -> PyJsonValue {
    let mut parser = StreamingValuesParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::Roots,
        string_value_mode: StringValueMode::Values,
        ..Default::default()
    });
    let mut out = parser.feed(payload).unwrap();
    out.extend(parser.finish().unwrap());
    let value = out.into_iter().find(|v| v.is_final).unwrap().value;
    let mut f = PyFactory;
    value_to_py(value, &mut f)
}

#[test]
fn streaming_values_medium() {
    let payload = std::fs::read_to_string(MEDIUM_JSON).unwrap();
    let v = parse_payload(&payload);
    Python::with_gil(|py| {
        let dict = v.0.bind(py).downcast::<PyDict>().unwrap();
        let item = dict.get_item("person").unwrap().unwrap();
        let person = item.downcast::<PyDict>().unwrap();
        let item = person.get_item("github").unwrap().unwrap();
        let github = item.downcast::<PyDict>().unwrap();
        let followers = github
            .get_item("followers")
            .unwrap()
            .unwrap()
            .extract::<f64>()
            .unwrap();
        assert_eq!(followers, 95.0);
    });
}

#[test]
fn streaming_values_large() {
    let payload = std::fs::read_to_string(LARGE_JSON).unwrap();
    let v = parse_payload(&payload);
    Python::with_gil(|py| {
        let dict = v.0.bind(py).downcast::<PyDict>().unwrap();
        let item = dict.get_item("person").unwrap().unwrap();
        let person = item.downcast::<PyDict>().unwrap();
        let item = person.get_item("github").unwrap().unwrap();
        let github = item.downcast::<PyDict>().unwrap();
        let followers = github
            .get_item("followers")
            .unwrap()
            .unwrap()
            .extract::<f64>()
            .unwrap();
        assert_eq!(followers, 95.0);
    });
}
