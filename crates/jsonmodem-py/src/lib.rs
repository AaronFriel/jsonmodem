use std::{
    borrow::Cow,
    collections::HashMap,
    os::raw::{c_int, c_void},
    sync::{Arc, Mutex},
};

use ::jsonmodem::{
    DecodeMode as CoreDecodeMode, JsonModem as CoreJsonModem, ParseEvent,
    ParserOptions as CoreParserOptions, Path, PathItem, StdBackend,
    lending_iterator::LendingIterator as CoreLendingIterator,
};
use pyo3::{
    IntoPyObject,
    class::basic::CompareOp,
    create_exception,
    exceptions::{PyException, PyIndexError, PyTypeError},
    ffi,
    prelude::*,
    types::{
        PyAny, PyBool, PyBytes, PyDict, PyList, PyMemoryView, PySlice, PyString, PyStringMethods,
        PyTuple,
    },
    wrap_pyfunction,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum DecodeMode {
    #[default]
    StrictUnicode,
    SurrogatePreserving,
    ReplaceInvalid,
}

impl DecodeMode {
    fn to_core(self) -> CoreDecodeMode {
        match self {
            DecodeMode::StrictUnicode => CoreDecodeMode::StrictUnicode,
            DecodeMode::SurrogatePreserving => CoreDecodeMode::SurrogatePreserving,
            DecodeMode::ReplaceInvalid => CoreDecodeMode::ReplaceInvalid,
        }
    }
}

create_exception!(jsonmodem._jsonmodem, JsonModemSyntaxError, PyException);
create_exception!(jsonmodem._jsonmodem, JsonModemStateError, PyException);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum OwnedPathComponent {
    Key(String),
    Index(usize),
}

#[derive(Clone, Copy)]
enum OwnedEventKind {
    Null,
    Bool,
    Number,
    String,
    ArrayBegin,
    ArrayEnd,
    ObjectBegin,
    ObjectEnd,
}

struct OwnedParserError {
    message: String,
    line: usize,
    column: usize,
}

enum EventRecord {
    Event(PyObject),
    Error(OwnedParserError),
    Consumed,
}

type EventRecordPool = Arc<Mutex<Vec<Vec<EventRecord>>>>;

fn new_event_record_pool() -> EventRecordPool {
    Arc::new(Mutex::new(Vec::new()))
}

fn take_event_records(pool: &EventRecordPool) -> Vec<EventRecord> {
    pool.lock()
        .ok()
        .and_then(|mut records| records.pop())
        .unwrap_or_default()
}

fn recycle_event_records(record_pool: &EventRecordPool, mut records: Vec<EventRecord>) {
    records.clear();
    if records.capacity() > 1024 {
        return;
    }

    if let Ok(mut available) = record_pool.lock() {
        if available.len() < 32 {
            available.push(records);
        }
    }
}

struct ByteViewStringFragment {
    fragment: PyObject,
    is_initial: bool,
    is_final: bool,
    is_view: bool,
}

enum ByteViewPayload {
    None,
    Bool(bool),
    Number(f64),
    String(ByteViewStringFragment),
}

struct ByteViewEvent {
    kind: OwnedEventKind,
    path: Vec<OwnedPathComponent>,
    payload: ByteViewPayload,
}

enum ByteViewRecord {
    Event(ByteViewEvent),
    Error(OwnedParserError),
}

#[derive(Clone)]
enum PathPatternComponent {
    Key(String),
    Index(usize),
    Wildcard,
}

type PathPattern = Vec<PathPatternComponent>;

fn convert_path(path: Path) -> Vec<OwnedPathComponent> {
    path.into_iter()
        .map(|component| match component {
            PathItem::Key(key) => OwnedPathComponent::Key(key.to_string()),
            PathItem::Index(index) => OwnedPathComponent::Index(index),
        })
        .collect()
}

fn convert_borrowed_path(path: &Path) -> Vec<OwnedPathComponent> {
    path.iter()
        .map(|component| match component {
            PathItem::Key(key) => OwnedPathComponent::Key(key.to_string()),
            PathItem::Index(index) => OwnedPathComponent::Index(*index),
        })
        .collect()
}

impl ByteViewEvent {
    fn to_raw_event(&self, py: Python<'_>, interns: &InternedStrings) -> PyResult<PyObject> {
        let kind = interns.kind_bound(py, self.kind).into_any().unbind();
        let path = build_path_tuple(py, &self.path, interns)?
            .into_any()
            .unbind();
        let payload = build_byte_view_payload_with_interns(py, &self.payload, interns)?
            .into_any()
            .unbind();
        let tuple = PyTuple::new(py, [kind, path, payload])?;
        Ok(tuple.into_any().unbind())
    }
}

fn borrowed_parse_event_to_raw_event(
    py: Python<'_>,
    event: ParseEvent<'_, &Path, StdBackend>,
    interns: &InternedStrings,
    active_string_paths: Option<&mut HashMap<Vec<OwnedPathComponent>, PyObject>>,
) -> PyResult<PyObject> {
    match event {
        ParseEvent::Null { path } => build_raw_event(
            py,
            OwnedEventKind::Null,
            path,
            py.None().into_bound(py).into_any(),
            interns,
        ),
        ParseEvent::Boolean { path, value } => build_raw_event(
            py,
            OwnedEventKind::Bool,
            path,
            PyBool::new(py, value).to_owned().into_any(),
            interns,
        ),
        ParseEvent::Number { path, value } => build_raw_event(
            py,
            OwnedEventKind::Number,
            path,
            value.into_pyobject(py)?.into_any(),
            interns,
        ),
        ParseEvent::String {
            path,
            fragment,
            is_initial,
            is_final,
        } => {
            let dict = PyDict::new(py);
            dict.set_item(interns.fragment_key(py), fragment.as_ref())?;
            dict.set_item(interns.is_initial_key(py), is_initial)?;
            dict.set_item(interns.is_final_key(py), is_final)?;
            if let Some(active_string_paths) = active_string_paths {
                let path = string_path_object(
                    py,
                    path,
                    is_initial,
                    is_final,
                    interns,
                    active_string_paths,
                )?;
                build_raw_event_with_path(
                    py,
                    OwnedEventKind::String,
                    path,
                    dict.into_any(),
                    interns,
                )
            } else {
                build_raw_event(py, OwnedEventKind::String, path, dict.into_any(), interns)
            }
        }
        ParseEvent::ArrayBegin { path } => build_raw_event(
            py,
            OwnedEventKind::ArrayBegin,
            path,
            py.None().into_bound(py).into_any(),
            interns,
        ),
        ParseEvent::ArrayEnd { path, .. } => build_raw_event(
            py,
            OwnedEventKind::ArrayEnd,
            path,
            py.None().into_bound(py).into_any(),
            interns,
        ),
        ParseEvent::ObjectBegin { path } => build_raw_event(
            py,
            OwnedEventKind::ObjectBegin,
            path,
            py.None().into_bound(py).into_any(),
            interns,
        ),
        ParseEvent::ObjectEnd { path, .. } => build_raw_event(
            py,
            OwnedEventKind::ObjectEnd,
            path,
            py.None().into_bound(py).into_any(),
            interns,
        ),
    }
}

fn string_path_object(
    py: Python<'_>,
    path: &Path,
    is_initial: bool,
    is_final: bool,
    interns: &InternedStrings,
    active_string_paths: &mut HashMap<Vec<OwnedPathComponent>, PyObject>,
) -> PyResult<PyObject> {
    let key = convert_borrowed_path(path);
    if !is_initial {
        if let Some(path_object) = active_string_paths.get(&key) {
            let path_object = path_object.clone_ref(py);
            if is_final {
                active_string_paths.remove(&key);
            }
            return Ok(path_object);
        }
    }

    let path_object = build_path_tuple(py, &key, interns)?.into_any().unbind();
    if is_initial && !is_final {
        active_string_paths.insert(key, path_object.clone_ref(py));
    } else if is_final {
        active_string_paths.remove(&key);
    }
    Ok(path_object)
}

fn build_raw_event(
    py: Python<'_>,
    kind: OwnedEventKind,
    path: &Path,
    payload: Bound<'_, PyAny>,
    interns: &InternedStrings,
) -> PyResult<PyObject> {
    let path = build_core_path_tuple(py, path, interns)?
        .into_any()
        .unbind();
    build_raw_event_with_path(py, kind, path, payload, interns)
}

fn build_raw_event_with_path(
    py: Python<'_>,
    kind: OwnedEventKind,
    path: PyObject,
    payload: Bound<'_, PyAny>,
    interns: &InternedStrings,
) -> PyResult<PyObject> {
    let kind = interns.kind_bound(py, kind).into_any().unbind();
    let payload = payload.unbind();
    unsafe {
        let tuple_ptr = ffi::PyTuple_New(3);
        if tuple_ptr.is_null() {
            return Err(PyErr::fetch(py));
        }

        if ffi::PyTuple_SetItem(tuple_ptr, 0, kind.into_ptr()) != 0 {
            ffi::Py_DECREF(tuple_ptr);
            return Err(PyErr::fetch(py));
        }
        if ffi::PyTuple_SetItem(tuple_ptr, 1, path.into_ptr()) != 0 {
            ffi::Py_DECREF(tuple_ptr);
            return Err(PyErr::fetch(py));
        }
        if ffi::PyTuple_SetItem(tuple_ptr, 2, payload.into_ptr()) != 0 {
            ffi::Py_DECREF(tuple_ptr);
            return Err(PyErr::fetch(py));
        }

        Ok(Bound::from_owned_ptr(py, tuple_ptr).into_any().unbind())
    }
}

fn build_view_event(
    py: Python<'_>,
    kind: OwnedEventKind,
    path: Vec<OwnedPathComponent>,
    payload: PyObject,
    interns: &InternedStrings,
) -> PyResult<PyObject> {
    let kind = interns.kind_bound(py, kind).into_any().unbind();
    let path = Py::new(py, PyPathView { path })?
        .into_bound(py)
        .into_any()
        .unbind();
    unsafe {
        let tuple_ptr = ffi::PyTuple_New(3);
        if tuple_ptr.is_null() {
            return Err(PyErr::fetch(py));
        }

        if ffi::PyTuple_SetItem(tuple_ptr, 0, kind.into_ptr()) != 0 {
            ffi::Py_DECREF(tuple_ptr);
            return Err(PyErr::fetch(py));
        }
        if ffi::PyTuple_SetItem(tuple_ptr, 1, path.into_ptr()) != 0 {
            ffi::Py_DECREF(tuple_ptr);
            return Err(PyErr::fetch(py));
        }
        if ffi::PyTuple_SetItem(tuple_ptr, 2, payload.into_ptr()) != 0 {
            ffi::Py_DECREF(tuple_ptr);
            return Err(PyErr::fetch(py));
        }

        Ok(Bound::from_owned_ptr(py, tuple_ptr).into_any().unbind())
    }
}

fn borrowed_parse_event_to_view_event(
    py: Python<'_>,
    event: ParseEvent<'_, &Path, StdBackend>,
    interns: &InternedStrings,
) -> PyResult<PyObject> {
    match event {
        ParseEvent::Null { path } => build_view_event(
            py,
            OwnedEventKind::Null,
            convert_borrowed_path(path),
            py.None(),
            interns,
        ),
        ParseEvent::Boolean { path, value } => build_view_event(
            py,
            OwnedEventKind::Bool,
            convert_borrowed_path(path),
            PyBool::new(py, value).to_owned().into_any().unbind(),
            interns,
        ),
        ParseEvent::Number { path, value } => build_view_event(
            py,
            OwnedEventKind::Number,
            convert_borrowed_path(path),
            value.into_pyobject(py)?.into_any().unbind(),
            interns,
        ),
        ParseEvent::String {
            path,
            fragment,
            is_initial,
            is_final,
        } => build_view_event(
            py,
            OwnedEventKind::String,
            convert_borrowed_path(path),
            Py::new(
                py,
                PyStringPayload {
                    fragment: fragment.as_ref().to_string(),
                    is_initial,
                    is_final,
                },
            )?
            .into_bound(py)
            .into_any()
            .unbind(),
            interns,
        ),
        ParseEvent::ArrayBegin { path } => build_view_event(
            py,
            OwnedEventKind::ArrayBegin,
            convert_borrowed_path(path),
            py.None(),
            interns,
        ),
        ParseEvent::ArrayEnd { path, .. } => build_view_event(
            py,
            OwnedEventKind::ArrayEnd,
            convert_borrowed_path(path),
            py.None(),
            interns,
        ),
        ParseEvent::ObjectBegin { path } => build_view_event(
            py,
            OwnedEventKind::ObjectBegin,
            convert_borrowed_path(path),
            py.None(),
            interns,
        ),
        ParseEvent::ObjectEnd { path, .. } => build_view_event(
            py,
            OwnedEventKind::ObjectEnd,
            convert_borrowed_path(path),
            py.None(),
            interns,
        ),
    }
}

fn build_core_path_tuple<'py>(
    py: Python<'py>,
    path: &Path,
    interns: &'py InternedStrings,
) -> PyResult<Bound<'py, PyTuple>> {
    if path.is_empty() {
        return Ok(PyTuple::empty(py));
    }

    unsafe {
        let tuple_ptr = ffi::PyTuple_New(path.len() as ffi::Py_ssize_t);
        if tuple_ptr.is_null() {
            return Err(PyErr::fetch(py));
        }

        for (index, component) in path.iter().enumerate() {
            let pair = match build_core_path_component_tuple(py, component, interns) {
                Ok(pair) => pair,
                Err(err) => {
                    ffi::Py_DECREF(tuple_ptr);
                    return Err(err);
                }
            };
            let status = ffi::PyTuple_SetItem(
                tuple_ptr,
                index as ffi::Py_ssize_t,
                pair.into_any().unbind().into_ptr(),
            );
            if status != 0 {
                ffi::Py_DECREF(tuple_ptr);
                return Err(PyErr::fetch(py));
            }
        }

        Ok(Bound::from_owned_ptr(py, tuple_ptr).downcast_into_unchecked())
    }
}

