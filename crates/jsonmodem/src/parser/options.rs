/// Configuration options for the JSON streaming parser core.
///
/// These options control parser behavior such as whitespace handling and
/// multiple value support. String coalescing and value building are handled by
/// adapters (`JsonModemBuffers`, `JsonModemValues`) layered on top of the core.
///
/// # Default
///
/// All options default to `false`.
#[derive(Debug, Clone, Copy, Default)]
#[non_exhaustive]
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
    pub(crate) allow_unicode_whitespace: bool,

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
    pub(crate) allow_multiple_json_values: bool,

    // No core option to emit composite values; adapters own building.
    /// Panic on syntax errors instead of returning them.
    ///
    /// Enable only in test builds to produce backtraces on parse failures.
    pub(crate) panic_on_error: bool,

    /// Unicode escape decode mode and compatibility knobs.
    ///
    /// Controls how `\uXXXX` sequences and UTF-16 surrogate pairs are
    /// interpreted while decoding JSON strings.
    pub(crate) decode_mode: DecodeMode,
    /// Allow uppercase `\U` introducer for Unicode escapes. JSON grammar uses
    /// lowercase `u`; this is a compatibility knob.
    pub(crate) allow_uppercase_u: bool,
    /// Allow fewer than 4 hex digits after `\u`. JSON requires exactly 4; this
    /// is a compatibility knob. Currently not used by the core parser.
    pub(crate) allow_short_hex: bool,
}

impl ParserOptions {
    /// Creates parser options with every toggle disabled.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Controls whether non-standard whitespace is treated as a delimiter.
    #[must_use]
    pub fn with_allow_unicode_whitespace(mut self, allow: bool) -> Self {
        self.allow_unicode_whitespace = allow;
        self
    }

    /// Enables parsing multiple top-level JSON values from one stream.
    #[must_use]
    pub fn with_allow_multiple_json_values(mut self, allow: bool) -> Self {
        self.allow_multiple_json_values = allow;
        self
    }

    /// Toggles whether syntax errors return results or trigger panics.
    #[must_use]
    pub fn with_panic_on_error(mut self, enabled: bool) -> Self {
        self.panic_on_error = enabled;
        self
    }

    /// Selects how `\uXXXX` escapes and surrogate pairs are decoded.
    #[must_use]
    pub fn with_decode_mode(mut self, mode: DecodeMode) -> Self {
        self.decode_mode = mode;
        self
    }

    /// Allows upper-case `\U` escape introducers as a compatibility mode.
    #[must_use]
    pub fn with_allow_uppercase_u(mut self, allow: bool) -> Self {
        self.allow_uppercase_u = allow;
        self
    }

    /// Permits shorter-than-four-digit Unicode escapes when parsing strings.
    #[must_use]
    pub fn with_allow_short_hex(mut self, allow: bool) -> Self {
        self.allow_short_hex = allow;
        self
    }
}

/// Decode behavior for Unicode escapes and surrogate pairs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DecodeMode {
    /// RFC‑compliant: join valid surrogate pairs; error on any unpaired
    /// surrogate or invalid escape.
    #[default]
    StrictUnicode,
    /// Preserve unpaired surrogates (requires non‑UTF8 output representation).
    /// In UTF‑8 builds this mode degrades to `ReplaceInvalid`.
    SurrogatePreserving,
    /// Join valid pairs; replace any unpaired surrogate with U+FFFD.
    ReplaceInvalid,
}
