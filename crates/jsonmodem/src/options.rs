#![allow(clippy::struct_excessive_bools)]

/// Configuration options for the JSON streaming parser.
///
/// These options control parser behavior such as whitespace handling,
/// multiple value support, and how string or composite values are
/// emitted during parsing.
///
/// # Examples
///
/// ```rust
/// use jsonmodem::{ParserOptions, StreamingParser, Value};
///
/// let mut options = ParserOptions {
///     allow_multiple_json_values: true,
///     emit_non_scalar_values: true,
///     ..Default::default()
/// };
/// let mut parser = StreamingParser::new(options);
/// ```
///
/// # Default
///
/// All options default to `false`.
#[derive(Debug, Clone, Copy, Default)]
pub struct ParserOptions {
    /// Whether to allow any Unicode whitespace between JSON values.
    ///
    /// By default, the parser only recognizes the four whitespace characters
    /// defined by the JSON specification: space (U+0020), line feed (U+000A),
    /// carriage return (U+000D), and horizontal tab (U+0009).
    ///
    /// # Default
    ///
    /// `false`
    pub allow_unicode_whitespace: bool,

    /// Whether to parse multiple JSON values in a single input stream.
    ///
    /// When `true`, the parser does not reset its state at end-of-file, but
    /// continues parsing any additional whitespace-delimited JSON values. This
    /// supports formats such as JSON Lines (JSONL) and newline-delimited JSON
    /// (ND-JSON), and arbitrary concatenation of JSON values.
    ///
    /// # Examples
    ///
    /// ```json
    /// {}{}{}
    /// ```
    ///
    /// ```json
    /// 123 45 678 9
    /// ```
    ///
    /// # Default
    ///
    /// `false`
    pub allow_multiple_json_values: bool,

    /// Whether to emit complete string values as single `ParseEvent::Value`
    /// events.
    ///
    /// When `false`, the parser emits partial string events as it processes
    /// literal content. Enabling this buffers each entire string and emits
    /// it as one event, which may increase memory usage up to the size of the
    /// largest string.
    ///
    /// # Default
    ///
    /// `false`
    pub emit_completed_strings: bool,

    /// Whether to emit complete composite values (objects and arrays) as
    /// `ParseEvent::Value` events.
    ///
    /// When `false`, only scalar values (strings, numbers, booleans, null)
    /// are emitted as complete events. Enabling this buffers each full object
    /// or array, increasing memory usage up to the size of the largest
    /// composite value.
    ///
    /// # Default
    ///
    /// `false`
    pub emit_non_scalar_values: bool,

    /// Whether to emit a `ParseEvent::Complete` after fully parsing one or more
    /// JSON values.
    ///
    /// When enabled, the parser accumulates each JSON root value until it is
    /// complete and then emits `ParseEvent::Complete`. If
    /// `allow_multiple_json_values` is also `true`, multiple complete events
    /// may be produced from the same input stream.
    ///
    /// # Default
    ///
    /// `false`
    pub emit_completed_values: bool,

    #[cfg(any(test, feature = "fuzzing"))]
    /// Panic on syntax errors instead of returning them.
    ///
    /// Enabled only in test builds to produce backtraces on parse failures.
    pub panic_on_error: bool,
}