fn build_core_path_component_tuple<'py>(
    py: Python<'py>,
    component: &PathItem<impl AsRef<str>, usize>,
    interns: &'py InternedStrings,
) -> PyResult<Bound<'py, PyTuple>> {
    match component {
        PathItem::Key(key) => PyTuple::new(
            py,
            [
                interns.key_tag(py).into_any().unbind(),
                PyString::new(py, key.as_ref()).into_any().unbind(),
            ],
        ),
        PathItem::Index(index) => PyTuple::new(
            py,
            [
                interns.index_tag(py).into_any().unbind(),
                index.into_pyobject(py)?.into_any().unbind(),
            ],
        ),
    }
}

fn build_byte_view_payload_with_interns<'py>(
    py: Python<'py>,
    payload: &ByteViewPayload,
    interns: &'py InternedStrings,
) -> PyResult<Bound<'py, PyAny>> {
    match payload {
        ByteViewPayload::None => Ok(py.None().into_bound(py)),
        ByteViewPayload::Bool(value) => Ok(PyBool::new(py, *value).to_owned().into_any()),
        ByteViewPayload::Number(value) => Ok(value.into_pyobject(py)?.into_any()),
        ByteViewPayload::String(fragment) => {
            let dict = PyDict::new(py);
            dict.set_item(interns.fragment_key(py), fragment.fragment.clone_ref(py))?;
            dict.set_item(interns.is_initial_key(py), fragment.is_initial)?;
            dict.set_item(interns.is_final_key(py), fragment.is_final)?;
            dict.set_item(interns.is_view_key(py), fragment.is_view)?;
            Ok(dict.into_any())
        }
    }
}

fn build_path_tuple<'py>(
    py: Python<'py>,
    path: &[OwnedPathComponent],
    interns: &'py InternedStrings,
) -> PyResult<Bound<'py, PyTuple>> {
    if path.is_empty() {
        return Ok(PyTuple::empty(py));
    }

    unsafe {
        let tuple_ptr = ffi::PyTuple_New(path.len() as ffi::Py_ssize_t);
        if tuple_ptr.is_null() {
            return Err(PyErr::fetch(py));
        }

        for (index, component) in path.iter().enumerate() {
            let pair = match build_path_component_tuple(py, component, interns) {
                Ok(pair) => pair,
                Err(err) => {
                    ffi::Py_DECREF(tuple_ptr);
                    return Err(err);
                }
            };
            let status = ffi::PyTuple_SetItem(
                tuple_ptr,
                index as ffi::Py_ssize_t,
                pair.into_any().unbind().into_ptr(),
            );
            if status != 0 {
                ffi::Py_DECREF(tuple_ptr);
                return Err(PyErr::fetch(py));
            }
        }

        Ok(Bound::from_owned_ptr(py, tuple_ptr).downcast_into_unchecked())
    }
}

fn build_path_component_tuple<'py>(
    py: Python<'py>,
    component: &OwnedPathComponent,
    interns: &'py InternedStrings,
) -> PyResult<Bound<'py, PyTuple>> {
    match component {
        OwnedPathComponent::Key(key) => PyTuple::new(
            py,
            [
                interns.key_tag(py).into_any().unbind(),
                PyString::new(py, key).into_any().unbind(),
            ],
        ),
        OwnedPathComponent::Index(index) => PyTuple::new(
            py,
            [
                interns.index_tag(py).into_any().unbind(),
                index.into_pyobject(py)?.into_any().unbind(),
            ],
        ),
    }
}

fn build_path_tuple_for_event(py: Python<'_>, path: &[OwnedPathComponent]) -> PyResult<PyObject> {
    if path.is_empty() {
        return Ok(PyTuple::empty(py).into_any().unbind());
    }

    unsafe {
        let tuple_ptr = ffi::PyTuple_New(path.len() as ffi::Py_ssize_t);
        if tuple_ptr.is_null() {
            return Err(PyErr::fetch(py));
        }

        for (index, component) in path.iter().enumerate() {
            let pair = match build_path_component_tuple_for_event(py, component) {
                Ok(pair) => pair,
                Err(err) => {
                    ffi::Py_DECREF(tuple_ptr);
                    return Err(err);
                }
            };
            let status = ffi::PyTuple_SetItem(
                tuple_ptr,
                index as ffi::Py_ssize_t,
                pair.into_any().unbind().into_ptr(),
            );
            if status != 0 {
                ffi::Py_DECREF(tuple_ptr);
                return Err(PyErr::fetch(py));
            }
        }

        Ok(Bound::from_owned_ptr(py, tuple_ptr).into_any().unbind())
    }
}

fn build_path_component_tuple_for_event<'py>(
    py: Python<'py>,
    component: &OwnedPathComponent,
) -> PyResult<Bound<'py, PyTuple>> {
    match component {
        OwnedPathComponent::Key(key) => PyTuple::new(
            py,
            [
                PyString::intern(py, "key").into_any().unbind(),
                PyString::new(py, key).into_any().unbind(),
            ],
        ),
        OwnedPathComponent::Index(index) => PyTuple::new(
            py,
            [
                PyString::intern(py, "index").into_any().unbind(),
                index.into_pyobject(py)?.into_any().unbind(),
            ],
        ),
    }
}

struct KindInterns {
    null: Py<PyString>,
    boolean: Py<PyString>,
    number: Py<PyString>,
    string: Py<PyString>,
    array_begin: Py<PyString>,
    array_end: Py<PyString>,
    object_begin: Py<PyString>,
    object_end: Py<PyString>,
}

impl KindInterns {
    fn new(py: Python<'_>) -> PyResult<Self> {
        Ok(Self {
            null: PyString::intern(py, "null").into(),
            boolean: PyString::intern(py, "bool").into(),
            number: PyString::intern(py, "number").into(),
            string: PyString::intern(py, "string").into(),
            array_begin: PyString::intern(py, "array_begin").into(),
            array_end: PyString::intern(py, "array_end").into(),
            object_begin: PyString::intern(py, "object_begin").into(),
            object_end: PyString::intern(py, "object_end").into(),
        })
    }

    fn kind(&self, kind: OwnedEventKind) -> &Py<PyString> {
        match kind {
            OwnedEventKind::Null => &self.null,
            OwnedEventKind::Bool => &self.boolean,
            OwnedEventKind::Number => &self.number,
            OwnedEventKind::String => &self.string,
            OwnedEventKind::ArrayBegin => &self.array_begin,
            OwnedEventKind::ArrayEnd => &self.array_end,
            OwnedEventKind::ObjectBegin => &self.object_begin,
            OwnedEventKind::ObjectEnd => &self.object_end,
        }
    }
}

struct PathInterns {
    key: Py<PyString>,
    index: Py<PyString>,
}

impl PathInterns {
    fn new(py: Python<'_>) -> PyResult<Self> {
        Ok(Self {
            key: PyString::intern(py, "key").into(),
            index: PyString::intern(py, "index").into(),
        })
    }
}

struct PayloadInterns {
    fragment: Py<PyString>,
    is_initial: Py<PyString>,
    is_final: Py<PyString>,
    is_view: Py<PyString>,
}

impl PayloadInterns {
    fn new(py: Python<'_>) -> PyResult<Self> {
        Ok(Self {
            fragment: PyString::intern(py, "fragment").into(),
            is_initial: PyString::intern(py, "is_initial").into(),
            is_final: PyString::intern(py, "is_final").into(),
            is_view: PyString::intern(py, "is_view").into(),
        })
    }
}

struct InternedStrings {
    kinds: KindInterns,
    path: PathInterns,
    payload: PayloadInterns,
}

impl InternedStrings {
    fn new(py: Python<'_>) -> PyResult<Self> {
        Ok(Self {
            kinds: KindInterns::new(py)?,
            path: PathInterns::new(py)?,
            payload: PayloadInterns::new(py)?,
        })
    }

    fn clone_ref(&self, py: Python<'_>) -> Self {
        Self {
            kinds: KindInterns {
                null: self.kinds.null.clone_ref(py),
                boolean: self.kinds.boolean.clone_ref(py),
                number: self.kinds.number.clone_ref(py),
                string: self.kinds.string.clone_ref(py),
                array_begin: self.kinds.array_begin.clone_ref(py),
                array_end: self.kinds.array_end.clone_ref(py),
                object_begin: self.kinds.object_begin.clone_ref(py),
                object_end: self.kinds.object_end.clone_ref(py),
            },
            path: PathInterns {
                key: self.path.key.clone_ref(py),
                index: self.path.index.clone_ref(py),
            },
            payload: PayloadInterns {
                fragment: self.payload.fragment.clone_ref(py),
                is_initial: self.payload.is_initial.clone_ref(py),
                is_final: self.payload.is_final.clone_ref(py),
                is_view: self.payload.is_view.clone_ref(py),
            },
        }
    }

    fn kind_bound<'py>(&'py self, py: Python<'py>, kind: OwnedEventKind) -> Bound<'py, PyString> {
        let owned = self.kinds.kind(kind).clone_ref(py);
        owned.into_bound(py)
    }

    fn key_tag<'py>(&'py self, py: Python<'py>) -> Bound<'py, PyString> {
        let owned = self.path.key.clone_ref(py);
        owned.into_bound(py)
    }

    fn index_tag<'py>(&'py self, py: Python<'py>) -> Bound<'py, PyString> {
        let owned = self.path.index.clone_ref(py);
        owned.into_bound(py)
    }

    fn fragment_key<'py>(&'py self, py: Python<'py>) -> Bound<'py, PyString> {
        let owned = self.payload.fragment.clone_ref(py);
        owned.into_bound(py)
    }

    fn is_initial_key<'py>(&'py self, py: Python<'py>) -> Bound<'py, PyString> {
        let owned = self.payload.is_initial.clone_ref(py);
        owned.into_bound(py)
    }

    fn is_final_key<'py>(&'py self, py: Python<'py>) -> Bound<'py, PyString> {
        let owned = self.payload.is_final.clone_ref(py);
        owned.into_bound(py)
    }

    fn is_view_key<'py>(&'py self, py: Python<'py>) -> Bound<'py, PyString> {
        let owned = self.payload.is_view.clone_ref(py);
        owned.into_bound(py)
    }
}

/// Mirror of `jsonmodem::DecodeMode`, exposed as a Python-style enum.
/// Controls how the parser decodes JSON string escapes.
///
/// Use the pre-instantiated enum values (`DecodeMode.StrictUnicode`, etc.) when
/// configuring `ParserOptions.decode_mode`.  The `value` property exposes the
/// underlying discriminant for callers that need to serialise the setting.
#[pyclass(module = "jsonmodem._jsonmodem", name = "DecodeMode")]
#[derive(Clone)]
struct PyDecodeMode {
    mode: DecodeMode,
}

impl PyDecodeMode {
    fn new_instance(py: Python<'_>, mode: DecodeMode) -> PyResult<Py<PyDecodeMode>> {
        Py::new(py, Self { mode })
    }

    fn label(mode: DecodeMode) -> &'static str {
        match mode {
            DecodeMode::StrictUnicode => "StrictUnicode",
            DecodeMode::SurrogatePreserving => "SurrogatePreserving",
            DecodeMode::ReplaceInvalid => "ReplaceInvalid",
        }
    }
}

