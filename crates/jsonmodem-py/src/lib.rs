use std::{
    borrow::Cow,
    cell::RefCell,
    collections::BTreeMap,
    os::raw::{c_int, c_void},
    rc::Rc,
    sync::{Arc, Mutex},
};

use ::jsonmodem::{
    DecodeMode as CoreDecodeMode, JsonModem as CoreJsonModem,
    JsonModemValues as CoreJsonModemValues, ParseEvent, ParserOptions as CoreParserOptions, Path,
    PathItem, StdBackend, StreamingValue as CoreStreamingValue, Value as CoreValue,
    ValuesError as CoreValuesError, ValuesOptions as CoreValuesOptions,
    lending_iterator::LendingIterator as CoreLendingIterator,
};
use pyo3::{
    class::basic::CompareOp,
    create_exception,
    exceptions::{PyException, PyIndexError, PyTypeError},
    ffi,
    prelude::*,
    types::{
        PyAny, PyBool, PyBytes, PyDict, PyList, PyMemoryView, PySlice, PyString, PyStringMethods,
        PyTuple,
    },
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
    patterns: Option<Vec<PathPattern>>,
    byte_views: bool,
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
    #[pyo3(signature=(options=None, *, paths=None, byte_views=false))]
    fn new(
        py: Python<'_>,
        options: Option<Bound<'_, PyAny>>,
        paths: Option<Bound<'_, PyAny>>,
        byte_views: bool,
    ) -> PyResult<Self> {
        let parsed_options = match options {
            Some(item) => read_parser_options(item)?,
            None => PyParserOptions::default(),
        };
        let patterns = match paths {
            Some(item) if !item.is_none() => Some(read_path_patterns(&item)?),
            _ => None,
        };

        Ok(Self {
            parser: Some(CoreJsonModem::new(parsed_options.to_core())),
            finished: false,
            patterns,
            byte_views,
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
    fn feed(&mut self, py: Python<'_>, chunk_or_chunks: Bound<'_, PyAny>) -> PyResult<PyObject> {
        let parser = self
            .parser
            .as_mut()
            .ok_or_else(|| state_error("parser has already finished"))?;

        if self.byte_views {
            let patterns = self.patterns.as_deref();
            let mut records = Vec::new();
            if is_single_byte_view_input(&chunk_or_chunks) {
                return with_readonly_byte_text(
                    py,
                    &chunk_or_chunks,
                    "JsonModem.feed()",
                    |text, source| {
                        let records = match patterns {
                            Some(patterns) => collect_filtered_byte_view_feed_events(
                                py, parser, text, source, patterns,
                            )?,
                            None => collect_byte_view_feed_events(py, parser, text, source)?,
                        };
                        Ok(
                            PyByteEventIter::new(py, records, self.interns.clone_ref(py))?
                                .into_any(),
                        )
                    },
                );
            }

            for item in chunk_or_chunks.try_iter()? {
                let chunk = item?;
                let mut chunk_records =
                    with_readonly_byte_text(py, &chunk, "JsonModem.feed()", |text, source| {
                        match patterns {
                            Some(patterns) => collect_filtered_byte_view_feed_events(
                                py, parser, text, source, patterns,
                            ),
                            None => collect_byte_view_feed_events(py, parser, text, source),
                        }
                    })?;
                let has_error = chunk_records
                    .iter()
                    .any(|record| matches!(record, ByteViewRecord::Error(_)));
                records.append(&mut chunk_records);
                if has_error {
                    break;
                }
            }
            return Ok(PyByteEventIter::new(py, records, self.interns.clone_ref(py))?.into_any());
        }

        let interns = self.interns.clone_ref(py);
        let record_pool = Arc::clone(&self.record_pool);
        let patterns = self.patterns.as_deref();
        if is_single_json_input(&chunk_or_chunks) {
            return with_input_text(py, &chunk_or_chunks, "feed()", |chunk| {
                let mut records = take_event_records(&record_pool);
                match patterns {
                    Some(patterns) => collect_filtered_view_feed_events(
                        py,
                        parser,
                        chunk,
                        patterns,
                        &interns,
                        &mut records,
                    )?,
                    None => collect_feed_events(py, parser, chunk, &interns, &mut records)?,
                }
                Ok(PyEventIter::new(py, records, record_pool)?.into_any())
            });
        }

        let mut records = take_event_records(&record_pool);
        for item in chunk_or_chunks.try_iter()? {
            let chunk = item?;
            with_input_text(py, &chunk, "feed()", |chunk| match patterns {
                Some(patterns) => collect_filtered_view_feed_events(
                    py,
                    parser,
                    chunk,
                    patterns,
                    &interns,
                    &mut records,
                ),
                None => collect_feed_events(py, parser, chunk, &interns, &mut records),
            })?;
            if matches!(records.last(), Some(EventRecord::Error(_))) {
                break;
            }
        }
        Ok(PyEventIter::new(py, records, record_pool)?.into_any())
    }

    /// Mark the parser as complete and emit any buffered trailing events.
    ///
    /// After `finish()` returns, subsequent calls to `feed()` raise
    /// `JsonModemStateError`.  The returned iterator may still surface syntax
    /// errors (for example, trailing garbage once the document is closed).
    #[pyo3(text_signature = "($self)")]
    fn finish(&mut self, py: Python<'_>) -> PyResult<PyObject> {
        if self.finished {
            return Err(state_error("finish() has already been called"));
        }

        let parser = self
            .parser
            .take()
            .ok_or_else(|| state_error("parser has already finished"))?;

        if self.byte_views {
            let records = match self.patterns.as_deref() {
                Some(patterns) => collect_filtered_byte_view_finish_events(py, parser, patterns)?,
                None => collect_byte_view_finish_events(py, parser)?,
            };
            self.finished = true;
            return Ok(PyByteEventIter::new(py, records, self.interns.clone_ref(py))?.into_any());
        }

        let mut records = take_event_records(&self.record_pool);
        let interns = self.interns.clone_ref(py);
        match self.patterns.as_deref() {
            Some(patterns) => {
                collect_filtered_view_finish_events(py, parser, patterns, &interns, &mut records)?;
            }
            None => collect_finish_events(py, parser, &interns, &mut records)?,
        }
        self.finished = true;
        Ok(PyEventIter::new(py, records, Arc::clone(&self.record_pool))?.into_any())
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

enum ValueRecord {
    Value(PyObject),
    Error(OwnedParserError),
    Consumed,
}

/// Streaming JSON parser that yields native Python value snapshots.
///
/// This is retained as an internal benchmark/control adapter. The public
/// `JsonModemValues` API is the read-only view parser below.
#[pyclass(
    module = "jsonmodem._jsonmodem",
    name = "JsonModemValueSnapshots",
    unsendable
)]
struct PyJsonModemValues {
    parser: Option<CoreJsonModemValues<StdBackend>>,
    finished: bool,
}

#[pymethods]
impl PyJsonModemValues {
    /// Construct an incremental value parser.
    ///
    /// Parameters
    /// ----------
    /// options:
    ///     Optional `ParserOptions` instance.  When omitted, defaults are used.
    /// partial:
    ///     Emit non-final snapshots while a root JSON value is still being
    ///     parsed.  This defaults to `True` because this class is meant for
    ///     partial stream consumers.
    #[new]
    #[pyo3(signature=(options=None, *, partial=true))]
    fn new(options: Option<Bound<'_, PyAny>>, partial: bool) -> PyResult<Self> {
        let parsed_options = match options {
            Some(item) => read_parser_options(item)?,
            None => PyParserOptions::default(),
        };
        let values_options = CoreValuesOptions::default().with_partial(partial);

        Ok(Self {
            parser: Some(CoreJsonModemValues::with_options(
                parsed_options.to_core(),
                values_options,
            )),
            finished: false,
        })
    }

    /// Feed UTF-8 JSON to the parser and get value snapshots.
    ///
    /// `chunk_or_chunks` accepts the same input types as `JsonModem.feed()`: a
    /// single `str`, `bytes`, `bytearray`, or contiguous `memoryview`, or an
    /// iterable of those chunk types.
    #[pyo3(text_signature = "($self, chunk_or_chunks)")]
    fn feed(
        &mut self,
        py: Python<'_>,
        chunk_or_chunks: Bound<'_, PyAny>,
    ) -> PyResult<Py<PyValueIter>> {
        let parser = self
            .parser
            .as_mut()
            .ok_or_else(|| state_error("parser has already finished"))?;

        let mut records = Vec::new();
        if is_single_json_input(&chunk_or_chunks) {
            return with_input_text(py, &chunk_or_chunks, "JsonModemValues.feed()", |chunk| {
                collect_value_feed(py, parser, chunk, &mut records)?;
                PyValueIter::new(py, records)
            });
        }

        for item in chunk_or_chunks.try_iter()? {
            let chunk = item?;
            with_input_text(py, &chunk, "JsonModemValues.feed()", |chunk| {
                collect_value_feed(py, parser, chunk, &mut records)
            })?;
            if matches!(records.last(), Some(ValueRecord::Error(_))) {
                break;
            }
        }
        PyValueIter::new(py, records)
    }

    /// Return a snapshot of the current root value.
    ///
    /// Before any input arrives this returns `None`, matching the Rust value
    /// adapter's empty root.
    fn view(&self, py: Python<'_>) -> PyResult<PyObject> {
        let parser = self
            .parser
            .as_ref()
            .ok_or_else(|| state_error("parser has already finished"))?;
        value_to_py(py, parser.view_root())
    }

    /// Mark the parser as complete and emit any remaining value snapshots.
    #[pyo3(text_signature = "($self)")]
    fn finish(&mut self, py: Python<'_>) -> PyResult<Py<PyValueIter>> {
        if self.finished {
            return Err(state_error("finish() has already been called"));
        }

        let parser = self
            .parser
            .take()
            .ok_or_else(|| state_error("parser has already finished"))?;
        let mut records = Vec::new();
        collect_value_finish(py, parser, &mut records)?;
        self.finished = true;
        PyValueIter::new(py, records)
    }

    /// `True` once the parser has been exhausted or `finish()` was called.
    #[getter]
    fn is_finished(&self) -> bool {
        self.finished
    }
}

/// Iterator over value snapshots produced by `JsonModemValues`.
#[pyclass(module = "jsonmodem._jsonmodem")]
struct PyValueIter {
    records: Vec<ValueRecord>,
    index: usize,
}

impl PyValueIter {
    fn new(py: Python<'_>, records: Vec<ValueRecord>) -> PyResult<Py<PyValueIter>> {
        Py::new(py, PyValueIter { records, index: 0 })
    }
}

#[pymethods]
impl PyValueIter {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    /// Yield the next `(index, value, is_final)` tuple or raise
    /// `StopIteration`.
    fn __next__<'py>(&mut self, py: Python<'py>) -> PyResult<Option<PyObject>> {
        if self.index >= self.records.len() {
            return Ok(None);
        }

        let entry = std::mem::replace(&mut self.records[self.index], ValueRecord::Consumed);
        self.index += 1;

        match entry {
            ValueRecord::Value(value) => Ok(Some(value)),
            ValueRecord::Error(err) => Err(parser_error_to_py(py, &err)),
            ValueRecord::Consumed => Ok(None),
        }
    }
}

/// Streaming JSON parser that mutates one Python root object in place.
///
/// `JsonModemMutableValues` is for consumers that want ordinary Python
/// `dict`/`list` containers and can process change notifications.  Each
/// yielded tuple is `(index, root, path, is_final)`, where `root` is the
/// current Python root object and `path` identifies the field changed by the
/// event.
#[pyclass(
    module = "jsonmodem._jsonmodem",
    name = "JsonModemMutableValues",
    unsendable
)]
struct PyJsonModemMutableValues {
    parser: Option<CoreJsonModem<StdBackend>>,
    root: Option<PyObject>,
    next_index: usize,
    finished: bool,
}

