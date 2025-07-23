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
        for evt in closed.by_ref() {
            let ev = evt?;
            match ev {
                ParseEvent::Null { .. } => {
                    out.push(StreamingValue {
                        index,
                        value: Value::Null,
                        is_final: true,
                    });
                    index += 1;
                }
                ParseEvent::Boolean { value, .. } => {
                    out.push(StreamingValue {
                        index,
                        value: Value::Boolean(value),
                        is_final: true,
                    });
                    index += 1;
                }
                ParseEvent::Number { value, .. } => {
                    out.push(StreamingValue {
                        index,
                        value: Value::Number(value),
                        is_final: true,
                    });
                    index += 1;
                }
                ParseEvent::String {
                    value: Some(v),
                    is_final,
                    ..
                } => {
                    out.push(StreamingValue {
                        index,
                        value: Value::String(v),
                        is_final,
                    });
                    if is_final {
                        index += 1;
                    }
                }
                ParseEvent::ArrayEnd { value: Some(v), .. } => {
                    out.push(StreamingValue {
                        index,
                        value: Value::Array(v),
                        is_final: true,
                    });
                    index += 1;
                }
                ParseEvent::ObjectEnd { value: Some(v), .. } => {
                    out.push(StreamingValue {
                        index,
                        value: Value::Object(v),
                        is_final: true,
                    });
                    index += 1;
                }
                _ => {}
            }
        }
        if let Some(val) = closed.unstable_get_current_value_ref() {
            out.push(StreamingValue {
                index,
                value: val.clone(),
                is_final: false,
            });
        }
        Ok(out)
    }

    #[inline]
    fn collect_from_parser(&mut self) -> Result<Vec<StreamingValue>, ParserError> {
        let mut out = Vec::new();
        for evt in self.parser.by_ref() {
            let ev = evt?;
            match ev {
                ParseEvent::Null { .. } => {
                    out.push(StreamingValue {
                        index: self.next_index,
                        value: Value::Null,
                        is_final: true,
                    });
                    self.next_index += 1;
                }
                ParseEvent::Boolean { value, .. } => {
                    out.push(StreamingValue {
                        index: self.next_index,
                        value: Value::Boolean(value),
                        is_final: true,
                    });
                    self.next_index += 1;
                }
                ParseEvent::Number { value, .. } => {
                    out.push(StreamingValue {
                        index: self.next_index,
                        value: Value::Number(value),
                        is_final: true,
                    });
                    self.next_index += 1;
                }
                ParseEvent::String {
                    value: Some(v),
                    is_final,
                    ..
                } => {
                    out.push(StreamingValue {
                        index: self.next_index,
                        value: Value::String(v),
                        is_final,
                    });
                    if is_final {
                        self.next_index += 1;
                    }
                }
                ParseEvent::ArrayEnd { value: Some(v), .. } => {
                    out.push(StreamingValue {
                        index: self.next_index,
                        value: Value::Array(v),
                        is_final: true,
                    });
                    self.next_index += 1;
                }
                ParseEvent::ObjectEnd { value: Some(v), .. } => {
                    out.push(StreamingValue {
                        index: self.next_index,
                        value: Value::Object(v),
                        is_final: true,
                    });
                    self.next_index += 1;
                }
                _ => {}
            }
        }
        if let Some(val) = self.parser.unstable_get_current_value_ref() {
            out.push(StreamingValue {
                index: self.next_index,
                value: val.clone(),
                is_final: false,
            });
        }
        Ok(out)
    }
}
