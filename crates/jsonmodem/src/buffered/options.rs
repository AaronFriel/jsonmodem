/// Controls buffering behavior for the `JsonModemBuffers` adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BufferOptions {
    pub string_buffer_mode: StringBufferMode,
    pub non_scalar_mode: NonScalarMode,
}

/// Buffering policy for string values in the `JsonModemBuffers` adapter.
///
/// - `None`: never attach a buffered `value` (emit fragments only).
/// - `Values`: attach the full string only when the string ends.
/// - `Prefixes`: attach the growing prefix with every flush.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StringBufferMode {
    None,
    Values,
    Prefixes,
}

impl Default for StringBufferMode {
    fn default() -> Self {
        Self::None
    }
}

/// Controls emission of container events during parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NonScalarMode {
    /// Do not emit container events beyond the default minimal set.
    None,
    /// Emit container events for all objects and arrays.
    All,
    /// Emit container events only for root values (those with an empty path).
    Roots,
}

impl Default for NonScalarMode {
    fn default() -> Self {
        Self::None
    }
}