#[pymethods]
impl PyJsonModemMutableValues {
    #[new]
    #[pyo3(signature=(options=None))]
    fn new(options: Option<Bound<'_, PyAny>>) -> PyResult<Self> {
        let parsed_options = match options {
            Some(item) => read_parser_options(item)?,
            None => PyParserOptions::default(),
        };

        Ok(Self {
            parser: Some(CoreJsonModem::new(parsed_options.to_core())),
            root: None,
            next_index: 0,
            finished: false,
        })
    }

    /// Feed UTF-8 JSON and get `(index, root, path, is_final)` updates.
    #[pyo3(text_signature = "($self, chunk_or_chunks)")]
    fn feed(
        &mut self,
        py: Python<'_>,
        chunk_or_chunks: Bound<'_, PyAny>,
    ) -> PyResult<Py<PyValueIter>> {
        let parser = self
            .parser
            .as_mut()
            .ok_or_else(|| state_error("parser has already finished"))?;

        let mut records = Vec::new();
        if is_single_json_input(&chunk_or_chunks) {
            return with_input_text(
                py,
                &chunk_or_chunks,
                "JsonModemMutableValues.feed()",
                |chunk| {
                    collect_mutable_value_feed(
                        py,
                        parser,
                        chunk,
                        &mut self.root,
                        &mut self.next_index,
                        &mut records,
                    )?;
                    PyValueIter::new(py, records)
                },
            );
        }

        for item in chunk_or_chunks.try_iter()? {
            let chunk = item?;
            with_input_text(py, &chunk, "JsonModemMutableValues.feed()", |chunk| {
                collect_mutable_value_feed(
                    py,
                    parser,
                    chunk,
                    &mut self.root,
                    &mut self.next_index,
                    &mut records,
                )
            })?;
            if matches!(records.last(), Some(ValueRecord::Error(_))) {
                break;
            }
        }
        PyValueIter::new(py, records)
    }

    /// Return the current Python root object, or `None` before input arrives.
    fn view(&self, py: Python<'_>) -> PyObject {
        self.root
            .as_ref()
            .map(|root| root.clone_ref(py))
            .unwrap_or_else(|| py.None())
    }

    /// Mark the parser as complete and emit any remaining updates.
    #[pyo3(text_signature = "($self)")]
    fn finish(&mut self, py: Python<'_>) -> PyResult<Py<PyValueIter>> {
        if self.finished {
            return Err(state_error("finish() has already been called"));
        }

        let parser = self
            .parser
            .take()
            .ok_or_else(|| state_error("parser has already finished"))?;
        let mut records = Vec::new();
        collect_mutable_value_finish(
            py,
            parser,
            &mut self.root,
            &mut self.next_index,
            &mut records,
        )?;
        self.finished = true;
        PyValueIter::new(py, records)
    }