#[pymethods]
impl PyDecodeMode {
    #[new]
    #[pyo3(signature=(name=None))]
    fn new(name: Option<&str>) -> PyResult<Self> {
        let mode = match name {
            None => DecodeMode::StrictUnicode,
            Some("StrictUnicode") => DecodeMode::StrictUnicode,
            Some("SurrogatePreserving") => DecodeMode::SurrogatePreserving,
            Some("ReplaceInvalid") => DecodeMode::ReplaceInvalid,
            Some(other) => {
                return Err(PyTypeError::new_err(format!(
                    "unknown DecodeMode value: {other}"
                )));
            }
        };
        Ok(Self { mode })
    }

    /// The human readable label (matches the Rust enum variant).
    #[getter]
    fn name(&self) -> &'static str {
        Self::label(self.mode)
    }

    /// Numeric identifier for the decode mode (0 = strict unicode).
    #[getter]
    fn value(&self) -> u8 {
        match self.mode {
            DecodeMode::StrictUnicode => 0,
            DecodeMode::SurrogatePreserving => 1,
            DecodeMode::ReplaceInvalid => 2,
        }
    }

    fn __repr__(&self) -> PyResult<String> {
        Ok(format!("DecodeMode.{}", Self::label(self.mode)))
    }

    fn __richcmp__(&self, other: Bound<'_, PyAny>, op: CompareOp) -> PyResult<PyObject> {
        let py = other.py();
        let other_mode = other.extract::<Py<PyDecodeMode>>().ok().map(|value| {
            let borrow = value.borrow(py);
            borrow.mode
        });

        let equal = match other_mode {
            Some(mode) => mode == self.mode,
            None => false,
        };

        let outcome = match op {
            CompareOp::Eq => equal,
            CompareOp::Ne => !equal,
            _ => return Ok(py.NotImplemented()),
        };

        Ok(PyBool::new(py, outcome).to_owned().into_any().unbind())
    }
}

/// Configuration options for `JsonModem` with sensible streaming defaults.
///
/// Each property mirrors a field on the underlying Rust `ParserOptions`
/// structure.  Instances are immutable after construction; use the keyword
/// arguments on `ParserOptions(...)` to set the behaviour you need.
#[pyclass(module = "jsonmodem._jsonmodem", name = "ParserOptions")]
#[derive(Clone)]
struct PyParserOptions {
    allow_unicode_whitespace: bool,
    allow_multiple: bool,
    decode_mode: DecodeMode,
    allow_uppercase_u: bool,
}

impl PyParserOptions {
    fn to_core(&self) -> CoreParserOptions {
        CoreParserOptions::new()
            .with_allow_unicode_whitespace(self.allow_unicode_whitespace)
            .with_allow_multiple_json_values(self.allow_multiple)
            .with_allow_uppercase_u(self.allow_uppercase_u)
            .with_decode_mode(self.decode_mode.to_core())
    }
}

impl Default for PyParserOptions {
    fn default() -> Self {
        Self {
            allow_unicode_whitespace: false,
            allow_multiple: false,
            decode_mode: DecodeMode::StrictUnicode,
            allow_uppercase_u: false,
        }
    }
}

#[pymethods]
impl PyParserOptions {
    /// Create a new set of parser options with optional overrides.
    ///
    /// Parameters mirror the exposed properties; each argument uses the Rust
    /// defaults when omitted.
    #[new]
    #[pyo3(signature=(
        allow_unicode_whitespace=false,
        allow_multiple=false,
        decode_mode=None,
        allow_uppercase_u=false
    ))]
    fn new(
        _py: Python<'_>,
        allow_unicode_whitespace: bool,
        allow_multiple: bool,
        decode_mode: Option<Bound<'_, PyAny>>,
        allow_uppercase_u: bool,
    ) -> PyResult<Self> {
        let mode = match decode_mode {
            Some(value) => extract_decode_mode(&value)?,
            None => DecodeMode::StrictUnicode,
        };

        Ok(Self {
            allow_unicode_whitespace,
            allow_multiple,
            decode_mode: mode,
            allow_uppercase_u,
        })
    }

    /// `True` when unicode whitespace (per JSON5) is accepted between values.
    #[getter]
    fn allow_unicode_whitespace(&self) -> bool {
        self.allow_unicode_whitespace
    }

    /// `True` when multiple JSON values may appear sequentially in the stream.
    #[getter]
    fn allow_multiple(&self) -> bool {
        self.allow_multiple
    }

    /// `True` to allow `\UXXXX` escapes (uppercase variant of the standard
    /// `\u` prefix) within strings.
    #[getter]
    fn allow_uppercase_u(&self) -> bool {
        self.allow_uppercase_u
    }

    /// Active decode strategy that governs string escape handling.
    #[getter]
    fn decode_mode<'py>(&self, py: Python<'py>) -> PyResult<Py<PyDecodeMode>> {
        PyDecodeMode::new_instance(py, self.decode_mode)
    }

    /// Convenience helper that returns the options as a standard Python dict.
    fn as_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        dict.set_item("allow_unicode_whitespace", self.allow_unicode_whitespace)?;
        dict.set_item("allow_multiple", self.allow_multiple)?;
        dict.set_item(
            "decode_mode",
            PyDecodeMode::new_instance(py, self.decode_mode)?,
        )?;
        dict.set_item("allow_uppercase_u", self.allow_uppercase_u)?;
        Ok(dict)
    }
}

/// Lazy path object returned by `JsonModem.feed()`.
#[pyclass(
    module = "jsonmodem._jsonmodem",
    name = "PathView",
    freelist = 65536,
    sequence
)]
struct PyPathView {
    path: Vec<OwnedPathComponent>,
}

impl PyPathView {
    fn component_object(py: Python<'_>, component: &OwnedPathComponent) -> PyResult<PyObject> {
        Ok(build_path_component_tuple_for_event(py, component)?
            .into_any()
            .unbind())
    }

    fn tuple_object(&self, py: Python<'_>) -> PyResult<PyObject> {
        build_path_tuple_for_event(py, &self.path)
    }

    fn tuple_range_object(
        &self,
        py: Python<'_>,
        start: isize,
        step: isize,
        length: usize,
    ) -> PyResult<PyObject> {
        if length == 0 {
            return Ok(PyTuple::empty(py).into_any().unbind());
        }

        unsafe {
            let tuple_ptr = ffi::PyTuple_New(length as ffi::Py_ssize_t);
            if tuple_ptr.is_null() {
                return Err(PyErr::fetch(py));
            }

            let mut source_index = start;
            for target_index in 0..length {
                let Some(component) = usize::try_from(source_index)
                    .ok()
                    .and_then(|index| self.path.get(index))
                else {
                    ffi::Py_DECREF(tuple_ptr);
                    return Err(PyIndexError::new_err("PathView index out of range"));
                };
                let pair = match build_path_component_tuple_for_event(py, component) {
                    Ok(pair) => pair,
                    Err(err) => {
                        ffi::Py_DECREF(tuple_ptr);
                        return Err(err);
                    }
                };
                let status = ffi::PyTuple_SetItem(
                    tuple_ptr,
                    target_index as ffi::Py_ssize_t,
                    pair.into_any().unbind().into_ptr(),
                );
                if status != 0 {
                    ffi::Py_DECREF(tuple_ptr);
                    return Err(PyErr::fetch(py));
                }
                source_index += step;
            }

            Ok(Bound::from_owned_ptr(py, tuple_ptr).into_any().unbind())
        }
    }

    fn item_at(&self, py: Python<'_>, index: isize) -> PyResult<PyObject> {
        let index = if index < 0 {
            index + self.path.len() as isize
        } else {
            index
        };
        let Some(component) = usize::try_from(index)
            .ok()
            .and_then(|index| self.path.get(index))
        else {
            return Err(PyIndexError::new_err("PathView index out of range"));
        };
        Self::component_object(py, component)
    }

    fn tuple_matches_at(&self, items: &Bound<'_, PyTuple>, offset: usize) -> PyResult<bool> {
        for (item_index, component) in self.path[offset..offset + items.len()].iter().enumerate() {
            let item = items.get_item(item_index)?;
            let Ok(pair) = item.downcast::<PyTuple>() else {
                return Ok(false);
            };
            if pair.len() != 2 {
                return Ok(false);
            }
            match component {
                OwnedPathComponent::Key(key) => {
                    if !pair.get_item(0)?.eq("key")? || !pair.get_item(1)?.eq(key)? {
                        return Ok(false);
                    }
                }
                OwnedPathComponent::Index(path_index) => {
                    if !pair.get_item(0)?.eq("index")? || !pair.get_item(1)?.eq(*path_index)? {
                        return Ok(false);
                    }
                }
            }
        }
        Ok(true)
    }

    fn equals_tuple(&self, other: &Bound<'_, PyAny>) -> PyResult<bool> {
        let Ok(items) = other.downcast::<PyTuple>() else {
            return Ok(false);
        };
        if items.len() != self.path.len() {
            return Ok(false);
        }
        self.tuple_matches_at(items, 0)
    }
}

#[pymethods]
impl PyPathView {
    fn __len__(&self) -> usize {
        self.path.len()
    }

    fn __getitem__(&self, py: Python<'_>, item: Bound<'_, PyAny>) -> PyResult<PyObject> {
        if let Ok(index) = item.extract::<isize>() {
            return self.item_at(py, index);
        }
        if let Ok(range) = item.downcast::<PySlice>() {
            let indices = range.indices(self.path.len() as isize)?;
            return self.tuple_range_object(py, indices.start, indices.step, indices.slicelength);
        }
        Err(PyTypeError::new_err(
            "PathView indices must be integers or slices",
        ))
    }

    fn as_tuple(&self, py: Python<'_>) -> PyResult<PyObject> {
        self.tuple_object(py)
    }

    fn endswith(&self, value: Bound<'_, PyAny>) -> PyResult<bool> {
        if let Ok(text) = value.downcast::<PyString>() {
            let text = <Bound<'_, PyString> as PyStringMethods<'_>>::to_cow(text)?;
            return Ok(matches!(
                self.path.last(),
                Some(OwnedPathComponent::Key(key)) if key == text.as_ref()
            ));
        }

        let Ok(items) = value.downcast::<PyTuple>() else {
            return Ok(false);
        };
        if items.len() > self.path.len() {
            return Ok(false);
        }
        self.tuple_matches_at(items, self.path.len() - items.len())
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(self.tuple_object(py)?.bind(py).repr()?.to_string())
    }

    fn __richcmp__(&self, other: Bound<'_, PyAny>, op: CompareOp) -> PyResult<PyObject> {
        let py = other.py();
        let equal = if let Ok(path_view) = other.extract::<Py<PyPathView>>() {
            self.path == path_view.borrow(py).path
        } else {
            self.equals_tuple(&other)?
        };
        match op {
            CompareOp::Eq => Ok(PyBool::new(py, equal).to_owned().into_any().unbind()),
            CompareOp::Ne => Ok(PyBool::new(py, !equal).to_owned().into_any().unbind()),
            _ => Ok(py.NotImplemented()),
        }
    }
}

/// Lazy string payload object returned for string events.
#[pyclass(
    module = "jsonmodem._jsonmodem",
    name = "StringPayload",
    freelist = 65536
)]
struct PyStringPayload {
    fragment: String,
    is_initial: bool,
    is_final: bool,
}

#[pymethods]
impl PyStringPayload {
    #[getter]
    fn fragment(&self) -> &str {
        &self.fragment
    }

    #[getter]
    fn is_initial(&self) -> bool {
        self.is_initial
    }

    #[getter]
    fn is_final(&self) -> bool {
        self.is_final
    }

