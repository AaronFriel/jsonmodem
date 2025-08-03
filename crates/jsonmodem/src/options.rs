/// Determines the meaning of the `value` field in the `String` token.
///
/// This setting determines the memory and network overhead for parsing string
/// values.
///
/// | mode       | memory overhead         | network overhead         |
/// | -----      | ----------------------- | ------------------------ |
/// | `None`     | None                    | None                     |
/// | `Values`   | O(largest string value) | O(total string length)   |
/// | `Prefixes` | O(largest string value) | O(total string length^2) |
///
/// In `None` mode, partially parsed strings are not buffered and only the
/// incremental fragments are returned.
///
/// In `Values` mode, the full string is returned only when it is fully parsed.
/// This incurs memory proportional to the size of the largest string value, and
/// network overhead proportional to the total size of all string values.
///
/// In `Prefixes` mode, for each fragment parsed, the parser returns the prefix
/// of the string that has been parsed so far. This incurs memory overhead
/// proportional to the size of the largest string value, and network overhead
/// proportional to the square of the total size of all string values - as each
/// prefix may be transmitted many times.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StringValueMode {
    /// The `value` field is always `None`.
    None,
    /// The `value` field contains the full string, and is emitted only when the
    /// string has been fully parsed.
    Values,
    /// The `value` field contains the string prefix that has been parsed thus
    /// far, and is emitted incrementally as the string is parsed.
    Prefixes,
}

impl Default for StringValueMode {
    fn default() -> Self {
        Self::None
    }
}

/// Controls emission of composite values during parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NonScalarValueMode {
    /// Do not emit composite values.
    None,
    /// Emit events for all composite values.
    All,
    /// Emit events only for root values (those with an empty path).
    Roots,
}

impl Default for NonScalarValueMode {
    fn default() -> Self {
        Self::None
    }
}

/// Configuration options for the JSON streaming parser.
///
/// These options control parser behavior such as whitespace handling,
/// multiple value support, and how string or composite values are
/// emitted during parsing.
///
/// # Examples
///
/// ```rust
/// use jsonmodem::{NonScalarValueMode, ParserOptions, StreamingParser, Value};
///
/// let mut options = ParserOptions {
///     allow_multiple_json_values: true,
///     non_scalar_values: NonScalarValueMode::All,
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

    /// Determines how string values are emitted during parsing.
    ///
    /// This option configures the parser's behavior for emitting string tokens,
    /// controlling memory and network overhead. See [`StringValueMode`] for
    /// details:
    ///
    /// - `None`: The `value` field is always `None`. Only incremental fragments
    ///   are returned.
    /// - `Values`: The full string is returned only when fully parsed.
    /// - `Prefixes`: Each fragment returns the prefix parsed so far, emitted
    ///   incrementally.
    ///
    /// See [`StringValueMode`] for a detailed explanation of each mode and
    /// their trade-offs.
    ///
    /// # Default
    ///
    /// `StringValueMode::None`
    pub string_value_mode: StringValueMode,

    /// Whether and how to emit complete composite values (objects and arrays).
    ///
    /// `None` disables emitting composite values. `All` emits events for all
    /// composite values, and `Roots` only emits events for values whose path is
    /// empty (root values). Emitting composite values buffers each full object
    /// or array, increasing memory usage up to the size of the largest
    /// composite value.
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