    #[getter]
    fn is_finished(&self) -> bool {
        self.finished
    }
}

/// Read-only view into the current incremental value tree.
#[pyclass(
    module = "jsonmodem._jsonmodem",
    name = "JsonModemValueView",
    unsendable
)]
struct PyJsonModemValueView {
    root: Rc<RefCell<Option<CoreValue>>>,
    path: Vec<OwnedPathComponent>,
}

#[pymethods]
impl PyJsonModemValueView {
    /// Return this view as a normal Python value.
    fn snapshot(&self, py: Python<'_>) -> PyResult<PyObject> {
        let root = self.root.borrow();
        match core_value_at_path(&root, &self.path) {
            Some(value) => value_to_py(py, value),
            None => Ok(py.None()),
        }
    }

    /// Return a nested read-only value view.
    fn __getitem__(
        &self,
        py: Python<'_>,
        key: Bound<'_, PyAny>,
    ) -> PyResult<Py<PyJsonModemValueView>> {
        let mut path = self.path.clone();
        {
            let root = self.root.borrow();
            let value = core_value_at_path(&root, &self.path)
                .ok_or_else(|| PyIndexError::new_err("value view is empty"))?;
            match value {
                CoreValue::Object(map) => {
                    let key: String = key.extract()?;
                    if !map.contains_key(key.as_str()) {
                        return Err(PyIndexError::new_err(format!("missing object key {key:?}")));
                    }
                    path.push(OwnedPathComponent::Key(key));
                }
                CoreValue::Array(values) => {
                    let index: usize = key.extract()?;
                    if index >= values.len() {
                        return Err(PyIndexError::new_err(format!(
                            "array index {index} out of range"
                        )));
                    }
                    path.push(OwnedPathComponent::Index(index));
                }
                _ => {
                    return Err(PyTypeError::new_err(
                        "value view only supports indexing arrays and objects",
                    ));
                }
            }
        }

        Py::new(
            py,
            PyJsonModemValueView {
                root: Rc::clone(&self.root),
                path,
            },
        )
    }

    fn __len__(&self) -> PyResult<usize> {
        let root = self.root.borrow();
        match core_value_at_path(&root, &self.path) {
            Some(CoreValue::Array(values)) => Ok(values.len()),
            Some(CoreValue::Object(map)) => Ok(map.len()),
            Some(CoreValue::String(value)) => Ok(value.chars().count()),
            Some(_) => Err(PyTypeError::new_err("value view has no length")),
            None => Ok(0),
        }
    }

    #[getter]
    fn kind(&self) -> &'static str {
        let root = self.root.borrow();
        match core_value_at_path(&root, &self.path) {
            Some(CoreValue::Null) => "null",
            Some(CoreValue::Boolean(_)) => "bool",
            Some(CoreValue::Number(_)) => "number",
            Some(CoreValue::String(_)) => "string",
            Some(CoreValue::Array(_)) => "array",
            Some(CoreValue::Object(_)) => "object",
            None => "empty",
        }
    }

    #[getter]
    fn path(&self, py: Python<'_>) -> PyResult<PyObject> {
        build_path_tuple_for_event(py, &self.path)
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let snapshot = self.snapshot(py)?;
        Ok(format!("JsonModemValueView({})", snapshot.bind(py).repr()?))
    }
}

/// Streaming JSON parser that returns read-only views and changed paths.
///
/// Each yielded tuple is `(index, view, path, is_final)`.  The view points at
/// the current root and can be inspected with `snapshot()` or `__getitem__()`;
/// it does not build a Python `dict`/`list` unless requested.
#[pyclass(
    module = "jsonmodem._jsonmodem",
    name = "JsonModemValueViews",
    unsendable
)]
struct PyJsonModemValueViews {
    parser: Option<CoreJsonModem<StdBackend>>,
    root: Rc<RefCell<Option<CoreValue>>>,
    next_index: usize,
    finished: bool,
}

#[pymethods]
impl PyJsonModemValueViews {
    #[new]
    #[pyo3(signature=(options=None))]
    fn new(options: Option<Bound<'_, PyAny>>) -> PyResult<Self> {
        let parsed_options = match options {
            Some(item) => read_parser_options(item)?,
            None => PyParserOptions::default(),
        };

        Ok(Self {
            parser: Some(CoreJsonModem::new(parsed_options.to_core())),
            root: Rc::new(RefCell::new(None)),
            next_index: 0,
            finished: false,
        })
    }

    /// Feed UTF-8 JSON and get `(index, view, path, is_final)` updates.
    #[pyo3(text_signature = "($self, chunk_or_chunks)")]
    fn feed(
        &mut self,
        py: Python<'_>,
        chunk_or_chunks: Bound<'_, PyAny>,
    ) -> PyResult<Py<PyValueIter>> {
        let parser = self
            .parser
            .as_mut()
            .ok_or_else(|| state_error("parser has already finished"))?;

        let mut records = Vec::new();
        if is_single_json_input(&chunk_or_chunks) {
            return with_input_text(
                py,
                &chunk_or_chunks,
                "JsonModemValueViews.feed()",
                |chunk| {
                    collect_view_value_feed(
                        py,
                        parser,
                        chunk,
                        &self.root,
                        &mut self.next_index,
                        &mut records,
                    )?;
                    PyValueIter::new(py, records)
                },
            );
        }

        for item in chunk_or_chunks.try_iter()? {
            let chunk = item?;
            with_input_text(py, &chunk, "JsonModemValueViews.feed()", |chunk| {
                collect_view_value_feed(
                    py,
                    parser,
                    chunk,
                    &self.root,
                    &mut self.next_index,
                    &mut records,
                )
            })?;
            if matches!(records.last(), Some(ValueRecord::Error(_))) {
                break;
            }
        }
        PyValueIter::new(py, records)
    }

    /// Return a read-only view of the current root value.
    fn view(&self, py: Python<'_>) -> PyResult<Py<PyJsonModemValueView>> {
        Py::new(
            py,
            PyJsonModemValueView {
                root: Rc::clone(&self.root),
                path: Vec::new(),
            },
        )
    }

    /// Mark the parser as complete and emit any remaining updates.
    #[pyo3(text_signature = "($self)")]
    fn finish(&mut self, py: Python<'_>) -> PyResult<Py<PyValueIter>> {
        if self.finished {
            return Err(state_error("finish() has already been called"));
        }

        let parser = self
            .parser
            .take()
            .ok_or_else(|| state_error("parser has already finished"))?;
        let mut records = Vec::new();
        collect_view_value_finish(py, parser, &self.root, &mut self.next_index, &mut records)?;
        self.finished = true;
        PyValueIter::new(py, records)
    }

    #[getter]
    fn is_finished(&self) -> bool {
        self.finished
    }
}

/// Experimental value-view parser that reuses one root view object.
#[pyclass(
    module = "jsonmodem._jsonmodem",
    name = "JsonModemValueViewsCached",
    unsendable
)]
struct PyJsonModemValueViewsCached {
    parser: Option<CoreJsonModem<StdBackend>>,
    root: Rc<RefCell<Option<CoreValue>>>,
    root_view: Py<PyJsonModemValueView>,
    next_index: usize,
    finished: bool,
}