    fn as_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        dict.set_item("fragment", &self.fragment)?;
        dict.set_item("is_initial", self.is_initial)?;
        dict.set_item("is_final", self.is_final)?;
        Ok(dict)
    }

    fn __getitem__(&self, key: &str) -> PyResult<PyObject> {
        Python::with_gil(|py| match key {
            "fragment" => Ok(PyString::new(py, &self.fragment).into_any().unbind()),
            "is_initial" => Ok(PyBool::new(py, self.is_initial)
                .to_owned()
                .into_any()
                .unbind()),
            "is_final" => Ok(PyBool::new(py, self.is_final)
                .to_owned()
                .into_any()
                .unbind()),
            _ => Err(PyIndexError::new_err(format!(
                "StringPayload has no key {key:?}"
            ))),
        })
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(self.as_dict(py)?.repr()?.to_string())
    }

    fn __richcmp__(&self, other: Bound<'_, PyAny>, op: CompareOp) -> PyResult<PyObject> {
        let py = other.py();
        let equal = if let Ok(other) = other.extract::<Py<PyStringPayload>>() {
            let other = other.borrow(py);
            self.fragment == other.fragment
                && self.is_initial == other.is_initial
                && self.is_final == other.is_final
        } else if let Ok(dict) = other.downcast::<PyDict>() {
            dict.get_item("fragment")?
                .is_some_and(|value| value.eq(&self.fragment).unwrap_or(false))
                && dict
                    .get_item("is_initial")?
                    .is_some_and(|value| value.eq(self.is_initial).unwrap_or(false))
                && dict
                    .get_item("is_final")?
                    .is_some_and(|value| value.eq(self.is_final).unwrap_or(false))
        } else {
            false
        };
        match op {
            CompareOp::Eq => Ok(PyBool::new(py, equal).to_owned().into_any().unbind()),
            CompareOp::Ne => Ok(PyBool::new(py, !equal).to_owned().into_any().unbind()),
            _ => Ok(py.NotImplemented()),
        }
    }
}

/// Streaming JSON parser that yields `(kind, path, payload)` tuples.
///
/// The parser keeps internal state so callers can feed arbitrarily chunked JSON
/// and still observe well-formed events.  Each `feed()` call returns an
/// iterator over the events produced while consuming that chunk; the
/// `finish()` call drains any buffered closing events.
///
/// Example
/// -------
/// ```pycon
/// >>> from jsonmodem import JsonModem
/// >>> modem = JsonModem()
/// >>> list(modem.feed('{"user":{"name":"Ada"'))
/// [('object_begin', (), None),
///  ('string', (('key', 'user'),), {'fragment': 'user', 'is_initial': True, 'is_final': True}),
///  ('object_begin', (('key', 'user'),), None),
///  ('string', (('key', 'user'), ('key', 'name')), {'fragment': 'Ada', 'is_initial': True, 'is_final': True})]
/// >>> list(modem.feed('}}'))
/// [('object_end', (('key', 'user'),), None), ('object_end', (), None)]
/// ```
#[pyclass(module = "jsonmodem._jsonmodem", name = "JsonModem", unsendable)]
struct PyJsonModem {
    parser: Option<CoreJsonModem<StdBackend>>,
    finished: bool,
    active_string_paths: HashMap<Vec<OwnedPathComponent>, PyObject>,
    interns: InternedStrings,
    record_pool: EventRecordPool,
}

#[pymethods]
impl PyJsonModem {
    /// Construct a streaming parser.
    ///
    /// Parameters
    /// ----------
    /// options:
    ///     Optional `ParserOptions` instance.  When omitted, defaults are used.
    #[new]
    #[pyo3(signature=(options=None))]
    fn new(py: Python<'_>, options: Option<Bound<'_, PyAny>>) -> PyResult<Self> {
        let parsed_options = match options {
            Some(item) => read_parser_options(item)?,
            None => PyParserOptions::default(),
        };

        Ok(Self {
            parser: Some(CoreJsonModem::new(parsed_options.to_core())),
            finished: false,
            active_string_paths: HashMap::new(),
            interns: InternedStrings::new(py)?,
            record_pool: new_event_record_pool(),
        })
    }

    /// Feed UTF-8 JSON to the parser and get an iterator over new events.
    ///
    /// `chunk` may be one `str`, `bytes`, `bytearray`, or contiguous
    /// `memoryview`, or it may be an iterable of those chunk types.
    /// Bytes-like inputs are borrowed for the duration of this call when the
    /// buffer protocol allows it.
    ///
    /// The iterator owns each event tuple, so the caller can freely retain the
    /// results even after the next `feed()` call.  Errors are reported lazily:
    /// a `JsonModemSyntaxError` is raised from the iterator at the first
    /// invalid token.
    #[pyo3(text_signature = "($self, chunk_or_chunks)")]
    fn feed(
        &mut self,
        py: Python<'_>,
        chunk_or_chunks: Bound<'_, PyAny>,
    ) -> PyResult<Py<PyEventIter>> {
        let parser = self
            .parser
            .as_mut()
            .ok_or_else(|| state_error("parser has already finished"))?;

        let interns = self.interns.clone_ref(py);
        let record_pool = Arc::clone(&self.record_pool);
        if is_single_json_input(&chunk_or_chunks) {
            return with_input_text(py, &chunk_or_chunks, "feed()", |chunk| {
                let mut records = take_event_records(&record_pool);
                collect_feed_events(py, parser, chunk, &interns, &mut records)?;
                PyEventIter::new(py, records, record_pool)
            });
        }

        let mut records = take_event_records(&record_pool);
        for item in chunk_or_chunks.try_iter()? {
            let chunk = item?;
            with_input_text(py, &chunk, "feed()", |chunk| {
                collect_feed_events(py, parser, chunk, &interns, &mut records)
            })?;
            if matches!(records.last(), Some(EventRecord::Error(_))) {
                break;
            }
        }
        PyEventIter::new(py, records, record_pool)
    }

    /// Mark the parser as complete and emit any buffered trailing events.
    ///
    /// After `finish()` returns, subsequent calls to `feed()` raise
    /// `JsonModemStateError`.  The returned iterator may still surface syntax
    /// errors (for example, trailing garbage once the document is closed).
    #[pyo3(text_signature = "($self)")]
    fn finish(&mut self, py: Python<'_>) -> PyResult<Py<PyEventIter>> {
        if self.finished {
            return Err(state_error("finish() has already been called"));
        }

        let parser = self
            .parser
            .take()
            .ok_or_else(|| state_error("parser has already finished"))?;
        let mut records = take_event_records(&self.record_pool);
        let interns = self.interns.clone_ref(py);
        collect_finish_events(py, parser, &interns, &mut records)?;
        self.finished = true;
        self.active_string_paths.clear();
        PyEventIter::new(py, records, Arc::clone(&self.record_pool))
    }

    /// `True` once the parser has been exhausted or `finish()` was called.
    #[getter]
    fn is_finished(&self) -> bool {
        self.finished
    }
}

/// Iterator over streaming events produced by `JsonModem`.
///
/// The iterator yields fully-owned event tuples; no borrowing into the input
/// buffer occurs.  It implements the standard Python iterator protocol so it
/// can be consumed by `list()`, `for`, or any itertools-style helper.
#[pyclass(module = "jsonmodem._jsonmodem")]
struct PyEventIter {
    records: Vec<EventRecord>,
    index: usize,
    record_pool: EventRecordPool,
}

impl PyEventIter {
    fn new(
        py: Python<'_>,
        records: Vec<EventRecord>,
        record_pool: EventRecordPool,
    ) -> PyResult<Py<PyEventIter>> {
        Py::new(
            py,
            PyEventIter {
                records,
                index: 0,
                record_pool,
            },
        )
    }
}

impl Drop for PyEventIter {
    fn drop(&mut self) {
        let records = std::mem::take(&mut self.records);
        recycle_event_records(&self.record_pool, records);
    }
}

#[pymethods]
impl PyEventIter {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    /// Yield the next `(kind, path, payload)` tuple or raise `StopIteration`.
    fn __next__<'py>(&mut self, py: Python<'py>) -> PyResult<Option<PyObject>> {
        if self.index >= self.records.len() {
            return Ok(None);
        }

        let entry = std::mem::replace(&mut self.records[self.index], EventRecord::Consumed);
        self.index += 1;

        match entry {
            EventRecord::Event(event) => Ok(Some(event)),
            EventRecord::Error(err) => Err(parser_error_to_py(py, &err)),
            EventRecord::Consumed => Ok(None),
        }
    }
}

/// Streaming JSON parser that returns byte views for borrowed string payloads.
///
/// This parser is intended for consumers that can work with UTF-8 bytes
/// directly.  `feed()` accepts immutable `bytes` and read-only contiguous
/// `memoryview` objects.  When a string fragment is unescaped and lies inside
/// the current input chunk, the payload's `fragment` is a `memoryview` over the
/// caller's original bytes.  Escaped or otherwise materialized fragments fall
/// back to Python `str`.
#[pyclass(
    module = "jsonmodem._jsonmodem",
    name = "JsonModemByteViews",
    unsendable
)]
struct PyJsonModemByteViews {
    parser: Option<CoreJsonModem<StdBackend>>,
    finished: bool,
    interns: InternedStrings,
}

#[pymethods]
impl PyJsonModemByteViews {
    /// Construct a byte-view streaming parser.
    #[new]
    #[pyo3(signature=(options=None))]
    fn new(py: Python<'_>, options: Option<Bound<'_, PyAny>>) -> PyResult<Self> {
        let parsed_options = match options {
            Some(item) => read_parser_options(item)?,
            None => PyParserOptions::default(),
        };

        Ok(Self {
            parser: Some(CoreJsonModem::new(parsed_options.to_core())),
            finished: false,
            interns: InternedStrings::new(py)?,
        })
    }

    /// Feed immutable UTF-8 bytes and get an iterator over new events.
    ///
    /// `chunk` may be `bytes` or a read-only contiguous `memoryview`.
    /// `str`, `bytearray`, and writable memory views are rejected because this
    /// API returns views into caller-owned memory.
    #[pyo3(text_signature = "($self, chunk)")]
    fn feed(&mut self, py: Python<'_>, chunk: Bound<'_, PyAny>) -> PyResult<Py<PyByteEventIter>> {
        let parser = self
            .parser
            .as_mut()
            .ok_or_else(|| state_error("parser has already finished"))?;

        with_readonly_byte_text(py, &chunk, "JsonModemByteViews.feed()", |text, source| {
            let records = collect_byte_view_feed_events(py, parser, text, source)?;
            PyByteEventIter::new(py, records, self.interns.clone_ref(py))
        })
    }

    /// Mark the parser as complete and emit any buffered trailing events.
    #[pyo3(text_signature = "($self)")]
    fn finish(&mut self, py: Python<'_>) -> PyResult<Py<PyByteEventIter>> {
        if self.finished {
            return Err(state_error("finish() has already been called"));
        }

        let parser = self
            .parser
            .take()
            .ok_or_else(|| state_error("parser has already finished"))?;
        let records = collect_byte_view_finish_events(py, parser)?;
        self.finished = true;
        PyByteEventIter::new(py, records, self.interns.clone_ref(py))
    }

    /// `True` once the parser has been exhausted or `finish()` was called.
    #[getter]
    fn is_finished(&self) -> bool {
        self.finished
    }
}

/// Iterator over byte-view streaming events produced by `JsonModemByteViews`.
#[pyclass(module = "jsonmodem._jsonmodem")]
struct PyByteEventIter {
    records: Vec<ByteViewRecord>,
    index: usize,
    interns: InternedStrings,
}

impl PyByteEventIter {
    fn new(
        py: Python<'_>,
        records: Vec<ByteViewRecord>,
        interns: InternedStrings,
    ) -> PyResult<Py<PyByteEventIter>> {
        Py::new(
            py,
            PyByteEventIter {
                records,
                index: 0,
                interns,
            },
        )
    }
}

#[pymethods]
impl PyByteEventIter {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    /// Yield the next `(kind, path, payload)` tuple or raise `StopIteration`.
    fn __next__<'py>(&mut self, py: Python<'py>) -> PyResult<Option<PyObject>> {
        if self.index >= self.records.len() {
            return Ok(None);
        }

        let entry = &self.records[self.index];
        self.index += 1;

        match entry {
            ByteViewRecord::Event(event) => Ok(Some(event.to_raw_event(py, &self.interns)?)),
            ByteViewRecord::Error(err) => Err(parser_error_to_py(py, err)),
        }
    }
}

/// Streaming JSON parser that only emits events matching selected paths.
///
/// Paths are strings such as `"content"` or `"items.*.metadata.etag"`. A `*`
/// component matches either one object key or one array index. When
/// `byte_views=True`, matching string fragments use the same payload rules as
/// `JsonModemByteViews`.
#[pyclass(
    module = "jsonmodem._jsonmodem",
    name = "JsonModemPathFilter",
    unsendable
)]
struct PyJsonModemPathFilter {
    parser: Option<CoreJsonModem<StdBackend>>,
    finished: bool,
    patterns: Vec<PathPattern>,
    byte_views: bool,
    active_string_paths: HashMap<Vec<OwnedPathComponent>, PyObject>,
    interns: InternedStrings,
    record_pool: EventRecordPool,
}

