use jsonmodem::JsonValueFactory;
use jsonmodem_py::PyFactory;
use pyo3::{
    Python,
    types::{PyAnyMethods, PyString, PyStringMethods},
};

#[test]
fn pyfactory_scalar_roundtrip() {
    let mut f = PyFactory;

    let null_token = f.new_null();
    let v_null = f.build_from_null(null_token);

    let bool_token = f.new_bool(true);
    let v_bool = f.build_from_bool(bool_token);

    let num_token = f.new_number(3.5);
    let v_num = f.build_from_num(num_token);

    let str_token = f.new_string("hi");
    let v_str = f.build_from_str(str_token);

    Python::with_gil(|py| {
        assert!(v_null.0.bind(py).is_none());
        assert_eq!(v_bool.0.bind(py).extract::<bool>().unwrap(), true);
        assert_eq!(v_num.0.bind(py).extract::<f64>().unwrap(), 3.5);
        let s = v_str.0.bind(py).downcast::<PyString>().unwrap();
        assert_eq!(s.to_str().unwrap(), "hi");
    });
}