#[pymethods]
impl PyJsonModemValueViewsCached {
    #[new]
    #[pyo3(signature=(options=None))]
    fn new(py: Python<'_>, options: Option<Bound<'_, PyAny>>) -> PyResult<Self> {
        let parsed_options = match options {
            Some(item) => read_parser_options(item)?,
            None => PyParserOptions::default(),
        };
        let root = Rc::new(RefCell::new(None));
        let root_view = Py::new(
            py,
            PyJsonModemValueView {
                root: Rc::clone(&root),
                path: Vec::new(),
            },
        )?;

        Ok(Self {
            parser: Some(CoreJsonModem::new(parsed_options.to_core())),
            root,
            root_view,
            next_index: 0,
            finished: false,
        })
    }

    /// Feed UTF-8 JSON and get `(index, view, path, is_final)` updates.
    #[pyo3(text_signature = "($self, chunk_or_chunks)")]
    fn feed(
        &mut self,
        py: Python<'_>,
        chunk_or_chunks: Bound<'_, PyAny>,
    ) -> PyResult<Py<PyValueIter>> {
        let parser = self
            .parser
            .as_mut()
            .ok_or_else(|| state_error("parser has already finished"))?;

        let mut records = Vec::new();
        if is_single_json_input(&chunk_or_chunks) {
            return with_input_text(
                py,
                &chunk_or_chunks,
                "JsonModemValueViewsCached.feed()",
                |chunk| {
                    collect_view_value_feed_with(
                        py,
                        parser,
                        chunk,
                        &self.root,
                        &mut self.next_index,
                        &mut records,
                        |py, index, path, is_final| {
                            cached_view_update_record(py, index, &self.root_view, &path, is_final)
                        },
                    )?;
                    PyValueIter::new(py, records)
                },
            );
        }

        for item in chunk_or_chunks.try_iter()? {
            let chunk = item?;
            with_input_text(py, &chunk, "JsonModemValueViewsCached.feed()", |chunk| {
                collect_view_value_feed_with(
                    py,
                    parser,
                    chunk,
                    &self.root,
                    &mut self.next_index,
                    &mut records,
                    |py, index, path, is_final| {
                        cached_view_update_record(py, index, &self.root_view, &path, is_final)
                    },
                )
            })?;
            if matches!(records.last(), Some(ValueRecord::Error(_))) {
                break;
            }
        }
        PyValueIter::new(py, records)
    }

    /// Return the cached root view.
    fn view(&self, py: Python<'_>) -> Py<PyJsonModemValueView> {
        self.root_view.clone_ref(py)
    }

    /// Mark the parser as complete and emit any remaining updates.
    #[pyo3(text_signature = "($self)")]
    fn finish(&mut self, py: Python<'_>) -> PyResult<Py<PyValueIter>> {
        if self.finished {
            return Err(state_error("finish() has already been called"));
        }

        let parser = self
            .parser
            .take()
            .ok_or_else(|| state_error("parser has already finished"))?;
        let mut records = Vec::new();
        collect_view_value_finish_with(
            py,
            parser,
            &self.root,
            &mut self.next_index,
            &mut records,
            |py, index, path, is_final| {
                cached_view_update_record(py, index, &self.root_view, &path, is_final)
            },
        )?;
        self.finished = true;
        PyValueIter::new(py, records)
    }

    #[getter]
    fn is_finished(&self) -> bool {
        self.finished
    }
}

/// Experimental value-view parser that emits changed paths only.
#[pyclass(
    module = "jsonmodem._jsonmodem",
    name = "JsonModemValuePaths",
    unsendable
)]
struct PyJsonModemValuePaths {
    parser: Option<CoreJsonModem<StdBackend>>,
    root: Rc<RefCell<Option<CoreValue>>>,
    next_index: usize,
    finished: bool,
}

#[pymethods]
impl PyJsonModemValuePaths {
    #[new]
    #[pyo3(signature=(options=None))]
    fn new(options: Option<Bound<'_, PyAny>>) -> PyResult<Self> {
        let parsed_options = match options {
            Some(item) => read_parser_options(item)?,
            None => PyParserOptions::default(),
        };

        Ok(Self {
            parser: Some(CoreJsonModem::new(parsed_options.to_core())),
            root: Rc::new(RefCell::new(None)),
            next_index: 0,
            finished: false,
        })
    }

    /// Feed UTF-8 JSON and get `(index, path, is_final)` updates.
    #[pyo3(text_signature = "($self, chunk_or_chunks)")]
    fn feed(
        &mut self,
        py: Python<'_>,
        chunk_or_chunks: Bound<'_, PyAny>,
    ) -> PyResult<Py<PyValueIter>> {
        let parser = self
            .parser
            .as_mut()
            .ok_or_else(|| state_error("parser has already finished"))?;

        let mut records = Vec::new();
        if is_single_json_input(&chunk_or_chunks) {
            return with_input_text(
                py,
                &chunk_or_chunks,
                "JsonModemValuePaths.feed()",
                |chunk| {
                    collect_view_value_feed_with(
                        py,
                        parser,
                        chunk,
                        &self.root,
                        &mut self.next_index,
                        &mut records,
                        path_only_update_record,
                    )?;
                    PyValueIter::new(py, records)
                },
            );
        }

        for item in chunk_or_chunks.try_iter()? {
            let chunk = item?;
            with_input_text(py, &chunk, "JsonModemValuePaths.feed()", |chunk| {
                collect_view_value_feed_with(
                    py,
                    parser,
                    chunk,
                    &self.root,
                    &mut self.next_index,
                    &mut records,
                    path_only_update_record,
                )
            })?;
            if matches!(records.last(), Some(ValueRecord::Error(_))) {
                break;
            }
        }
        PyValueIter::new(py, records)
    }

    /// Return a read-only view of the current root value.
    fn view(&self, py: Python<'_>) -> PyResult<Py<PyJsonModemValueView>> {
        Py::new(
            py,
            PyJsonModemValueView {
                root: Rc::clone(&self.root),
                path: Vec::new(),
            },
        )
    }

    /// Mark the parser as complete and emit any remaining updates.
    #[pyo3(text_signature = "($self)")]
    fn finish(&mut self, py: Python<'_>) -> PyResult<Py<PyValueIter>> {
        if self.finished {
            return Err(state_error("finish() has already been called"));
        }

        let parser = self
            .parser
            .take()
            .ok_or_else(|| state_error("parser has already finished"))?;
        let mut records = Vec::new();
        collect_view_value_finish_with(
            py,
            parser,
            &self.root,
            &mut self.next_index,
            &mut records,
            path_only_update_record,
        )?;
        self.finished = true;
        PyValueIter::new(py, records)
    }

    #[getter]
    fn is_finished(&self) -> bool {
        self.finished
    }
}

/// Streaming JSON parser that returns a reused read-only root view.
#[pyclass(module = "jsonmodem._jsonmodem", name = "JsonModemValues", unsendable)]
struct PyJsonModemValueViewsPathView {
    parser: Option<CoreJsonModem<StdBackend>>,
    root: Rc<RefCell<Option<CoreValue>>>,
    root_view: Py<PyJsonModemValueView>,
    next_index: usize,
    finished: bool,
}

