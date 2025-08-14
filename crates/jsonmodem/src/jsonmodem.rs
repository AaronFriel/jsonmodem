use crate::{
    ParseEvent, ParserOptions,
    options::NonScalarValueMode,
    parser::{ClosedStreamingParser, StreamingParserImpl},
    value::Value,
};

/// `JsonModem`: minimal, low-overhead streaming parser core.
///
/// This thin wrapper enforces low-overhead options on the underlying parser:
/// - no full-string buffering (fragments only)
/// - `non_scalar_values = None` (no composite value building)
#[derive(Debug)]
pub struct JsonModem {
    inner: StreamingParserImpl<Value>,
}

impl JsonModem {
    /// Create a new `JsonModem` with options; overrides building knobs.
    #[must_use]
    pub fn new(mut options: ParserOptions) -> Self {
        options.non_scalar_values = NonScalarValueMode::None;
        Self {
            inner: StreamingParserImpl::new(options),
        }
    }

    /// Feed a chunk of input and iterate over low-overhead events.
    pub fn feed<'a>(&'a mut self, chunk: &str) -> JsonModemIter<'a> {
        JsonModemIter {
            inner: self.inner.feed(chunk),
        }
    }

    /// Finish the stream and iterate remaining events.
    #[must_use]
    pub fn finish(self) -> JsonModemClosed {
        JsonModemClosed {
            inner: self.inner.finish(),
        }
    }
}

pub struct JsonModemIter<'a> {
    inner: crate::parser::StreamingParserIteratorWith<'a, crate::factory::StdValueFactory>,
}

impl Iterator for JsonModemIter<'_> {
    type Item = Result<ParseEvent<Value>, crate::parser::ParserError>;
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

pub struct JsonModemClosed {
    inner: ClosedStreamingParser<crate::factory::StdValueFactory>,
}

impl Iterator for JsonModemClosed {
    type Item = Result<ParseEvent<Value>, crate::parser::ParserError>;
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}
