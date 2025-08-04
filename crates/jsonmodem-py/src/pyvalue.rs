use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyList, PyString};

use jsonmodem::{JsonValue, ValueKind};

/// Wrapper around a Python object implementing [`JsonValue`].
pub struct PyJsonValue(pub Py<PyAny>);

impl Clone for PyJsonValue {
    fn clone(&self) -> Self {
        Python::with_gil(|py| Self(self.0.clone_ref(py)))
    }
}

impl core::fmt::Debug for PyJsonValue {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        Python::with_gil(|py| match self.0.bind(py).repr() {
            Ok(rep) => write!(f, "PyJsonValue({})", rep.to_string_lossy()),
            Err(_) => write!(f, "PyJsonValue(<repr error>)"),
        })
    }
}

impl PartialEq for PyJsonValue {
    fn eq(&self, other: &Self) -> bool {
        Python::with_gil(|py| self.0.bind(py).eq(other.0.bind(py)).unwrap_or(false))
    }
}

impl Default for PyJsonValue {
    fn default() -> Self {
        Python::with_gil(|py| Self(py.None().into()))
    }
}

impl JsonValue for PyJsonValue {
    type Str = String;
    type Num = f64;
    type Bool = bool;
    type Null = ();
    type Array = Vec<PyJsonValue>;
    type Object = Vec<(String, PyJsonValue)>;

    fn kind(v: &Self) -> ValueKind {
        Python::with_gil(|py| {
            let obj = v.0.bind(py);
            if obj.is_none() {
                ValueKind::Null
            } else if obj.is_instance(&py.get_type::<PyBool>()).unwrap_or(false) {
                ValueKind::Bool
            } else if obj.is_instance(&py.get_type::<PyFloat>()).unwrap_or(false) {
                ValueKind::Num
            } else if obj.is_instance(&py.get_type::<PyString>()).unwrap_or(false) {
                ValueKind::Str
            } else if obj.is_instance(&py.get_type::<PyList>()).unwrap_or(false) {
                ValueKind::Array
            } else if obj.is_instance(&py.get_type::<PyDict>()).unwrap_or(false) {
                ValueKind::Object
            } else {
                ValueKind::Null
            }
        })
    }

    fn as_string_mut(_v: &mut Self) -> Option<&mut Self::Str> {
        None
    }

    fn as_array_mut(_v: &mut Self) -> Option<&mut Self::Array> {
        None
    }

    fn as_object_mut(_v: &mut Self) -> Option<&mut Self::Object> {
        None
    }

    fn object_get_mut<'a>(obj: &'a mut Self::Object, key: &str) -> Option<&'a mut Self> {
        obj.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    fn array_get_mut(arr: &mut Self::Array, idx: usize) -> Option<&mut Self> {
        arr.get_mut(idx)
    }

    fn array_len(arr: &Self::Array) -> usize {
        arr.len()
    }

    fn into_array(v: Self) -> Option<Self::Array> {
        Python::with_gil(|py| {
            let obj = v.0.into_bound(py);
            obj.downcast::<PyList>()
                .ok()
                .map(|list| list.iter().map(|item| PyJsonValue(item.into())).collect())
        })
    }

    fn into_object(v: Self) -> Option<Self::Object> {
        Python::with_gil(|py| {
            let obj = v.0.into_bound(py);
            obj.downcast::<PyDict>().ok().map(|dict| {
                dict.iter()
                    .map(|(k, v)| {
                        let key: String = k.extract().unwrap_or_default();
                        (key, PyJsonValue(v.into()))
                    })
                    .collect()
            })
        })
    }
}