#[pymethods]
impl PyJsonModemValueViewsPathView {
    #[new]
    #[pyo3(signature=(options=None))]
    fn new(py: Python<'_>, options: Option<Bound<'_, PyAny>>) -> PyResult<Self> {
        let parsed_options = match options {
            Some(item) => read_parser_options(item)?,
            None => PyParserOptions::default(),
        };
        let root = Rc::new(RefCell::new(None));
        let root_view = Py::new(
            py,
            PyJsonModemValueView {
                root: Rc::clone(&root),
                path: Vec::new(),
            },
        )?;

        Ok(Self {
            parser: Some(CoreJsonModem::new(parsed_options.to_core())),
            root,
            root_view,
            next_index: 0,
            finished: false,
        })
    }

    /// Feed UTF-8 JSON and get `(index, view, path_view, is_final)` updates.
    #[pyo3(text_signature = "($self, chunk_or_chunks)")]
    fn feed(
        &mut self,
        py: Python<'_>,
        chunk_or_chunks: Bound<'_, PyAny>,
    ) -> PyResult<Py<PyValueIter>> {
        let parser = self
            .parser
            .as_mut()
            .ok_or_else(|| state_error("parser has already finished"))?;

        let mut records = Vec::new();
        if is_single_json_input(&chunk_or_chunks) {
            return with_input_text(py, &chunk_or_chunks, "JsonModemValues.feed()", |chunk| {
                collect_view_value_feed_with(
                    py,
                    parser,
                    chunk,
                    &self.root,
                    &mut self.next_index,
                    &mut records,
                    |py, index, path, is_final| {
                        cached_view_path_view_update_record(
                            py,
                            index,
                            &self.root_view,
                            path,
                            is_final,
                        )
                    },
                )?;
                PyValueIter::new(py, records)
            });
        }

        for item in chunk_or_chunks.try_iter()? {
            let chunk = item?;
            with_input_text(py, &chunk, "JsonModemValues.feed()", |chunk| {
                collect_view_value_feed_with(
                    py,
                    parser,
                    chunk,
                    &self.root,
                    &mut self.next_index,
                    &mut records,
                    |py, index, path, is_final| {
                        cached_view_path_view_update_record(
                            py,
                            index,
                            &self.root_view,
                            path,
                            is_final,
                        )
                    },
                )
            })?;
            if matches!(records.last(), Some(ValueRecord::Error(_))) {
                break;
            }
        }
        PyValueIter::new(py, records)
    }

    /// Return the cached root view.
    fn view(&self, py: Python<'_>) -> Py<PyJsonModemValueView> {
        self.root_view.clone_ref(py)
    }

    /// Mark the parser as complete and emit any remaining updates.
    #[pyo3(text_signature = "($self)")]
    fn finish(&mut self, py: Python<'_>) -> PyResult<Py<PyValueIter>> {
        if self.finished {
            return Err(state_error("finish() has already been called"));
        }

        let parser = self
            .parser
            .take()
            .ok_or_else(|| state_error("parser has already finished"))?;
        let mut records = Vec::new();
        collect_view_value_finish_with(
            py,
            parser,
            &self.root,
            &mut self.next_index,
            &mut records,
            |py, index, path, is_final| {
                cached_view_path_view_update_record(py, index, &self.root_view, path, is_final)
            },
        )?;
        self.finished = true;
        PyValueIter::new(py, records)
    }

    #[getter]
    fn is_finished(&self) -> bool {
        self.finished
    }
}

/// Iterator over byte-view streaming events produced by `JsonModem`.
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

