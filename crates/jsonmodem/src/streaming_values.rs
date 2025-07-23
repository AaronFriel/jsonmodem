#![doc(hidden)]

use alloc::vec::Vec;

use crate::{
    ParseEvent, StreamingParser, Value,
    options::{NonScalarValueMode, ParserOptions},
    parser::ParserError,
};

/// A value produced during streaming parsing.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq)]
pub struct StreamingValue {
    pub index: usize,
    pub value: Value,
    pub is_final: bool,
}

/// Parser wrapper that returns complete values after each chunk.
#[doc(hidden)]
#[derive(Debug)]
pub struct StreamingValuesParser {
    parser: StreamingParser,
    next_index: usize,
}

impl StreamingValuesParser {
    /// Create a new parser. `non_scalar_values` must not be `None`.
    #[must_use]
    #[inline]
    pub fn new(mut options: ParserOptions) -> Self {
        assert!(
            !matches!(options.non_scalar_values, NonScalarValueMode::None),
            "non scalar values mode must be enabled"
        );
        // use root-only events for efficiency
        options.non_scalar_values = NonScalarValueMode::Roots;
        Self {
            parser: StreamingParser::new(options),
            next_index: 0,
        }
    }

    /// Feed a chunk of input and collect streaming values.
    #[inline]
    pub fn feed(&mut self, chunk: &str) -> Result<Vec<StreamingValue>, ParserError> {
        self.parser.feed(chunk);
        self.collect_from_parser()
    }

    /// Signal end of input and collect remaining values.
    #[inline]
    pub fn finish(self) -> Result<Vec<StreamingValue>, ParserError> {
        let mut closed = self.parser.finish();
        let mut out = Vec::new();
        let mut index = self.next_index;
        while let Some(evt) = closed.next() {
            let ev = evt?;
            let (value, is_final) = match &ev {
                ParseEvent::Null { .. } => (Value::Null, true),
                ParseEvent::Boolean { value, .. } => (Value::Boolean(*value), true),
                ParseEvent::Number { value, .. } => (Value::Number(*value), true),
                ParseEvent::String {
                    value: Some(v),
                    is_final,
                    ..
                } => (Value::String(v.clone()), *is_final),
                ParseEvent::String { value: None, .. }
                | ParseEvent::ArrayStart { .. }
                | ParseEvent::ObjectBegin { .. } => (
                    closed.unstable_get_current_value().unwrap_or(Value::Null),
                    false,
                ),
                ParseEvent::ArrayEnd { value: Some(v), .. } => (Value::Array(v.clone()), true),
                ParseEvent::ObjectEnd { value: Some(v), .. } => (Value::Object(v.clone()), true),
                _ => continue,
            };
            out.push(StreamingValue {
                index,
                value,
                is_final,
            });
            if is_final {
                index += 1;
            }
        }
        Ok(out)
    }

    #[inline]
    fn collect_from_parser(&mut self) -> Result<Vec<StreamingValue>, ParserError> {
        let mut out = Vec::new();
        let mut had_event = false;
        while let Some(evt) = self.parser.next() {
            let ev = evt?;
            had_event = true;
            self.push_from_event(&ev, &mut out);
        }
        if !had_event {
            if let Some(val) = self.parser.unstable_get_current_value() {
                out.push(StreamingValue {
                    index: self.next_index,
                    value: val,
                    is_final: false,
                });
            }
        }
        Ok(out)
    }

    #[inline]
    fn push_from_event(&mut self, event: &ParseEvent, out: &mut Vec<StreamingValue>) {
        let (value, is_final) = match event {
            ParseEvent::Null { .. } => (Value::Null, true),
            ParseEvent::Boolean { value, .. } => (Value::Boolean(*value), true),
            ParseEvent::Number { value, .. } => (Value::Number(*value), true),
            ParseEvent::String {
                value: Some(v),
                is_final,
                ..
            } => (Value::String(v.clone()), *is_final),
            ParseEvent::String { value: None, .. }
            | ParseEvent::ArrayStart { .. }
            | ParseEvent::ObjectBegin { .. } => (
                self.parser
                    .unstable_get_current_value()
                    .unwrap_or(Value::Null),
                false,
            ),
            ParseEvent::ArrayEnd { value: Some(v), .. } => (Value::Array(v.clone()), true),
            ParseEvent::ObjectEnd { value: Some(v), .. } => (Value::Object(v.clone()), true),
            _ => return,
        };
        out.push(StreamingValue {
            index: self.next_index,
            value,
            is_final,
        });
        if is_final {
            self.next_index += 1;
        }
    }
}