#[pymethods]
impl PyJsonModemPathFilter {
    /// Construct a path-filtered streaming parser.
    #[new]
    #[pyo3(signature=(paths, *, options=None, byte_views=false))]
    fn new(
        _py: Python<'_>,
        paths: Bound<'_, PyAny>,
        options: Option<Bound<'_, PyAny>>,
        byte_views: bool,
    ) -> PyResult<Self> {
        let parsed_options = match options {
            Some(item) => read_parser_options(item)?,
            None => PyParserOptions::default(),
        };
        let patterns = read_path_patterns(&paths)?;

        Ok(Self {
            parser: Some(CoreJsonModem::new(parsed_options.to_core())),
            finished: false,
            patterns,
            byte_views,
            active_string_paths: HashMap::new(),
            interns: InternedStrings::new(_py)?,
            record_pool: new_event_record_pool(),
        })
    }

    /// Feed JSON and get an iterator over matching events.
    ///
    /// With `byte_views=False`, input accepts the same types as `JsonModem`.
    /// With `byte_views=True`, input accepts the same read-only bytes-like
    /// types as `JsonModemByteViews`.
    #[pyo3(text_signature = "($self, chunk)")]
    fn feed(&mut self, py: Python<'_>, chunk: Bound<'_, PyAny>) -> PyResult<PyObject> {
        let parser = self
            .parser
            .as_mut()
            .ok_or_else(|| state_error("parser has already finished"))?;

        if self.byte_views {
            let patterns = &self.patterns;
            with_readonly_byte_text(py, &chunk, "JsonModemPathFilter.feed()", |text, source| {
                let records =
                    collect_filtered_byte_view_feed_events(py, parser, text, source, patterns)?;
                Ok(PyByteEventIter::new(py, records, self.interns.clone_ref(py))?.into_any())
            })
        } else {
            let patterns = &self.patterns;
            let interns = self.interns.clone_ref(py);
            let record_pool = Arc::clone(&self.record_pool);
            with_input_text(py, &chunk, "JsonModemPathFilter.feed()", |text| {
                let mut records = take_event_records(&record_pool);
                collect_filtered_feed_events(
                    py,
                    parser,
                    text,
                    patterns,
                    &interns,
                    None,
                    &mut records,
                )?;
                Ok(PyEventIter::new(py, records, record_pool)?.into_any())
            })
        }
    }

    /// Mark the parser as complete and emit any buffered matching events.
    #[pyo3(text_signature = "($self)")]
    fn finish(&mut self, py: Python<'_>) -> PyResult<PyObject> {
        if self.finished {
            return Err(state_error("finish() has already been called"));
        }

        let parser = self
            .parser
            .take()
            .ok_or_else(|| state_error("parser has already finished"))?;
        self.finished = true;

        if self.byte_views {
            let records = collect_filtered_byte_view_finish_events(py, parser, &self.patterns)?;
            Ok(PyByteEventIter::new(py, records, self.interns.clone_ref(py))?.into_any())
        } else {
            let mut records = take_event_records(&self.record_pool);
            let interns = self.interns.clone_ref(py);
            collect_filtered_finish_events(
                py,
                parser,
                &self.patterns,
                &interns,
                Some(&mut self.active_string_paths),
                &mut records,
            )?;
            self.active_string_paths.clear();
            Ok(PyEventIter::new(py, records, Arc::clone(&self.record_pool))?.into_any())
        }
    }

    /// `True` once the parser has been exhausted or `finish()` was called.
    #[getter]
    fn is_finished(&self) -> bool {
        self.finished
    }
}

fn collect_feed_events(
    py: Python<'_>,
    parser: &mut CoreJsonModem<StdBackend>,
    chunk: &str,
    interns: &InternedStrings,
    records: &mut Vec<EventRecord>,
) -> PyResult<()> {
    let mut events = parser.feed(chunk);
    while let Some(item) = CoreLendingIterator::next(&mut events) {
        match item {
            Ok(event) => records.push(view_event_record(py, event, interns)?),
            Err(err) => {
                records.push(error_record(err.to_string(), err.line(), err.column()));
                return Ok(());
            }
        }
    }
    drop(events);
    drain_pending_events(py, parser, interns, records)
}

fn collect_filtered_feed_events(
    py: Python<'_>,
    parser: &mut CoreJsonModem<StdBackend>,
    chunk: &str,
    patterns: &[PathPattern],
    interns: &InternedStrings,
    mut active_string_paths: Option<&mut HashMap<Vec<OwnedPathComponent>, PyObject>>,
    records: &mut Vec<EventRecord>,
) -> PyResult<()> {
    let mut events = parser.feed(chunk);
    while let Some(item) = CoreLendingIterator::next(&mut events) {
        match item {
            Ok(event) => {
                if path_matches_patterns(event.path(), patterns) {
                    records.push(event_record(
                        py,
                        event,
                        interns,
                        active_string_paths.as_deref_mut(),
                    )?);
                }
            }
            Err(err) => {
                records.push(error_record(err.to_string(), err.line(), err.column()));
                return Ok(());
            }
        }
    }
    drop(events);
    drain_filtered_pending_events(py, parser, patterns, interns, active_string_paths, records)
}

fn collect_byte_view_feed_events(
    py: Python<'_>,
    parser: &mut CoreJsonModem<StdBackend>,
    chunk: &str,
    source: &Bound<'_, PyMemoryView>,
) -> PyResult<Vec<ByteViewRecord>> {
    let mut records = Vec::new();
    for item in parser.feed(chunk).to_iter() {
        match item {
            Ok(event) => records.push(byte_view_event_record(py, event, chunk, Some(source))?),
            Err(err) => {
                records.push(byte_view_error_record(
                    err.to_string(),
                    err.line(),
                    err.column(),
                ));
                return Ok(records);
            }
        }
    }
    drain_byte_view_pending_events(py, parser, chunk, source, &mut records)?;
    Ok(records)
}

fn collect_filtered_byte_view_feed_events(
    py: Python<'_>,
    parser: &mut CoreJsonModem<StdBackend>,
    chunk: &str,
    source: &Bound<'_, PyMemoryView>,
    patterns: &[PathPattern],
) -> PyResult<Vec<ByteViewRecord>> {
    let mut records = Vec::new();
    let mut events = parser.feed(chunk);
    while let Some(item) = CoreLendingIterator::next(&mut events) {
        match item {
            Ok(event) => {
                if path_matches_patterns(event.path(), patterns) {
                    records.push(borrowed_byte_view_event_record(
                        py,
                        event,
                        chunk,
                        Some(source),
                    )?);
                }
            }
            Err(err) => {
                records.push(byte_view_error_record(
                    err.to_string(),
                    err.line(),
                    err.column(),
                ));
                return Ok(records);
            }
        }
    }
    drop(events);
    drain_filtered_byte_view_pending_events(py, parser, chunk, source, patterns, &mut records)?;
    Ok(records)
}

fn collect_byte_view_finish_events(
    py: Python<'_>,
    parser: CoreJsonModem<StdBackend>,
) -> PyResult<Vec<ByteViewRecord>> {
    let mut records = Vec::new();
    for item in parser.finish().to_iter() {
        match item {
            Ok(event) => records.push(byte_view_event_record(py, event, "", None)?),
            Err(err) => {
                records.push(byte_view_error_record(
                    err.to_string(),
                    err.line(),
                    err.column(),
                ));
                break;
            }
        }
    }
    Ok(records)
}

fn collect_filtered_byte_view_finish_events(
    py: Python<'_>,
    parser: CoreJsonModem<StdBackend>,
    patterns: &[PathPattern],
) -> PyResult<Vec<ByteViewRecord>> {
    let mut records = Vec::new();
    let mut events = parser.finish();
    while let Some(item) = CoreLendingIterator::next(&mut events) {
        match item {
            Ok(event) => {
                if path_matches_patterns(event.path(), patterns) {
                    records.push(borrowed_byte_view_event_record(py, event, "", None)?);
                }
            }
            Err(err) => {
                records.push(byte_view_error_record(
                    err.to_string(),
                    err.line(),
                    err.column(),
                ));
                break;
            }
        }
    }
    Ok(records)
}

fn drain_byte_view_pending_events(
    py: Python<'_>,
    parser: &mut CoreJsonModem<StdBackend>,
    chunk: &str,
    source: &Bound<'_, PyMemoryView>,
    records: &mut Vec<ByteViewRecord>,
) -> PyResult<()> {
    loop {
        let mut produced = false;
        for item in parser.feed("").to_iter() {
            produced = true;
            match item {
                Ok(event) => {
                    records.push(byte_view_event_record(py, event, chunk, Some(source))?);
                }
                Err(err) => {
                    records.push(byte_view_error_record(
                        err.to_string(),
                        err.line(),
                        err.column(),
                    ));
                    return Ok(());
                }
            }
        }
        if !produced {
            break;
        }
    }
    Ok(())
}

fn drain_filtered_byte_view_pending_events(
    py: Python<'_>,
    parser: &mut CoreJsonModem<StdBackend>,
    chunk: &str,
    source: &Bound<'_, PyMemoryView>,
    patterns: &[PathPattern],
    records: &mut Vec<ByteViewRecord>,
) -> PyResult<()> {
    loop {
        let mut produced = false;
        {
            let mut events = parser.feed("");
            while let Some(item) = CoreLendingIterator::next(&mut events) {
                produced = true;
                match item {
                    Ok(event) => {
                        if path_matches_patterns(event.path(), patterns) {
                            records.push(borrowed_byte_view_event_record(
                                py,
                                event,
                                chunk,
                                Some(source),
                            )?);
                        }
                    }
                    Err(err) => {
                        records.push(byte_view_error_record(
                            err.to_string(),
                            err.line(),
                            err.column(),
                        ));
                        return Ok(());
                    }
                }
            }
        }
        if !produced {
            break;
        }
    }
    Ok(())
}

fn collect_finish_events(
    py: Python<'_>,
    parser: CoreJsonModem<StdBackend>,
    interns: &InternedStrings,
    records: &mut Vec<EventRecord>,
) -> PyResult<()> {
    let mut events = parser.finish();
    while let Some(item) = CoreLendingIterator::next(&mut events) {
        match item {
            Ok(event) => records.push(view_event_record(py, event, interns)?),
            Err(err) => {
                records.push(error_record(err.to_string(), err.line(), err.column()));
                break;
            }
        }
    }
    Ok(())
}

fn collect_filtered_finish_events(
    py: Python<'_>,
    parser: CoreJsonModem<StdBackend>,
    patterns: &[PathPattern],
    interns: &InternedStrings,
    mut active_string_paths: Option<&mut HashMap<Vec<OwnedPathComponent>, PyObject>>,
    records: &mut Vec<EventRecord>,
) -> PyResult<()> {
    let mut events = parser.finish();
    while let Some(item) = CoreLendingIterator::next(&mut events) {
        match item {
            Ok(event) => {
                if path_matches_patterns(event.path(), patterns) {
                    records.push(event_record(
                        py,
                        event,
                        interns,
                        active_string_paths.as_deref_mut(),
                    )?);
                }
            }
            Err(err) => {
                records.push(error_record(err.to_string(), err.line(), err.column()));
                break;
            }
        }
    }
    Ok(())
}

fn drain_pending_events(
    py: Python<'_>,
    parser: &mut CoreJsonModem<StdBackend>,
    interns: &InternedStrings,
    records: &mut Vec<EventRecord>,
) -> PyResult<()> {
    loop {
        let mut produced = false;
        {
            let mut events = parser.feed("");
            while let Some(item) = CoreLendingIterator::next(&mut events) {
                produced = true;
                match item {
                    Ok(event) => records.push(view_event_record(py, event, interns)?),
                    Err(err) => {
                        records.push(error_record(err.to_string(), err.line(), err.column()));
                        return Ok(());
                    }
                }
            }
        }
        if !produced {
            break;
        }
    }
    Ok(())
}

fn drain_filtered_pending_events(
    py: Python<'_>,
    parser: &mut CoreJsonModem<StdBackend>,
    patterns: &[PathPattern],
    interns: &InternedStrings,
    mut active_string_paths: Option<&mut HashMap<Vec<OwnedPathComponent>, PyObject>>,
    records: &mut Vec<EventRecord>,
) -> PyResult<()> {
    loop {
        let mut produced = false;
        {
            let mut events = parser.feed("");
            while let Some(item) = CoreLendingIterator::next(&mut events) {
                produced = true;
                match item {
                    Ok(event) => {
                        if path_matches_patterns(event.path(), patterns) {
                            records.push(event_record(
                                py,
                                event,
                                interns,
                                active_string_paths.as_deref_mut(),
                            )?);
                        }
                    }
                    Err(err) => {
                        records.push(error_record(err.to_string(), err.line(), err.column()));
                        return Ok(());
                    }
                }
            }
        }
        if !produced {
            break;
        }
    }
    Ok(())
}