fn collect_filtered_view_feed_events(
    py: Python<'_>,
    parser: &mut CoreJsonModem<StdBackend>,
    chunk: &str,
    patterns: &[PathPattern],
    interns: &InternedStrings,
    records: &mut Vec<EventRecord>,
) -> PyResult<()> {
    let mut events = parser.feed(chunk);
    while let Some(item) = CoreLendingIterator::next(&mut events) {
        match item {
            Ok(event) => {
                if path_matches_patterns(event.path(), patterns) {
                    records.push(view_event_record(py, event, interns)?);
                }
            }
            Err(err) => {
                records.push(error_record(err.to_string(), err.line(), err.column()));
                return Ok(());
            }
        }
    }
    drop(events);
    drain_filtered_view_pending_events(py, parser, patterns, interns, records)
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

fn collect_filtered_view_finish_events(
    py: Python<'_>,
    parser: CoreJsonModem<StdBackend>,
    patterns: &[PathPattern],
    interns: &InternedStrings,
    records: &mut Vec<EventRecord>,
) -> PyResult<()> {
    let mut events = parser.finish();
    while let Some(item) = CoreLendingIterator::next(&mut events) {
        match item {
            Ok(event) => {
                if path_matches_patterns(event.path(), patterns) {
                    records.push(view_event_record(py, event, interns)?);
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

fn collect_value_feed(
    py: Python<'_>,
    parser: &mut CoreJsonModemValues<StdBackend>,
    chunk: &str,
    records: &mut Vec<ValueRecord>,
) -> PyResult<()> {
    for item in parser.feed(chunk) {
        match item {
            Ok(value) => records.push(streaming_value_record(py, value)?),
            Err(err) => {
                records.push(values_error_record(err));
                return Ok(());
            }
        }
    }
    Ok(())
}

fn collect_value_finish(
    py: Python<'_>,
    parser: CoreJsonModemValues<StdBackend>,
    records: &mut Vec<ValueRecord>,
) -> PyResult<()> {
    for item in parser.finish() {
        match item {
            Ok(value) => records.push(streaming_value_record(py, value)?),
            Err(err) => {
                records.push(values_error_record(err));
                return Ok(());
            }
        }
    }
    Ok(())
}

fn streaming_value_record(
    py: Python<'_>,
    value: CoreStreamingValue<CoreValue>,
) -> PyResult<ValueRecord> {
    let index = value.index.into_pyobject(py)?.into_any().unbind();
    let value_object = value_to_py(py, &value.value)?;
    let is_final = PyBool::new(py, value.is_final)
        .to_owned()
        .into_any()
        .unbind();
    let tuple = PyTuple::new(py, [index, value_object, is_final])?;
    Ok(ValueRecord::Value(tuple.into_any().unbind()))
}

fn values_error_record(err: CoreValuesError<StdBackend>) -> ValueRecord {
    let err = match err {
        CoreValuesError::Parser(err) => OwnedParserError {
            message: err.to_string(),
            line: err.line(),
            column: err.column(),
        },
        CoreValuesError::Assembler(err) => OwnedParserError {
            message: err.to_string(),
            line: 0,
            column: 0,
        },
    };
    ValueRecord::Error(err)
}

fn value_to_py(py: Python<'_>, value: &CoreValue) -> PyResult<PyObject> {
    match value {
        CoreValue::Null => Ok(py.None()),
        CoreValue::Boolean(value) => Ok(PyBool::new(py, *value).to_owned().into_any().unbind()),
        CoreValue::Number(value) => Ok(value.into_pyobject(py)?.into_any().unbind()),
        CoreValue::String(value) => Ok(PyString::new(py, value).into_any().unbind()),
        CoreValue::Array(values) => {
            let list = PyList::empty(py);
            for item in values {
                list.append(value_to_py(py, item)?)?;
            }
            Ok(list.into_any().unbind())
        }
        CoreValue::Object(map) => {
            let dict = PyDict::new(py);
            for (key, item) in map {
                dict.set_item(key.as_ref(), value_to_py(py, item)?)?;
            }
            Ok(dict.into_any().unbind())
        }
    }
}

fn collect_mutable_value_feed(
    py: Python<'_>,
    parser: &mut CoreJsonModem<StdBackend>,
    chunk: &str,
    root: &mut Option<PyObject>,
    next_index: &mut usize,
    records: &mut Vec<ValueRecord>,
) -> PyResult<()> {
    let mut events = parser.feed(chunk);
    while let Some(item) = CoreLendingIterator::next(&mut events) {
        match item {
            Ok(event) => {
                if let Some(record) = mutable_event_record(py, event, root, next_index)? {
                    records.push(record);
                }
            }
            Err(err) => {
                records.push(ValueRecord::Error(OwnedParserError {
                    message: err.to_string(),
                    line: err.line(),
                    column: err.column(),
                }));
                return Ok(());
            }
        }
    }
    Ok(())
}

fn collect_mutable_value_finish(
    py: Python<'_>,
    parser: CoreJsonModem<StdBackend>,
    root: &mut Option<PyObject>,
    next_index: &mut usize,
    records: &mut Vec<ValueRecord>,
) -> PyResult<()> {
    let mut events = parser.finish();
    while let Some(item) = CoreLendingIterator::next(&mut events) {
        match item {
            Ok(event) => {
                if let Some(record) = mutable_event_record(py, event, root, next_index)? {
                    records.push(record);
                }
            }
            Err(err) => {
                records.push(ValueRecord::Error(OwnedParserError {
                    message: err.to_string(),
                    line: err.line(),
                    column: err.column(),
                }));
                return Ok(());
            }
        }
    }
    Ok(())
}

fn collect_view_value_feed(
    py: Python<'_>,
    parser: &mut CoreJsonModem<StdBackend>,
    chunk: &str,
    root: &Rc<RefCell<Option<CoreValue>>>,
    next_index: &mut usize,
    records: &mut Vec<ValueRecord>,
) -> PyResult<()> {
    let mut events = parser.feed(chunk);
    while let Some(item) = CoreLendingIterator::next(&mut events) {
        match item {
            Ok(event) => {
                if let Some(record) = view_value_event_record(py, event, root, next_index)? {
                    records.push(record);
                }
            }
            Err(err) => {
                records.push(ValueRecord::Error(OwnedParserError {
                    message: err.to_string(),
                    line: err.line(),
                    column: err.column(),
                }));
                return Ok(());
            }
        }
    }
    Ok(())
}

fn collect_view_value_finish(
    py: Python<'_>,
    parser: CoreJsonModem<StdBackend>,
    root: &Rc<RefCell<Option<CoreValue>>>,
    next_index: &mut usize,
    records: &mut Vec<ValueRecord>,
) -> PyResult<()> {
    let mut events = parser.finish();
    while let Some(item) = CoreLendingIterator::next(&mut events) {
        match item {
            Ok(event) => {
                if let Some(record) = view_value_event_record(py, event, root, next_index)? {
                    records.push(record);
                }
            }
            Err(err) => {
                records.push(ValueRecord::Error(OwnedParserError {
                    message: err.to_string(),
                    line: err.line(),
                    column: err.column(),
                }));
                return Ok(());
            }
        }
    }
    Ok(())
}

fn collect_view_value_feed_with(
    py: Python<'_>,
    parser: &mut CoreJsonModem<StdBackend>,
    chunk: &str,
    root: &Rc<RefCell<Option<CoreValue>>>,
    next_index: &mut usize,
    records: &mut Vec<ValueRecord>,
    mut build_record: impl FnMut(
        Python<'_>,
        usize,
        Vec<OwnedPathComponent>,
        bool,
    ) -> PyResult<ValueRecord>,
) -> PyResult<()> {
    let mut events = parser.feed(chunk);
    while let Some(item) = CoreLendingIterator::next(&mut events) {
        match item {
            Ok(event) => {
                if let Some((path, is_final)) = core_apply_event(event, &mut root.borrow_mut()) {
                    records.push(build_record(py, *next_index, path, is_final)?);
                    if is_final {
                        *next_index += 1;
                    }
                }
            }
            Err(err) => {
                records.push(ValueRecord::Error(OwnedParserError {
                    message: err.to_string(),
                    line: err.line(),
                    column: err.column(),
                }));
                return Ok(());
            }
        }
    }
    Ok(())
}

fn collect_view_value_finish_with(
    py: Python<'_>,
    parser: CoreJsonModem<StdBackend>,
    root: &Rc<RefCell<Option<CoreValue>>>,
    next_index: &mut usize,
    records: &mut Vec<ValueRecord>,
    mut build_record: impl FnMut(
        Python<'_>,
        usize,
        Vec<OwnedPathComponent>,
        bool,
    ) -> PyResult<ValueRecord>,
) -> PyResult<()> {
    let mut events = parser.finish();
    while let Some(item) = CoreLendingIterator::next(&mut events) {
        match item {
            Ok(event) => {
                if let Some((path, is_final)) = core_apply_event(event, &mut root.borrow_mut()) {
                    records.push(build_record(py, *next_index, path, is_final)?);
                    if is_final {
                        *next_index += 1;
                    }
                }
            }
            Err(err) => {
                records.push(ValueRecord::Error(OwnedParserError {
                    message: err.to_string(),
                    line: err.line(),
                    column: err.column(),
                }));
                return Ok(());
            }
        }
    }
    Ok(())
}

fn mutable_event_record(
    py: Python<'_>,
    event: ParseEvent<'_, &Path, StdBackend>,
    root: &mut Option<PyObject>,
    next_index: &mut usize,
) -> PyResult<Option<ValueRecord>> {
    let Some((path, is_final)) = mutable_apply_event(py, event, root)? else {
        return Ok(None);
    };
    let Some(root_object) = root.as_ref() else {
        return Ok(None);
    };
    let record = value_update_record(py, *next_index, root_object.clone_ref(py), &path, is_final)?;
    if is_final {
        *next_index += 1;
    }
    Ok(Some(record))
}

fn cached_view_update_record(
    py: Python<'_>,
    index: usize,
    root_view: &Py<PyJsonModemValueView>,
    path: &[OwnedPathComponent],
    is_final: bool,
) -> PyResult<ValueRecord> {
    value_update_record(
        py,
        index,
        root_view.clone_ref(py).into_bound(py).into_any().unbind(),
        path,
        is_final,
    )
}

fn path_only_update_record(
    py: Python<'_>,
    index: usize,
    path: Vec<OwnedPathComponent>,
    is_final: bool,
) -> PyResult<ValueRecord> {
    let index = index.into_pyobject(py)?.into_any().unbind();
    let path = build_path_tuple_for_event(py, &path)?;
    let is_final = PyBool::new(py, is_final).to_owned().into_any().unbind();
    let tuple = PyTuple::new(py, [index, path, is_final])?;
    Ok(ValueRecord::Value(tuple.into_any().unbind()))
}

fn cached_view_path_view_update_record(
    py: Python<'_>,
    index: usize,
    root_view: &Py<PyJsonModemValueView>,
    path: Vec<OwnedPathComponent>,
    is_final: bool,
) -> PyResult<ValueRecord> {
    let index = index.into_pyobject(py)?.into_any().unbind();
    let root_view = root_view.clone_ref(py).into_bound(py).into_any().unbind();
    let path = Py::new(py, PyPathView { path })?
        .into_bound(py)
        .into_any()
        .unbind();
    let is_final = PyBool::new(py, is_final).to_owned().into_any().unbind();
    let tuple = PyTuple::new(py, [index, root_view, path, is_final])?;
    Ok(ValueRecord::Value(tuple.into_any().unbind()))
}

fn view_value_event_record(
    py: Python<'_>,
    event: ParseEvent<'_, &Path, StdBackend>,
    root: &Rc<RefCell<Option<CoreValue>>>,
    next_index: &mut usize,
) -> PyResult<Option<ValueRecord>> {
    let Some((path, is_final)) = core_apply_event(event, &mut root.borrow_mut()) else {
        return Ok(None);
    };
    let view = Py::new(
        py,
        PyJsonModemValueView {
            root: Rc::clone(root),
            path: Vec::new(),
        },
    )?
    .into_bound(py)
    .into_any()
    .unbind();
    let record = value_update_record(py, *next_index, view, &path, is_final)?;
    if is_final {
        *next_index += 1;
    }
    Ok(Some(record))
}

fn value_update_record(
    py: Python<'_>,
    index: usize,
    root_or_view: PyObject,
    path: &[OwnedPathComponent],
    is_final: bool,
) -> PyResult<ValueRecord> {
    let index = index.into_pyobject(py)?.into_any().unbind();
    let path = build_path_tuple_for_event(py, path)?;
    let is_final = PyBool::new(py, is_final).to_owned().into_any().unbind();
    let tuple = PyTuple::new(py, [index, root_or_view, path, is_final])?;
    Ok(ValueRecord::Value(tuple.into_any().unbind()))
}

fn mutable_apply_event(
    py: Python<'_>,
    event: ParseEvent<'_, &Path, StdBackend>,
    root: &mut Option<PyObject>,
) -> PyResult<Option<(Vec<OwnedPathComponent>, bool)>> {
    match event {
        ParseEvent::Null { path } => {
            let path = convert_borrowed_path(path);
            py_assign_at_path(py, root, &path, py.None())?;
            let is_final = path.is_empty();
            Ok(Some((path, is_final)))
        }
        ParseEvent::Boolean { path, value } => {
            let path = convert_borrowed_path(path);
            py_assign_at_path(
                py,
                root,
                &path,
                PyBool::new(py, value).to_owned().into_any().unbind(),
            )?;
            let is_final = path.is_empty();
            Ok(Some((path, is_final)))
        }
        ParseEvent::Number { path, value } => {
            let path = convert_borrowed_path(path);
            py_assign_at_path(
                py,
                root,
                &path,
                value.into_pyobject(py)?.into_any().unbind(),
            )?;
            let is_final = path.is_empty();
            Ok(Some((path, is_final)))
        }
        ParseEvent::String {
            path,
            fragment,
            is_initial,
            is_final,
        } => {
            let path = convert_borrowed_path(path);
            let is_final_root = is_final && path.is_empty();
            if is_initial {
                py_assign_at_path(
                    py,
                    root,
                    &path,
                    PyString::new(py, fragment.as_ref()).into_any().unbind(),
                )?;
            } else {
                let mut text = py_value_at_path(py, root, &path)?
                    .and_then(|value| value.extract::<String>(py).ok())
                    .unwrap_or_default();
                text.push_str(fragment.as_ref());
                py_assign_at_path(
                    py,
                    root,
                    &path,
                    PyString::new(py, &text).into_any().unbind(),
                )?;
            }
            Ok(Some((path, is_final_root)))
        }
        ParseEvent::ArrayBegin { path } => {
            let path = convert_borrowed_path(path);
            py_assign_at_path(py, root, &path, PyList::empty(py).into_any().unbind())?;
            Ok(Some((path, false)))
        }
        ParseEvent::ObjectBegin { path } => {
            let path = convert_borrowed_path(path);
            py_assign_at_path(py, root, &path, PyDict::new(py).into_any().unbind())?;
            Ok(Some((path, false)))
        }
        ParseEvent::ArrayEnd { path, .. } | ParseEvent::ObjectEnd { path, .. } => {
            if path.is_empty() {
                Ok(Some((Vec::new(), true)))
            } else {
                Ok(None)
            }
        }
    }
}

fn py_value_at_path(
    py: Python<'_>,
    root: &Option<PyObject>,
    path: &[OwnedPathComponent],
) -> PyResult<Option<PyObject>> {
    let Some(root) = root else {
        return Ok(None);
    };
    if path.is_empty() {
        return Ok(Some(root.clone_ref(py)));
    }

    let mut current = root.clone_ref(py);
    for component in path {
        let current_bound = current.bind(py);
        current = match component {
            OwnedPathComponent::Key(key) => {
                let dict = current_bound.downcast::<PyDict>()?;
                let Some(value) = dict.get_item(key)? else {
                    return Ok(None);
                };
                value.into_any().unbind()
            }
            OwnedPathComponent::Index(index) => {
                let list = current_bound.downcast::<PyList>()?;
                if *index >= list.len() {
                    return Ok(None);
                }
                list.get_item(*index)?.into_any().unbind()
            }
        };
    }
    Ok(Some(current))
}

fn py_assign_at_path(
    py: Python<'_>,
    root: &mut Option<PyObject>,
    path: &[OwnedPathComponent],
    value: PyObject,
) -> PyResult<()> {
    if path.is_empty() {
        *root = Some(value);
        return Ok(());
    }

    let root_object = root.get_or_insert_with(|| match path.first() {
        Some(OwnedPathComponent::Index(_)) => PyList::empty(py).into_any().unbind(),
        _ => PyDict::new(py).into_any().unbind(),
    });

    let parent_path = &path[..path.len() - 1];
    let mut current = root_object.clone_ref(py);
    for (position, component) in parent_path.iter().enumerate() {
        let next_component = path.get(position + 1);
        let current_bound = current.bind(py);
        current = match component {
            OwnedPathComponent::Key(key) => {
                let dict = current_bound.downcast::<PyDict>()?;
                if let Some(value) = dict.get_item(key)? {
                    value.into_any().unbind()
                } else {
                    let container = py_container_for_next(py, next_component);
                    dict.set_item(key, container.clone_ref(py))?;
                    container
                }
            }
            OwnedPathComponent::Index(index) => {
                let list = current_bound.downcast::<PyList>()?;
                while list.len() <= *index {
                    list.append(py.None())?;
                }
                let value = list.get_item(*index)?;
                if value.is_none() {
                    let container = py_container_for_next(py, next_component);
                    list.set_item(*index, container.clone_ref(py))?;
                    container
                } else {
                    value.into_any().unbind()
                }
            }
        };
    }

    let Some(last) = path.last() else {
        return Ok(());
    };
    let current_bound = current.bind(py);
    match last {
        OwnedPathComponent::Key(key) => {
            current_bound.downcast::<PyDict>()?.set_item(key, value)?;
        }
        OwnedPathComponent::Index(index) => {
            let list = current_bound.downcast::<PyList>()?;
            while list.len() < *index {
                list.append(py.None())?;
            }
            if list.len() == *index {
                list.append(value)?;
            } else {
                list.set_item(*index, value)?;
            }
        }
    }
    Ok(())
}

fn py_container_for_next(py: Python<'_>, next: Option<&OwnedPathComponent>) -> PyObject {
    match next {
        Some(OwnedPathComponent::Index(_)) => PyList::empty(py).into_any().unbind(),
        _ => PyDict::new(py).into_any().unbind(),
    }
}

fn core_apply_event(
    event: ParseEvent<'_, &Path, StdBackend>,
    root: &mut Option<CoreValue>,
) -> Option<(Vec<OwnedPathComponent>, bool)> {
    match event {
        ParseEvent::Null { path } => {
            let path = convert_borrowed_path(path);
            core_assign_at_path(root, &path, CoreValue::Null);
            let is_final = path.is_empty();
            Some((path, is_final))
        }
        ParseEvent::Boolean { path, value } => {
            let path = convert_borrowed_path(path);
            core_assign_at_path(root, &path, CoreValue::Boolean(value));
            let is_final = path.is_empty();
            Some((path, is_final))
        }
        ParseEvent::Number { path, value } => {
            let path = convert_borrowed_path(path);
            core_assign_at_path(root, &path, CoreValue::Number(value));
            let is_final = path.is_empty();
            Some((path, is_final))
        }
        ParseEvent::String {
            path,
            fragment,
            is_initial,
            is_final,
        } => {
            let path = convert_borrowed_path(path);
            let is_final_root = is_final && path.is_empty();
            if is_initial {
                core_assign_at_path(
                    root,
                    &path,
                    CoreValue::String(fragment.as_ref().to_string()),
                );
            } else if let Some(CoreValue::String(text)) = core_value_at_path_mut(root, &path) {
                text.push_str(fragment.as_ref());
            } else {
                core_assign_at_path(
                    root,
                    &path,
                    CoreValue::String(fragment.as_ref().to_string()),
                );
            }
            Some((path, is_final_root))
        }
        ParseEvent::ArrayBegin { path } => {
            let path = convert_borrowed_path(path);
            core_assign_at_path(root, &path, CoreValue::Array(Vec::new()));
            Some((path, false))
        }
        ParseEvent::ObjectBegin { path } => {
            let path = convert_borrowed_path(path);
            core_assign_at_path(root, &path, CoreValue::Object(BTreeMap::new()));
            Some((path, false))
        }
        ParseEvent::ArrayEnd { path, .. } | ParseEvent::ObjectEnd { path, .. } => {
            if path.is_empty() {
                Some((Vec::new(), true))
            } else {
                None
            }
        }
    }
}

fn core_value_at_path<'a>(
    root: &'a Option<CoreValue>,
    path: &[OwnedPathComponent],
) -> Option<&'a CoreValue> {
    let mut current = root.as_ref()?;
    for component in path {
        current = match (component, current) {
            (OwnedPathComponent::Key(key), CoreValue::Object(map)) => map.get(key.as_str())?,
            (OwnedPathComponent::Index(index), CoreValue::Array(values)) => values.get(*index)?,
            _ => return None,
        };
    }
    Some(current)
}

fn core_value_at_path_mut<'a>(
    root: &'a mut Option<CoreValue>,
    path: &[OwnedPathComponent],
) -> Option<&'a mut CoreValue> {
    let mut current = root.as_mut()?;
    for component in path {
        current = match component {
            OwnedPathComponent::Key(key) => match current {
                CoreValue::Object(map) => map.get_mut(key.as_str())?,
                _ => return None,
            },
            OwnedPathComponent::Index(index) => match current {
                CoreValue::Array(values) => values.get_mut(*index)?,
                _ => return None,
            },
        };
    }
    Some(current)
}

fn core_assign_at_path(
    root: &mut Option<CoreValue>,
    path: &[OwnedPathComponent],
    value: CoreValue,
) {
    if path.is_empty() {
        *root = Some(value);
        return;
    }

    let root_value = root.get_or_insert_with(|| core_container_for_next(path.first()));
    core_assign_inside(root_value, path, value);
}

fn core_assign_inside(current: &mut CoreValue, path: &[OwnedPathComponent], value: CoreValue) {
    if path.len() == 1 {
        match &path[0] {
            OwnedPathComponent::Key(key) => {
                let map = ensure_core_object(current);
                map.insert(key.as_str().into(), value);
            }
            OwnedPathComponent::Index(index) => {
                let values = ensure_core_array(current);
                while values.len() < *index {
                    values.push(CoreValue::Null);
                }
                if values.len() == *index {
                    values.push(value);
                } else {
                    values[*index] = value;
                }
            }
        }
        return;
    }

    let next = path.get(1);
    match &path[0] {
        OwnedPathComponent::Key(key) => {
            let map = ensure_core_object(current);
            let child = map
                .entry(key.as_str().into())
                .or_insert_with(|| core_container_for_next(next));
            core_assign_inside(child, &path[1..], value);
        }
        OwnedPathComponent::Index(index) => {
            let values = ensure_core_array(current);
            while values.len() <= *index {
                values.push(core_container_for_next(next));
            }
            core_assign_inside(&mut values[*index], &path[1..], value);
        }
    }
}

fn core_container_for_next(next: Option<&OwnedPathComponent>) -> CoreValue {
    match next {
        Some(OwnedPathComponent::Index(_)) => CoreValue::Array(Vec::new()),
        _ => CoreValue::Object(BTreeMap::new()),
    }
}

fn ensure_core_object(current: &mut CoreValue) -> &mut BTreeMap<std::sync::Arc<str>, CoreValue> {
    if !matches!(current, CoreValue::Object(_)) {
        *current = CoreValue::Object(BTreeMap::new());
    }
    match current {
        CoreValue::Object(map) => map,
        _ => unreachable!(),
    }
}

fn ensure_core_array(current: &mut CoreValue) -> &mut Vec<CoreValue> {
    if !matches!(current, CoreValue::Array(_)) {
        *current = CoreValue::Array(Vec::new());
    }
    match current {
        CoreValue::Array(values) => values,
        _ => unreachable!(),
    }
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

fn drain_filtered_view_pending_events(
    py: Python<'_>,
    parser: &mut CoreJsonModem<StdBackend>,
    patterns: &[PathPattern],
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
                    Ok(event) => {
                        if path_matches_patterns(event.path(), patterns) {
                            records.push(view_event_record(py, event, interns)?);
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

fn is_single_byte_view_input(data: &Bound<'_, PyAny>) -> bool {
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
    if guard.view.itemsize != 1 {
        return Err(PyTypeError::new_err(format!(
            "{caller} requires a bytes-like input with itemsize 1 for no-copy payload views"
        )));
    }
    if let Ok(memoryview) = data.downcast::<PyMemoryView>() {
        let owner = memoryview.getattr("obj")?;
        if owner.downcast::<PyBytes>().is_err() {
            return Err(PyTypeError::new_err(format!(
                "{caller} requires memoryview input backed by bytes for stable no-copy payload views"
            )));
        }
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
    m.add_class::<PyValueIter>()?;
    m.add_class::<PyJsonModemValueView>()?;
    m.add_class::<PyJsonModemValueViewsPathView>()?;
    m.add_class::<PyByteEventIter>()?;
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
