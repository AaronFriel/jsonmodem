#![cfg(feature = "todo")]

use crate::parser::{
    ClosedStreamingParser, EventBuilder, ParseEvent, ParserError, ParserOptions,
    StreamingParserImpl,
};

/// `JsonModem`: minimal, low-overhead streaming parser.
#[derive(Debug)]
pub struct JsonModem<B: EventBuilder> {
    pub(crate) inner: StreamingParserImpl<B>,
}

impl<B: EventBuilder> JsonModem<B> {
    /// Create a new `JsonModem` with options; overrides building knobs.
    #[must_use]
    pub fn new_with_factory(b: &mut B, options: ParserOptions) -> Self {
        Self {
            inner: StreamingParserImpl::new_with_factory(b, options),
        }
    }

    /// Feed a chunk of input and iterate over low-overhead events.
    pub fn feed<'a>(&'a mut self, b: &mut B, chunk: &str) -> JsonModemIter<'a, B> {
        todo!()
        // JsonModemIter {
        //     inner: self.inner.feed_with(b, chunk),
        // }
    }

    /// Finish the stream and iterate remaining events.
    #[must_use]
    pub fn finish(self, b: &mut B) -> JsonModemClosed<B> {
        todo!()
        // JsonModemClosed {
        //     inner: self.inner.finish_with(b),
        // }
    }
}

pub struct JsonModemIter<'a, B: EventBuilder> {
    inner: crate::parser::StreamingParserIteratorWith<'a, B>,
}

impl<B: EventBuilder> Iterator for JsonModemIter<'_, B> {
    type Item = Result<ParseEvent<B>, ParserError>;
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

pub struct JsonModemClosed<B: EventBuilder> {
    inner: ClosedStreamingParser<B>,
}

impl<B: EventBuilder> Iterator for JsonModemClosed<B> {
    type Item = Result<ParseEvent<B>, ParserError>;
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}
