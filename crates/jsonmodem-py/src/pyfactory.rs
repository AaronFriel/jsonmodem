use jsonmodem::JsonValueFactory;
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyList, PyString};

use crate::pyvalue::PyJsonValue;

/// Factory that builds Python objects for the streaming parser.
#[derive(Copy, Clone, Default, Debug, Eq, PartialEq)]
pub struct PyFactory;

impl JsonValueFactory for PyFactory {
    type Value = PyJsonValue;

    fn new_null(&mut self) -> () {}
    fn new_bool(&mut self, b: bool) -> bool {
        b
    }
    fn new_number(&mut self, n: f64) -> f64 {
        n
    }
    fn new_string(&mut self, s: &str) -> String {
        s.into()
    }
    fn new_array(&mut self) -> Vec<PyJsonValue> {
        Vec::new()
    }
    fn new_object(&mut self) -> Vec<(String, PyJsonValue)> {
        Vec::new()
    }

    fn push_string(&mut self, tgt: &mut String, src: &String) {
        tgt.push_str(src);
    }
    fn push_str(&mut self, tgt: &mut String, src: &str) {
        tgt.push_str(src);
    }
    fn push_array(&mut self, arr: &mut Vec<PyJsonValue>, v: PyJsonValue) {
        arr.push(v);
    }
    fn insert_object(&mut self, o: &mut Vec<(String, PyJsonValue)>, k: &str, v: PyJsonValue) {
        o.push((k.into(), v));
    }

    fn build_from_str(&mut self, s: String) -> PyJsonValue {
        Python::with_gil(|py| PyJsonValue(PyString::new(py, &s).into()))
    }
    fn build_from_num(&mut self, n: f64) -> PyJsonValue {
        Python::with_gil(|py| PyJsonValue(PyFloat::new(py, n).into()))
    }
    fn build_from_bool(&mut self, b: bool) -> PyJsonValue {
        Python::with_gil(|py| {
            let obj: Py<PyBool> = PyBool::new(py, b).into();
            PyJsonValue(obj.into())
        })
    }
    fn build_from_null(&mut self, _n: ()) -> PyJsonValue {
        Python::with_gil(|py| PyJsonValue(py.None().into()))
    }
    fn build_from_array(&mut self, a: Vec<PyJsonValue>) -> PyJsonValue {
        Python::with_gil(|py| {
            let list = PyList::empty(py);
            for v in a {
                list.append(v.0).unwrap();
            }
            PyJsonValue(list.into())
        })
    }
    fn build_from_object(&mut self, o: Vec<(String, PyJsonValue)>) -> PyJsonValue {
        Python::with_gil(|py| {
            let dict = PyDict::new(py);
            for (k, v) in o {
                dict.set_item(k, v.0).unwrap();
            }
            PyJsonValue(dict.into())
        })
    }

    fn object_insert<'a, 'b: 'a>(
        &'a mut self,
        o: &'b mut Vec<(String, PyJsonValue)>,
        k: String,
        v: PyJsonValue,
    ) -> &'b mut PyJsonValue {
        o.push((k, v));
        &mut o.last_mut().unwrap().1
    }
    fn array_push<'a, 'b: 'a>(
        &'a mut self,
        a: &'b mut Vec<PyJsonValue>,
        v: PyJsonValue,
    ) -> &'b mut PyJsonValue {
        a.push(v);
        a.last_mut().unwrap()
    }
}