fn event_record(
    py: Python<'_>,
    event: ParseEvent<'_, &Path, StdBackend>,
    interns: &InternedStrings,
    active_string_paths: Option<&mut HashMap<Vec<OwnedPathComponent>, PyObject>>,
) -> PyResult<EventRecord> {
    Ok(EventRecord::Event(borrowed_parse_event_to_raw_event(
        py,
        event,
        interns,
        active_string_paths,
    )?))
}

fn view_event_record(
    py: Python<'_>,
    event: ParseEvent<'_, &Path, StdBackend>,
    interns: &InternedStrings,
) -> PyResult<EventRecord> {
    Ok(EventRecord::Event(borrowed_parse_event_to_view_event(
        py, event, interns,
    )?))
}

fn error_record(message: String, line: usize, column: usize) -> EventRecord {
    EventRecord::Error(OwnedParserError {
        message,
        line,
        column,
    })
}

fn byte_view_error_record(message: String, line: usize, column: usize) -> ByteViewRecord {
    ByteViewRecord::Error(OwnedParserError {
        message,
        line,
        column,
    })
}

fn byte_view_event_record(
    py: Python<'_>,
    event: ParseEvent,
    input: &str,
    source: Option<&Bound<'_, PyMemoryView>>,
) -> PyResult<ByteViewRecord> {
    let event = match event {
        ParseEvent::Null { path } => ByteViewEvent {
            kind: OwnedEventKind::Null,
            path: convert_path(path),
            payload: ByteViewPayload::None,
        },
        ParseEvent::Boolean { path, value } => ByteViewEvent {
            kind: OwnedEventKind::Bool,
            path: convert_path(path),
            payload: ByteViewPayload::Bool(value),
        },
        ParseEvent::Number { path, value } => ByteViewEvent {
            kind: OwnedEventKind::Number,
            path: convert_path(path),
            payload: ByteViewPayload::Number(value),
        },
        ParseEvent::String {
            path,
            fragment,
            is_initial,
            is_final,
        } => {
            let (fragment, is_view) = byte_view_fragment(py, input, source, fragment)?;
            ByteViewEvent {
                kind: OwnedEventKind::String,
                path: convert_path(path),
                payload: ByteViewPayload::String(ByteViewStringFragment {
                    fragment,
                    is_initial,
                    is_final,
                    is_view,
                }),
            }
        }
        ParseEvent::ArrayBegin { path } => ByteViewEvent {
            kind: OwnedEventKind::ArrayBegin,
            path: convert_path(path),
            payload: ByteViewPayload::None,
        },
        ParseEvent::ArrayEnd { path, .. } => ByteViewEvent {
            kind: OwnedEventKind::ArrayEnd,
            path: convert_path(path),
            payload: ByteViewPayload::None,
        },
        ParseEvent::ObjectBegin { path } => ByteViewEvent {
            kind: OwnedEventKind::ObjectBegin,
            path: convert_path(path),
            payload: ByteViewPayload::None,
        },
        ParseEvent::ObjectEnd { path, .. } => ByteViewEvent {
            kind: OwnedEventKind::ObjectEnd,
            path: convert_path(path),
            payload: ByteViewPayload::None,
        },
    };

    Ok(ByteViewRecord::Event(event))
}

fn borrowed_byte_view_event_record(
    py: Python<'_>,
    event: ParseEvent<'_, &Path, StdBackend>,
    input: &str,
    source: Option<&Bound<'_, PyMemoryView>>,
) -> PyResult<ByteViewRecord> {
    let event = match event {
        ParseEvent::Null { path } => ByteViewEvent {
            kind: OwnedEventKind::Null,
            path: convert_borrowed_path(path),
            payload: ByteViewPayload::None,
        },
        ParseEvent::Boolean { path, value } => ByteViewEvent {
            kind: OwnedEventKind::Bool,
            path: convert_borrowed_path(path),
            payload: ByteViewPayload::Bool(value),
        },
        ParseEvent::Number { path, value } => ByteViewEvent {
            kind: OwnedEventKind::Number,
            path: convert_borrowed_path(path),
            payload: ByteViewPayload::Number(value),
        },
        ParseEvent::String {
            path,
            fragment,
            is_initial,
            is_final,
        } => {
            let (fragment, is_view) = byte_view_fragment(py, input, source, fragment)?;
            ByteViewEvent {
                kind: OwnedEventKind::String,
                path: convert_borrowed_path(path),
                payload: ByteViewPayload::String(ByteViewStringFragment {
                    fragment,
                    is_initial,
                    is_final,
                    is_view,
                }),
            }
        }
        ParseEvent::ArrayBegin { path } => ByteViewEvent {
            kind: OwnedEventKind::ArrayBegin,
            path: convert_borrowed_path(path),
            payload: ByteViewPayload::None,
        },
        ParseEvent::ArrayEnd { path, .. } => ByteViewEvent {
            kind: OwnedEventKind::ArrayEnd,
            path: convert_borrowed_path(path),
            payload: ByteViewPayload::None,
        },
        ParseEvent::ObjectBegin { path } => ByteViewEvent {
            kind: OwnedEventKind::ObjectBegin,
            path: convert_borrowed_path(path),
            payload: ByteViewPayload::None,
        },
        ParseEvent::ObjectEnd { path, .. } => ByteViewEvent {
            kind: OwnedEventKind::ObjectEnd,
            path: convert_borrowed_path(path),
            payload: ByteViewPayload::None,
        },
    };

    Ok(ByteViewRecord::Event(event))
}

fn byte_view_fragment(
    py: Python<'_>,
    input: &str,
    source: Option<&Bound<'_, PyMemoryView>>,
    fragment: Cow<'_, str>,
) -> PyResult<(PyObject, bool)> {
    if let (Some(source), Cow::Borrowed(fragment)) = (source, &fragment) {
        if let Some((start, end)) = borrowed_range(input, fragment) {
            let view = memoryview_range(py, source, start, end)?;
            return Ok((view, true));
        }
    }

    Ok((
        PyString::new(py, fragment.as_ref()).into_any().unbind(),
        false,
    ))
}

fn read_path_patterns(value: &Bound<'_, PyAny>) -> PyResult<Vec<PathPattern>> {
    if let Ok(text) = value.downcast::<PyString>() {
        let text = <Bound<'_, PyString> as PyStringMethods<'_>>::to_cow(text)?;
        return Ok(vec![parse_path_pattern(text.as_ref())?]);
    }

    let mut patterns = Vec::new();
    for item in value.try_iter()? {
        let item = item?;
        let text: String = item.extract().map_err(|_| {
            PyTypeError::new_err("paths must be a path string or an iterable of path strings")
        })?;
        patterns.push(parse_path_pattern(&text)?);
    }

    if patterns.is_empty() {
        return Err(PyTypeError::new_err(
            "paths must contain at least one pattern",
        ));
    }
    Ok(patterns)
}

fn parse_path_pattern(pattern: &str) -> PyResult<PathPattern> {
    if pattern.is_empty() {
        return Err(PyTypeError::new_err("path pattern must not be empty"));
    }

    let mut parsed = Vec::new();
    for component in pattern.split('.') {
        if component.is_empty() {
            return Err(PyTypeError::new_err(format!(
                "path pattern {pattern:?} contains an empty component"
            )));
        }
        if component == "*" {
            parsed.push(PathPatternComponent::Wildcard);
        } else if let Ok(index) = component.parse::<usize>() {
            parsed.push(PathPatternComponent::Index(index));
        } else {
            parsed.push(PathPatternComponent::Key(component.to_string()));
        }
    }
    Ok(parsed)
}

fn path_matches_patterns(path: &Path, patterns: &[PathPattern]) -> bool {
    patterns
        .iter()
        .any(|pattern| path_matches_pattern(path, pattern))
}

fn path_matches_pattern(path: &Path, pattern: &[PathPatternComponent]) -> bool {
    path.len() == pattern.len()
        && path
            .iter()
            .zip(pattern)
            .all(
                |(path_component, pattern_component)| match (path_component, pattern_component) {
                    (_, PathPatternComponent::Wildcard) => true,
                    (PathItem::Key(path_key), PathPatternComponent::Key(pattern_key)) => {
                        path_key.as_ref() == pattern_key
                    }
                    (PathItem::Index(path_index), PathPatternComponent::Index(pattern_index)) => {
                        path_index == pattern_index
                    }
                    _ => false,
                },
            )
}

fn extract_decode_mode(value: &Bound<'_, PyAny>) -> PyResult<DecodeMode> {
    if let Ok(handle) = value.extract::<Py<PyDecodeMode>>() {
        Ok(handle.borrow(value.py()).mode)
    } else {
        Err(PyTypeError::new_err(format!(
            "decode_mode must be a DecodeMode, got {}",
            value.get_type().name()?
        )))
    }
}

fn read_parser_options(value: Bound<'_, PyAny>) -> PyResult<PyParserOptions> {
    let handle: Py<PyParserOptions> = value.extract()?;
    let borrowed = handle.borrow(value.py());
    let options = borrowed.clone();
    drop(borrowed);
    Ok(options)
}

fn parser_error_to_py(py: Python<'_>, err: &OwnedParserError) -> PyErr {
    let message = format_error_message(err);
    match py.get_type::<JsonModemSyntaxError>().call1((message,)) {
        Ok(exc) => {
            let _ = exc.setattr("line", err.line);
            let _ = exc.setattr("column", err.column);
            PyErr::from_value(exc)
        }
        Err(error) => error,
    }
}

fn state_error(message: &str) -> PyErr {
    PyErr::new::<JsonModemStateError, _>((message.to_string(),))
}

fn format_error_message(err: &OwnedParserError) -> String {
    if err.message.contains("invalid character") {
        format!("InvalidCharacter: {}", err.message)
    } else {
        err.message.clone()
    }
}

/// Decode one complete JSON document into native Python containers.
///
/// This path exists for apples-to-apples benchmarking against `json.loads`,
/// `orjson.loads`, `msgspec.json.decode`, and Jiter's Python object parser.
#[pyfunction]
#[pyo3(signature=(data))]
fn loads(py: Python<'_>, data: Bound<'_, PyAny>) -> PyResult<PyObject> {
    with_input_text(py, &data, "loads()", |text| parse_native_value(py, text))
}

fn with_input_text<T>(
    py: Python<'_>,
    data: &Bound<'_, PyAny>,
    caller: &str,
    f: impl FnOnce(&str) -> PyResult<T>,
) -> PyResult<T> {
    if let Ok(text) = data.downcast::<PyString>() {
        let text = <Bound<'_, PyString> as PyStringMethods<'_>>::to_cow(text)?;
        return f(text.as_ref());
    }

    if let Ok(bytes) = data.downcast::<PyBytes>() {
        let text = core::str::from_utf8(bytes.as_bytes()).map_err(|err| {
            PyTypeError::new_err(format!("{caller} input bytes are not valid UTF-8: {err}"))
        })?;
        return f(text);
    }

    if let Some(result) = with_buffer_text(py, data, caller, f)? {
        return result;
    }

    Err(PyTypeError::new_err(format!(
        "{caller} expected str, bytes, bytearray, or contiguous memoryview, got {}",
        data.get_type().name()?
    )))
}

fn is_single_json_input(data: &Bound<'_, PyAny>) -> bool {
    data.downcast::<PyString>().is_ok()
        || data.downcast::<PyBytes>().is_ok()
        || supports_buffer_protocol(data)
}

fn supports_buffer_protocol(data: &Bound<'_, PyAny>) -> bool {
    const PYBUF_SIMPLE: c_int = 0;

    let mut view = PyBufferView::new();
    let status = unsafe { PyObject_GetBuffer(data.as_ptr(), &mut view, PYBUF_SIMPLE) };
    if status != 0 {
        unsafe { ffi::PyErr_Clear() };
        return false;
    }
    let guard = PyBufferGuard { view };
    drop(guard);
    true
}

struct PyBufferGuard {
    view: PyBufferView,
}

impl Drop for PyBufferGuard {
    fn drop(&mut self) {
        if !self.view.obj.is_null() {
            unsafe { PyBuffer_Release(&mut self.view) };
        }
    }
}

#[repr(C)]
struct PyBufferView {
    buf: *mut c_void,
    obj: *mut ffi::PyObject,
    len: isize,
    itemsize: isize,
    readonly: c_int,
    ndim: c_int,
    format: *mut std::os::raw::c_char,
    shape: *mut isize,
    strides: *mut isize,
    suboffsets: *mut isize,
    internal: *mut c_void,
}

