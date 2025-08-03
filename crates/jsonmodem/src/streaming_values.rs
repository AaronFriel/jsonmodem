#![doc(hidden)]

use alloc::vec::Vec;

use crate::{
    JsonFactory, ParseEvent, Value,
    options::{NonScalarValueMode, ParserOptions},
    parser::{ParserError, StreamingParserImpl},
};

/// A value produced during streaming parsing.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq)]
pub struct StreamingValue<V: JsonFactory> {
    pub index: usize,
    pub value: V,
    pub is_final: bool,
}

pub type StreamingValuesParser = StreamingValuesParserImpl<Value>;

/// Parser wrapper that returns complete values after each chunk.
#[doc(hidden)]
#[derive(Debug)]
pub struct StreamingValuesParserImpl<V: JsonFactory> {
    parser: StreamingParserImpl<V>,
    next_index: usize,
}

impl<V: JsonFactory> StreamingValuesParserImpl<V> {
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
            parser: StreamingParserImpl::new(options),
            next_index: 0,
        }
    }

    /// Feed a chunk of input and collect streaming values.
    #[inline]
    pub fn feed(&mut self, chunk: &str) -> Result<Vec<StreamingValue<V>>, ParserError> {
        self.parser.feed(chunk);
        self.collect_from_parser()
    }

    /// Signal end of input and collect remaining values.
    #[inline]
    pub fn finish(self) -> Result<Vec<StreamingValue<V>>, ParserError> {
        let mut closed = self.parser.finish();
        let mut out = Vec::<StreamingValue<V>>::new();
        let mut index = self.next_index;
        for evt in closed.by_ref() {
            let ev = evt?;
            match ev {
                ParseEvent::Null { .. } => {
                    out.push(StreamingValue {
                        index,
                        value: V::from_null(V::new_null()),
                        is_final: true,
                    });
                    index += 1;
                }
                ParseEvent::Boolean { value, .. } => {
                    out.push(StreamingValue {
                        index,
                        value: V::from_bool(value),
                        is_final: true,
                    });
                    index += 1;
                }
                ParseEvent::Number { value, .. } => {
                    out.push(StreamingValue {
                        index,
                        value: V::from_num(value),
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
                        value: V::from_str(v),
                        is_final,
                    });
                    if is_final {
                        index += 1;
                    }
                }
                ParseEvent::ArrayEnd { value: Some(v), .. } => {
                    out.push(StreamingValue {
                        index,
                        value: V::from_array(v),
                        is_final: true,
                    });
                    index += 1;
                }
                ParseEvent::ObjectEnd { value: Some(v), .. } => {
                    out.push(StreamingValue {
                        index,
                        value: V::from_object(v),
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
    fn collect_from_parser(&mut self) -> Result<Vec<StreamingValue<V>>, ParserError> {
        let mut out = Vec::<StreamingValue<V>>::new();
        for evt in self.parser.by_ref() {
            let ev = evt?;
            match ev {
                ParseEvent::Null { .. } => {
                    out.push(StreamingValue {
                        index: self.next_index,
                        value: V::from_null(V::new_null()),
                        is_final: true,
                    });
                    self.next_index += 1;
                }
                ParseEvent::Boolean { value, .. } => {
                    out.push(StreamingValue {
                        index: self.next_index,
                        value: V::from_bool(value),
                        is_final: true,
                    });
                    self.next_index += 1;
                }
                ParseEvent::Number { value, .. } => {
                    out.push(StreamingValue {
                        index: self.next_index,
                        value: V::from_num(value),
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
                        value: V::from_str(v),
                        is_final,
                    });
                    if is_final {
                        self.next_index += 1;
                    }
                }
                ParseEvent::ArrayEnd { value: Some(v), .. } => {
                    out.push(StreamingValue {
                        index: self.next_index,
                        value: V::from_array(v),
                        is_final: true,
                    });
                    self.next_index += 1;
                }
                ParseEvent::ObjectEnd { value: Some(v), .. } => {
                    out.push(StreamingValue {
                        index: self.next_index,
                        value: V::from_object(v),
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
