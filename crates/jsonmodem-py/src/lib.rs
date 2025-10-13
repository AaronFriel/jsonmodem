use std::collections::HashSet;

use ::jsonmodem::{
    DecodeMode as CoreDecodeMode, JsonModem as CoreJsonModem, ParseEvent,
    ParserOptions as CoreParserOptions, Path, PathItem, StdBackend,
};
use pyo3::{
    IntoPyObject,
    class::basic::CompareOp,
    create_exception,
    exceptions::{PyException, PyTypeError},
    prelude::*,
    types::{PyAny, PyBool, PyDict, PyString, PyTuple},
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

#[derive(Clone)]
struct OwnedStringFragment {
    fragment: String,
    is_initial: bool,
    is_final: bool,
}

#[derive(Clone)]
enum OwnedPayload {
    None,
    Bool(bool),
    Number(f64),
    String(OwnedStringFragment),
}

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

#[derive(Clone)]
struct OwnedEvent {
    kind: OwnedEventKind,
    path: Vec<OwnedPathComponent>,
    payload: OwnedPayload,
}

struct OwnedParserError {
    message: String,
    line: usize,
    column: usize,
}

enum EventRecord {
    Event(OwnedEvent),
    Error(OwnedParserError),
}

impl OwnedEvent {
    fn from_parse_event(
        event: ParseEvent,
        string_tracker: &mut HashSet<Vec<OwnedPathComponent>>,
    ) -> Self {
        match event {
            ParseEvent::Null { path } => Self {
                kind: OwnedEventKind::Null,
                path: convert_path(path),
                payload: OwnedPayload::None,
            },
            ParseEvent::Boolean { path, value } => Self {
                kind: OwnedEventKind::Bool,
                path: convert_path(path),
                payload: OwnedPayload::Bool(value),
            },
            ParseEvent::Number { path, value } => Self {
                kind: OwnedEventKind::Number,
                path: convert_path(path),
                payload: OwnedPayload::Number(value),
            },
            ParseEvent::String {
                path,
                fragment,
                is_final,
                ..
            } => {
                let path = convert_path(path);
                let is_initial = string_tracker.insert(path.clone());
                if is_final {
                    string_tracker.remove(&path);
                }

                Self {
                    kind: OwnedEventKind::String,
                    path,
                    payload: OwnedPayload::String(OwnedStringFragment {
                        fragment: fragment.into_owned(),
                        is_initial,
                        is_final,
                    }),
                }
            }
            ParseEvent::ArrayBegin { path } => Self {
                kind: OwnedEventKind::ArrayBegin,
                path: convert_path(path),
                payload: OwnedPayload::None,
            },
            ParseEvent::ArrayEnd { path, .. } => Self {
                kind: OwnedEventKind::ArrayEnd,
                path: convert_path(path),
                payload: OwnedPayload::None,
            },
            ParseEvent::ObjectBegin { path } => Self {
                kind: OwnedEventKind::ObjectBegin,
                path: convert_path(path),
                payload: OwnedPayload::None,
            },
            ParseEvent::ObjectEnd { path, .. } => Self {
                kind: OwnedEventKind::ObjectEnd,
                path: convert_path(path),
                payload: OwnedPayload::None,
            },
        }
    }

    fn to_raw_event(&self, py: Python<'_>, interns: &InternedStrings) -> PyResult<PyObject> {
        let kind = interns.kind_bound(py, self.kind).into_any().unbind();
        let path = build_path_tuple(py, &self.path, interns)?
            .into_any()
            .unbind();
        let payload = build_payload(py, &self.payload)?.into_any().unbind();
        let tuple = PyTuple::new(py, [kind, path, payload])?;
        Ok(tuple.into_any().unbind())
    }
}

fn convert_path(path: Path) -> Vec<OwnedPathComponent> {
    path.into_iter()
        .map(|component| match component {
            PathItem::Key(key) => OwnedPathComponent::Key(key.to_string()),
            PathItem::Index(index) => OwnedPathComponent::Index(index),
        })
        .collect()
}

fn build_payload<'py>(py: Python<'py>, payload: &OwnedPayload) -> PyResult<Bound<'py, PyAny>> {
    match payload {
        OwnedPayload::None => Ok(py.None().into_bound(py)),
        OwnedPayload::Bool(value) => Ok(PyBool::new(py, *value).to_owned().into_any()),
        OwnedPayload::Number(value) => Ok(value.into_pyobject(py)?.into_any()),
        OwnedPayload::String(fragment) => {
            let dict = PyDict::new(py);
            dict.set_item("fragment", fragment.fragment.as_str())?;
            dict.set_item("is_initial", fragment.is_initial)?;
            dict.set_item("is_final", fragment.is_final)?;
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

    let mut entries = Vec::with_capacity(path.len());
    for component in path {
        let pair = match component {
            OwnedPathComponent::Key(key) => PyTuple::new(
                py,
                [
                    interns.key_tag(py).into_any().unbind(),
                    PyString::new(py, key).into_any().unbind(),
                ],
            )?,
            OwnedPathComponent::Index(index) => PyTuple::new(
                py,
                [
                    interns.index_tag(py).into_any().unbind(),
                    index.into_pyobject(py)?.into_any().unbind(),
                ],
            )?,
        };
        entries.push(pair.into_any().unbind());
    }

    PyTuple::new(py, entries)
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

struct InternedStrings {
    kinds: KindInterns,
    path: PathInterns,
}

impl InternedStrings {
    fn new(py: Python<'_>) -> PyResult<Self> {
        Ok(Self {
            kinds: KindInterns::new(py)?,
            path: PathInterns::new(py)?,
        })
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

/// Streaming JSON parser that yields `(kind, path, payload)` tuples.
///
/// The parser keeps internal state so callers can feed arbitrarily sliced JSON
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
    active_strings: HashSet<Vec<OwnedPathComponent>>,
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
    fn new(_py: Python<'_>, options: Option<Bound<'_, PyAny>>) -> PyResult<Self> {
        let parsed_options = match options {
            Some(item) => read_parser_options(item)?,
            None => PyParserOptions::default(),
        };

        Ok(Self {
            parser: Some(CoreJsonModem::new(parsed_options.to_core())),
            finished: false,
            active_strings: HashSet::new(),
        })
    }

    /// Feed UTF-8 JSON to the parser and get an iterator over new events.
    ///
    /// The iterator owns each event tuple, so the caller can freely retain the
    /// results even after the next `feed()` call.  Errors are reported lazily:
    /// a `JsonModemSyntaxError` is raised from the iterator at the first
    /// invalid token.
    #[pyo3(text_signature = "($self, chunk)")]
    fn feed(&mut self, py: Python<'_>, chunk: &str) -> PyResult<Py<PyEventIter>> {
        let parser = self
            .parser
            .as_mut()
            .ok_or_else(|| state_error("parser has already finished"))?;

        let active_strings = &mut self.active_strings;
        let records = collect_feed_events(parser, chunk, active_strings);
        PyEventIter::new(py, records)
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
        let active_strings = &mut self.active_strings;
        let records = collect_finish_events(parser, active_strings);
        self.finished = true;
        self.active_strings.clear();
        PyEventIter::new(py, records)
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
    interns: InternedStrings,
}

impl PyEventIter {
    fn new(py: Python<'_>, records: Vec<EventRecord>) -> PyResult<Py<PyEventIter>> {
        Py::new(
            py,
            PyEventIter {
                records,
                index: 0,
                interns: InternedStrings::new(py)?,
            },
        )
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

        let entry = &self.records[self.index];
        self.index += 1;

        match entry {
            EventRecord::Event(event) => Ok(Some(event.to_raw_event(py, &self.interns)?)),
            EventRecord::Error(err) => Err(parser_error_to_py(py, err)),
        }
    }
}

fn collect_feed_events(
    parser: &mut CoreJsonModem<StdBackend>,
    chunk: &str,
    string_tracker: &mut HashSet<Vec<OwnedPathComponent>>,
) -> Vec<EventRecord> {
    let mut records = Vec::new();
    for item in parser.feed(chunk).to_iter() {
        match item {
            Ok(event) => records.push(event_record(event, string_tracker)),
            Err(err) => {
                records.push(error_record(err.to_string(), err.line(), err.column()));
                return records;
            }
        }
    }
    drain_pending_events(parser, string_tracker, &mut records);
    records
}

fn collect_finish_events(
    parser: CoreJsonModem<StdBackend>,
    string_tracker: &mut HashSet<Vec<OwnedPathComponent>>,
) -> Vec<EventRecord> {
    let mut records = Vec::new();
    for item in parser.finish().to_iter() {
        match item {
            Ok(event) => records.push(event_record(event, string_tracker)),
            Err(err) => {
                records.push(error_record(err.to_string(), err.line(), err.column()));
                break;
            }
        }
    }
    records
}

fn drain_pending_events(
    parser: &mut CoreJsonModem<StdBackend>,
    string_tracker: &mut HashSet<Vec<OwnedPathComponent>>,
    records: &mut Vec<EventRecord>,
) {
    loop {
        let mut produced = false;
        for item in parser.feed("").to_iter() {
            produced = true;
            match item {
                Ok(event) => records.push(event_record(event, string_tracker)),
                Err(err) => {
                    records.push(error_record(err.to_string(), err.line(), err.column()));
                    return;
                }
            }
        }
        if !produced {
            break;
        }
    }
}

fn event_record(
    event: ParseEvent,
    string_tracker: &mut HashSet<Vec<OwnedPathComponent>>,
) -> EventRecord {
    EventRecord::Event(OwnedEvent::from_parse_event(event, string_tracker))
}

fn error_record(message: String, line: usize, column: usize) -> EventRecord {
    EventRecord::Error(OwnedParserError {
        message,
        line,
        column,
    })
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