impl PyBufferView {
    const fn new() -> Self {
        Self {
            buf: std::ptr::null_mut(),
            obj: std::ptr::null_mut(),
            len: 0,
            itemsize: 0,
            readonly: 0,
            ndim: 0,
            format: std::ptr::null_mut(),
            shape: std::ptr::null_mut(),
            strides: std::ptr::null_mut(),
            suboffsets: std::ptr::null_mut(),
            internal: std::ptr::null_mut(),
        }
    }
}

unsafe extern "C" {
    fn PyObject_GetBuffer(obj: *mut ffi::PyObject, view: *mut PyBufferView, flags: c_int) -> c_int;
    fn PyBuffer_Release(view: *mut PyBufferView);
}

fn with_buffer_text<T>(
    _py: Python<'_>,
    data: &Bound<'_, PyAny>,
    caller: &str,
    f: impl FnOnce(&str) -> PyResult<T>,
) -> PyResult<Option<PyResult<T>>> {
    const PYBUF_SIMPLE: c_int = 0;

    let mut view = PyBufferView::new();
    let status = unsafe { PyObject_GetBuffer(data.as_ptr(), &mut view, PYBUF_SIMPLE) };
    if status != 0 {
        unsafe { ffi::PyErr_Clear() };
        return Ok(None);
    }

    let guard = PyBufferGuard { view };
    if guard.view.len < 0 {
        return Ok(Some(Err(PyTypeError::new_err(format!(
            "{caller} received a negative buffer length"
        )))));
    }

    let bytes = if guard.view.len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(guard.view.buf.cast::<u8>(), guard.view.len as usize) }
    };
    let text = core::str::from_utf8(bytes).map_err(|err| {
        PyTypeError::new_err(format!("{caller} input bytes are not valid UTF-8: {err}"))
    });
    Ok(Some(text.and_then(f)))
}

fn with_readonly_byte_text<T>(
    _py: Python<'_>,
    data: &Bound<'_, PyAny>,
    caller: &str,
    f: impl FnOnce(&str, &Bound<'_, PyMemoryView>) -> PyResult<T>,
) -> PyResult<T> {
    if data.downcast::<PyString>().is_ok() {
        return Err(PyTypeError::new_err(format!(
            "{caller} cannot return no-copy memoryview payloads from str input; pass bytes or a read-only memoryview"
        )));
    }

    const PYBUF_SIMPLE: c_int = 0;

    let mut view = PyBufferView::new();
    let status = unsafe { PyObject_GetBuffer(data.as_ptr(), &mut view, PYBUF_SIMPLE) };
    if status != 0 {
        unsafe { ffi::PyErr_Clear() };
        return Err(PyTypeError::new_err(format!(
            "{caller} expected bytes or a read-only contiguous memoryview, got {}",
            data.get_type().name()?
        )));
    }

    let guard = PyBufferGuard { view };
    if guard.view.readonly == 0 {
        return Err(PyTypeError::new_err(format!(
            "{caller} requires read-only bytes-like input for no-copy payload views"
        )));
    }
    if guard.view.len < 0 {
        return Err(PyTypeError::new_err(format!(
            "{caller} received a negative buffer length"
        )));
    }

    let bytes = if guard.view.len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(guard.view.buf.cast::<u8>(), guard.view.len as usize) }
    };
    let text = core::str::from_utf8(bytes).map_err(|err| {
        PyTypeError::new_err(format!("{caller} input bytes are not valid UTF-8: {err}"))
    })?;
    let source = PyMemoryView::from(data)?;
    f(text, &source)
}

fn memoryview_range(
    py: Python<'_>,
    source: &Bound<'_, PyMemoryView>,
    start: usize,
    end: usize,
) -> PyResult<PyObject> {
    let start = isize::try_from(start)
        .map_err(|_| PyException::new_err("memoryview start exceeds isize::MAX"))?;
    let end = isize::try_from(end)
        .map_err(|_| PyException::new_err("memoryview end exceeds isize::MAX"))?;
    let key = PySlice::new(py, start, end, 1);
    Ok(source.get_item(key)?.into_any().unbind())
}

enum NativeContainer {
    List(Py<PyList>),
    Dict(Py<PyDict>),
}

struct NativeBuilder {
    root: Option<PyObject>,
    containers: Vec<NativeContainer>,
    partial_strings: HashMap<Vec<OwnedPathComponent>, String>,
}

impl NativeBuilder {
    fn new() -> Self {
        Self {
            root: None,
            containers: Vec::new(),
            partial_strings: HashMap::new(),
        }
    }

    fn finish(self) -> PyResult<PyObject> {
        if !self.containers.is_empty() {
            return Err(PyException::new_err(
                "JSON document ended with open containers",
            ));
        }
        if !self.partial_strings.is_empty() {
            return Err(PyException::new_err(
                "JSON document ended with unfinished strings",
            ));
        }
        self.root
            .ok_or_else(|| PyException::new_err("no complete JSON value parsed"))
    }

    fn handle_event(
        &mut self,
        py: Python<'_>,
        event: ParseEvent<'_, &Path, StdBackend>,
    ) -> PyResult<()> {
        match event {
            ParseEvent::Null { path } => self.insert(py, path_key(&path), py.None()),
            ParseEvent::Boolean { path, value } => self.insert(
                py,
                path_key(&path),
                PyBool::new(py, value).to_owned().into_any().unbind(),
            ),
            ParseEvent::Number { path, value } => {
                let number = value.into_pyobject(py)?.into_any().unbind();
                self.insert(py, path_key(&path), number)
            }
            ParseEvent::String {
                path,
                fragment,
                is_final,
                ..
            } => {
                if is_final {
                    let owned_path = if self.partial_strings.is_empty() {
                        None
                    } else {
                        Some(convert_path((*path).clone()))
                    };
                    let value = if let Some(mut pending) = owned_path
                        .as_ref()
                        .and_then(|path| self.partial_strings.remove(path))
                    {
                        pending.push_str(fragment.as_ref());
                        PyString::new(py, &pending).into_any().unbind()
                    } else {
                        PyString::new(py, fragment.as_ref()).into_any().unbind()
                    };
                    self.insert(py, path_key(&path), value)
                } else {
                    self.partial_strings
                        .entry(convert_path((*path).clone()))
                        .or_default()
                        .push_str(fragment.as_ref());
                    Ok(())
                }
            }
            ParseEvent::ArrayBegin { path } => {
                let list = PyList::empty(py);
                let object = list.clone().into_any().unbind();
                self.insert(py, path_key(&path), object)?;
                self.containers.push(NativeContainer::List(list.unbind()));
                Ok(())
            }
            ParseEvent::ArrayEnd { .. } => self.end_container(true),
            ParseEvent::ObjectBegin { path } => {
                let dict = PyDict::new(py);
                let object = dict.clone().into_any().unbind();
                self.insert(py, path_key(&path), object)?;
                self.containers.push(NativeContainer::Dict(dict.unbind()));
                Ok(())
            }
            ParseEvent::ObjectEnd { .. } => self.end_container(false),
        }
    }

    fn insert(&mut self, py: Python<'_>, key: Option<&str>, value: PyObject) -> PyResult<()> {
        match self.containers.last() {
            Some(NativeContainer::List(list)) => list.bind(py).append(value),
            Some(NativeContainer::Dict(dict)) => {
                let key = key.ok_or_else(|| {
                    PyException::new_err("object value event did not include a property key")
                })?;
                dict.bind(py).set_item(key, value)
            }
            None => {
                if self.root.is_some() {
                    return Err(PyException::new_err(
                        "loads() expected exactly one JSON value, got multiple values",
                    ));
                }
                self.root = Some(value);
                Ok(())
            }
        }
    }

    fn end_container(&mut self, array: bool) -> PyResult<()> {
        match (array, self.containers.pop()) {
            (true, Some(NativeContainer::List(_))) | (false, Some(NativeContainer::Dict(_))) => {
                Ok(())
            }
            _ => Err(PyException::new_err(
                "container end event did not match parser state",
            )),
        }
    }
}

fn path_key(path: &Path) -> Option<&str> {
    match path.last() {
        Some(PathItem::Key(key)) => Some(key.as_ref()),
        _ => None,
    }
}

fn parse_native_value(py: Python<'_>, text: &str) -> PyResult<PyObject> {
    let mut parser = CoreJsonModem::new(CoreParserOptions::new());
    let mut builder = NativeBuilder::new();

    let mut events = parser.feed(text);
    while let Some(item) = CoreLendingIterator::next(&mut events) {
        match item {
            Ok(event) => builder.handle_event(py, event)?,
            Err(err) => {
                return Err(parser_error_to_py(
                    py,
                    &OwnedParserError {
                        message: err.to_string(),
                        line: err.line(),
                        column: err.column(),
                    },
                ));
            }
        }
    }
    drop(events);

    drain_native_pending(py, &mut parser, &mut builder)?;

    let mut events = parser.finish();
    while let Some(item) = CoreLendingIterator::next(&mut events) {
        match item {
            Ok(event) => builder.handle_event(py, event)?,
            Err(err) => {
                return Err(parser_error_to_py(
                    py,
                    &OwnedParserError {
                        message: err.to_string(),
                        line: err.line(),
                        column: err.column(),
                    },
                ));
            }
        }
    }

    builder.finish()
}

/// Return byte ranges for JSON string values that can borrow from `data`.
///
/// Each output item corresponds to one JSON string value. The item is
/// `(start, end)` when the UTF-8 payload is contiguous inside the input bytes,
/// and `None` when escapes or feed boundaries require materialization.
#[pyfunction]
#[pyo3(signature=(data))]
fn string_ranges(py: Python<'_>, data: Bound<'_, PyAny>) -> PyResult<PyObject> {
    let type_name = data.get_type().name()?.to_string();
    let bytes = data.downcast::<PyBytes>().map_err(|_| {
        PyTypeError::new_err(format!("string_ranges() expected bytes, got {type_name}"))
    })?;
    let input = core::str::from_utf8(bytes.as_bytes())
        .map_err(|err| PyTypeError::new_err(format!("input bytes are not valid UTF-8: {err}")))?;

    collect_string_ranges(py, input)
}

/// Return packed byte ranges for JSON string values that can borrow from
/// `data`.
///
/// The result is little-endian `(start: u32, end: u32)` pairs. A pair of
/// `u32::MAX` values marks a string value that required materialization.
#[pyfunction]
#[pyo3(signature=(data))]
fn string_range_table(py: Python<'_>, data: Bound<'_, PyAny>) -> PyResult<PyObject> {
    let type_name = data.get_type().name()?.to_string();
    let bytes = data.downcast::<PyBytes>().map_err(|_| {
        PyTypeError::new_err(format!(
            "string_range_table() expected bytes, got {type_name}"
        ))
    })?;
    let input = core::str::from_utf8(bytes.as_bytes())
        .map_err(|err| PyTypeError::new_err(format!("input bytes are not valid UTF-8: {err}")))?;

    collect_string_range_table(py, input)
}

fn collect_string_ranges(py: Python<'_>, input: &str) -> PyResult<PyObject> {
    let mut parser = CoreJsonModem::new(CoreParserOptions::new());
    let ranges = PyList::empty(py);
    let mut string_was_fragmented = false;

    {
        let mut events = parser.feed(input);
        while let Some(item) = CoreLendingIterator::next(&mut events) {
            match item {
                Ok(event) => {
                    record_string_range(py, input, &ranges, event, &mut string_was_fragmented)?;
                }
                Err(err) => {
                    return Err(parser_error_to_py(
                        py,
                        &OwnedParserError {
                            message: err.to_string(),
                            line: err.line(),
                            column: err.column(),
                        },
                    ));
                }
            }
        }
    }

    loop {
        let mut produced = false;
        let mut events = parser.feed("");
        while let Some(item) = CoreLendingIterator::next(&mut events) {
            produced = true;
            match item {
                Ok(event) => {
                    record_string_range(py, input, &ranges, event, &mut string_was_fragmented)?;
                }
                Err(err) => {
                    return Err(parser_error_to_py(
                        py,
                        &OwnedParserError {
                            message: err.to_string(),
                            line: err.line(),
                            column: err.column(),
                        },
                    ));
                }
            }
        }
        drop(events);
        if !produced {
            break;
        }
    }

    let mut events = parser.finish();
    while let Some(item) = CoreLendingIterator::next(&mut events) {
        match item {
            Ok(event) => {
                record_string_range(py, input, &ranges, event, &mut string_was_fragmented)?;
            }
            Err(err) => {
                return Err(parser_error_to_py(
                    py,
                    &OwnedParserError {
                        message: err.to_string(),
                        line: err.line(),
                        column: err.column(),
                    },
                ));
            }
        }
    }

    Ok(ranges.into_any().unbind())
}

