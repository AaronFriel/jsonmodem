/// Controls emission of container events during parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NonScalarValueMode {
    /// Do not emit container events beyond the default minimal set.
    None,
    /// Emit container events for all objects and arrays.
    All,
    /// Emit container events only for root values (those with an empty path).
    Roots,
}

impl Default for NonScalarValueMode {
    fn default() -> Self {
        Self::None
    }
}

/// Configuration options for the JSON streaming parser.
///
/// These options control parser behavior such as whitespace handling and
/// multiple value support. Buffering and value building are handled by adapters
/// (`JsonModemBuffers`, `JsonModemValues`) layered on top of the core.
///
/// # Examples
///
/// ```rust
/// use jsonmodem::{DefaultStreamingParser, NonScalarValueMode, ParserOptions, Value};
///
/// let mut options = ParserOptions {
///     allow_multiple_json_values: true,
///     non_scalar_values: NonScalarValueMode::All,
///     ..Default::default()
/// };
/// let mut parser = DefaultStreamingParser::new(options);
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

    /// Whether and how to emit container events (objects and arrays).
    ///
    /// `None` disables additional container events. `All` emits container
    /// events for all objects and arrays, and `Roots` limits container events
    /// to root values (empty path). Building complete composite values is not
    /// performed by the core; use adapters for that functionality.
    ///
    /// # Default
    ///
    /// `NonScalarValueMode::None`
    pub non_scalar_values: NonScalarValueMode,

    #[cfg(any(test, feature = "fuzzing"))]
    /// Panic on syntax errors instead of returning them.
    ///
    /// Enabled only in test builds to produce backtraces on parse failures.
    pub panic_on_error: bool,
}