fn collect_string_range_table(py: Python<'_>, input: &str) -> PyResult<PyObject> {
    let table = scan_string_range_table(input.as_bytes())?;
    Ok(PyBytes::new(py, &table).into_any().unbind())
}

fn record_string_range(
    py: Python<'_>,
    input: &str,
    ranges: &Bound<'_, PyList>,
    event: ParseEvent<'_, &Path, StdBackend>,
    string_was_fragmented: &mut bool,
) -> PyResult<()> {
    let ParseEvent::String {
        fragment, is_final, ..
    } = event
    else {
        return Ok(());
    };

    if !is_final {
        *string_was_fragmented = true;
        return Ok(());
    }

    let range = if *string_was_fragmented {
        *string_was_fragmented = false;
        None
    } else if let Cow::Borrowed(fragment) = fragment {
        borrowed_range(input, fragment)
    } else {
        None
    };

    if let Some((start, end)) = range {
        ranges.append(PyTuple::new(py, [start, end])?)?;
    } else {
        ranges.append(py.None())?;
    }
    Ok(())
}

fn append_range_row(table: &mut Vec<u8>, range: Option<(usize, usize)>) -> PyResult<()> {
    let (start, end) = match range {
        Some((start, end)) => {
            let start = u32::try_from(start)
                .map_err(|_| PyException::new_err("string range start exceeds u32::MAX"))?;
            let end = u32::try_from(end)
                .map_err(|_| PyException::new_err("string range end exceeds u32::MAX"))?;
            (start, end)
        }
        None => (u32::MAX, u32::MAX),
    };
    table.extend_from_slice(&start.to_le_bytes());
    table.extend_from_slice(&end.to_le_bytes());
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ByteScanState {
    ArrayValueOrEnd,
    ArrayValue,
    ArrayAfterValue,
    ObjectKeyOrEnd,
    ObjectKey,
    ObjectAfterKey,
    ObjectValue,
    ObjectAfterValue,
}

fn scan_string_range_table(bytes: &[u8]) -> PyResult<Vec<u8>> {
    let mut table = Vec::new();
    let mut stack = Vec::new();
    let mut root_done = false;
    let mut index = 0usize;

    while index < bytes.len() {
        index = skip_json_whitespace(bytes, index);
        if index >= bytes.len() {
            break;
        }

        let state = stack.last().copied();
        match bytes[index] {
            b'{' if can_start_value(state, root_done) => {
                stack.push(ByteScanState::ObjectKeyOrEnd);
                index += 1;
            }
            b'[' if can_start_value(state, root_done) => {
                stack.push(ByteScanState::ArrayValueOrEnd);
                index += 1;
            }
            b'"' => match state {
                Some(ByteScanState::ObjectKeyOrEnd | ByteScanState::ObjectKey) => {
                    let (_, _, next, _) = scan_json_string(bytes, index)?;
                    *stack.last_mut().expect("object state") = ByteScanState::ObjectAfterKey;
                    index = next;
                }
                _ if can_start_value(state, root_done) => {
                    let (start, end, next, borrowed) = scan_json_string(bytes, index)?;
                    append_range_row(&mut table, borrowed.then_some((start, end)))?;
                    complete_value(&mut stack, &mut root_done)?;
                    index = next;
                }
                _ => return Err(scan_error("string was not valid in this position")),
            },
            b'}' => match state {
                Some(ByteScanState::ObjectKeyOrEnd | ByteScanState::ObjectAfterValue) => {
                    stack.pop();
                    complete_value(&mut stack, &mut root_done)?;
                    index += 1;
                }
                _ => return Err(scan_error("object close was not valid in this position")),
            },
            b']' => match state {
                Some(ByteScanState::ArrayValueOrEnd | ByteScanState::ArrayAfterValue) => {
                    stack.pop();
                    complete_value(&mut stack, &mut root_done)?;
                    index += 1;
                }
                _ => return Err(scan_error("array close was not valid in this position")),
            },
            b':' => match stack.last_mut() {
                Some(state @ ByteScanState::ObjectAfterKey) => {
                    *state = ByteScanState::ObjectValue;
                    index += 1;
                }
                _ => return Err(scan_error("colon was not valid in this position")),
            },
            b',' => match stack.last_mut() {
                Some(state @ ByteScanState::ArrayAfterValue) => {
                    *state = ByteScanState::ArrayValue;
                    index += 1;
                }
                Some(state @ ByteScanState::ObjectAfterValue) => {
                    *state = ByteScanState::ObjectKey;
                    index += 1;
                }
                _ => return Err(scan_error("comma was not valid in this position")),
            },
            b'n' | b't' | b'f' if can_start_value(state, root_done) => {
                index = skip_literal(bytes, index)?;
                complete_value(&mut stack, &mut root_done)?;
            }
            b'-' | b'0'..=b'9' if can_start_value(state, root_done) => {
                index = skip_number(bytes, index)?;
                complete_value(&mut stack, &mut root_done)?;
            }
            _ => return Err(scan_error("invalid byte in JSON input")),
        }
    }

    if !root_done || !stack.is_empty() {
        return Err(scan_error("JSON document ended before a complete value"));
    }

    Ok(table)
}

fn can_start_value(state: Option<ByteScanState>, root_done: bool) -> bool {
    match state {
        None => !root_done,
        Some(
            ByteScanState::ArrayValueOrEnd | ByteScanState::ArrayValue | ByteScanState::ObjectValue,
        ) => true,
        Some(
            ByteScanState::ArrayAfterValue
            | ByteScanState::ObjectKeyOrEnd
            | ByteScanState::ObjectKey
            | ByteScanState::ObjectAfterKey
            | ByteScanState::ObjectAfterValue,
        ) => false,
    }
}

fn complete_value(stack: &mut [ByteScanState], root_done: &mut bool) -> PyResult<()> {
    match stack.last_mut() {
        Some(state @ (ByteScanState::ArrayValueOrEnd | ByteScanState::ArrayValue)) => {
            *state = ByteScanState::ArrayAfterValue;
        }
        Some(state @ ByteScanState::ObjectValue) => {
            *state = ByteScanState::ObjectAfterValue;
        }
        None if !*root_done => {
            *root_done = true;
        }
        _ => return Err(scan_error("value ended in an invalid parser state")),
    }
    Ok(())
}

fn skip_json_whitespace(bytes: &[u8], mut index: usize) -> usize {
    while matches!(bytes.get(index), Some(b' ' | b'\n' | b'\r' | b'\t')) {
        index += 1;
    }
    index
}

fn scan_json_string(bytes: &[u8], quote: usize) -> PyResult<(usize, usize, usize, bool)> {
    let payload_start = quote + 1;
    let mut index = payload_start;
    let mut borrowed = true;

    while let Some(&byte) = bytes.get(index) {
        match byte {
            b'"' => return Ok((payload_start, index, index + 1, borrowed)),
            b'\\' => {
                borrowed = false;
                index += 1;
                let Some(&escape) = bytes.get(index) else {
                    return Err(scan_error("string escape ended early"));
                };
                match escape {
                    b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => index += 1,
                    b'u' => {
                        for offset in 1..=4 {
                            let Some(&digit) = bytes.get(index + offset) else {
                                return Err(scan_error("unicode escape ended early"));
                            };
                            if !digit.is_ascii_hexdigit() {
                                return Err(scan_error("unicode escape contained a non-hex digit"));
                            }
                        }
                        index += 5;
                    }
                    _ => return Err(scan_error("invalid string escape")),
                }
            }
            0x00..=0x1F => return Err(scan_error("unescaped control byte in string")),
            _ => index += 1,
        }
    }

    Err(scan_error("string ended early"))
}

fn skip_literal(bytes: &[u8], index: usize) -> PyResult<usize> {
    for literal in [b"null".as_slice(), b"true".as_slice(), b"false".as_slice()] {
        if bytes
            .get(index..index + literal.len())
            .is_some_and(|candidate| candidate == literal)
        {
            return Ok(index + literal.len());
        }
    }
    Err(scan_error("invalid literal"))
}

fn skip_number(bytes: &[u8], mut index: usize) -> PyResult<usize> {
    if matches!(bytes.get(index), Some(b'-')) {
        index += 1;
    }

    match bytes.get(index) {
        Some(b'0') => index += 1,
        Some(b'1'..=b'9') => {
            index += 1;
            while matches!(bytes.get(index), Some(b'0'..=b'9')) {
                index += 1;
            }
        }
        _ => return Err(scan_error("invalid number")),
    }

    if matches!(bytes.get(index), Some(b'.')) {
        index += 1;
        let digit_start = index;
        while matches!(bytes.get(index), Some(b'0'..=b'9')) {
            index += 1;
        }
        if index == digit_start {
            return Err(scan_error("number fraction had no digits"));
        }
    }

    if matches!(bytes.get(index), Some(b'e' | b'E')) {
        index += 1;
        if matches!(bytes.get(index), Some(b'+' | b'-')) {
            index += 1;
        }
        let digit_start = index;
        while matches!(bytes.get(index), Some(b'0'..=b'9')) {
            index += 1;
        }
        if index == digit_start {
            return Err(scan_error("number exponent had no digits"));
        }
    }

    Ok(index)
}

fn scan_error(message: &'static str) -> PyErr {
    PyException::new_err(message)
}

fn borrowed_range(input: &str, fragment: &str) -> Option<(usize, usize)> {
    let base = input.as_ptr() as usize;
    let ptr = fragment.as_ptr() as usize;
    let end = ptr.checked_add(fragment.len())?;
    let input_end = base.checked_add(input.len())?;
    if ptr < base || end > input_end {
        return None;
    }
    Some((ptr - base, end - base))
}

fn drain_native_pending(
    py: Python<'_>,
    parser: &mut CoreJsonModem<StdBackend>,
    builder: &mut NativeBuilder,
) -> PyResult<()> {
    loop {
        let mut produced = false;
        let mut events = parser.feed("");
        while let Some(item) = CoreLendingIterator::next(&mut events) {
            produced = true;
            match item {
                Ok(event) => builder.handle_event(py, event)?,
                Err(err) => {
                    return Err(parser_error_to_py(
                        py,
                        &OwnedParserError {
                            message: err.to_string(),
                            line: err.line(),
                            column: err.column(),
                        },
                    ));
                }
            }
        }
        drop(events);
        if !produced {
            break;
        }
    }
    Ok(())
}

fn register_decode_mode_constants(py: Python<'_>) -> PyResult<()> {
    let ty = py.get_type::<PyDecodeMode>();
    ty.setattr(
        "StrictUnicode",
        PyDecodeMode::new_instance(py, DecodeMode::StrictUnicode)?,
    )?;
    ty.setattr(
        "SurrogatePreserving",
        PyDecodeMode::new_instance(py, DecodeMode::SurrogatePreserving)?,
    )?;
    ty.setattr(
        "ReplaceInvalid",
        PyDecodeMode::new_instance(py, DecodeMode::ReplaceInvalid)?,
    )?;
    Ok(())
}

/// jsonmodem Python bindings
#[pymodule]
#[pyo3(name = "_jsonmodem")]
fn jsonmodem(py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add(
        "__doc__",
        concat!(
            "jsonmodem: streaming JSON parser bindings for Python.\n\n",
            "Use JsonModem to feed chunked JSON input and observe incremental parse events without sacrificing performance."
        ),
    )?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add_class::<PyDecodeMode>()?;
    register_decode_mode_constants(py)?;
    m.add_class::<PyParserOptions>()?;
    m.add_class::<PyJsonModem>()?;
    m.add_class::<PyEventIter>()?;
    m.add_class::<PyPathView>()?;
    m.add_class::<PyStringPayload>()?;
    m.add_class::<PyJsonModemByteViews>()?;
    m.add_class::<PyByteEventIter>()?;
    m.add_class::<PyJsonModemPathFilter>()?;
    m.add_function(wrap_pyfunction!(loads, m)?)?;
    m.add_function(wrap_pyfunction!(string_ranges, m)?)?;
    m.add_function(wrap_pyfunction!(string_range_table, m)?)?;
    m.add(
        "JsonModemSyntaxError",
        py.get_type::<JsonModemSyntaxError>(),
    )?;
    m.add("JsonModemStateError", py.get_type::<JsonModemStateError>())?;

    py.get_type::<JsonModemSyntaxError>().setattr(
        "__doc__",
        "Raised when the input stream contains invalid JSON syntax.",
    )?;
    py.get_type::<JsonModemStateError>().setattr(
        "__doc__",
        "Raised when JsonModem is used after finish() or in an invalid state.",
    )?;

    Ok(())
}
