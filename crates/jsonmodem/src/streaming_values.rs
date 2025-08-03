#![doc(hidden)]

use alloc::vec::Vec;

use crate::{
    JsonValue, JsonValueFactory, ParseEvent, StdValueFactory, Value,
    options::{NonScalarValueMode, ParserOptions},
    parser::{ParserError, StreamingParserImpl},
};

/// A value produced during streaming parsing.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq)]
pub struct StreamingValue<V: JsonValue> {
    pub index: usize,
    pub value: V,
    pub is_final: bool,
}

pub type StreamingValuesParser = StreamingValuesParserImpl<Value>;

/// Parser wrapper that returns complete values after each chunk.
#[doc(hidden)]
#[derive(Debug)]
pub struct StreamingValuesParserImpl<V: JsonValue> {
    parser: StreamingParserImpl<V>,
    next_index: usize,
}

impl<V: JsonValue> StreamingValuesParserImpl<V> {
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
    pub fn feed_with<F: JsonValueFactory<Value = V> + Default>(
        &mut self,
        f: F,
        chunk: &str,
    ) -> Result<Vec<StreamingValue<V>>, ParserError> {
        let mut f = f;
        self.parser.feed_with(&mut f, chunk);
        self.collect_from_parser(&mut f)
    }

    /// Signal end of input and collect remaining values.
    #[inline]
    pub fn finish_with<F: JsonValueFactory<Value = V>>(
        self,
        f: F,
    ) -> Result<Vec<StreamingValue<V>>, ParserError> {
        let mut f = f;
        let mut event_iter = self.parser.finish_with(&mut f);
        let mut out = Vec::<StreamingValue<V>>::new();
        let mut index = self.next_index;
        while let Some(evt) = event_iter.next() {
            let ev = evt?;
            match ev {
                ParseEvent::Null { .. } => {
                    let v = event_iter.factory.new_null();
                    out.push(StreamingValue {
                        index,
                        value: event_iter.factory.build_from_null(v),
                        is_final: true,
                    });
                    index += 1;
                }
                ParseEvent::Boolean { value, .. } => {
                    out.push(StreamingValue {
                        index,
                        value: event_iter.factory.build_from_bool(value),
                        is_final: true,
                    });
                    index += 1;
                }
                ParseEvent::Number { value, .. } => {
                    out.push(StreamingValue {
                        index,
                        value: event_iter.factory.build_from_num(value),
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
                        value: event_iter.factory.build_from_str(v),
                        is_final,
                    });
                    if is_final {
                        index += 1;
                    }
                }
                ParseEvent::ArrayEnd { value: Some(v), .. } => {
                    out.push(StreamingValue {
                        index,
                        value: event_iter.factory.build_from_array(v),
                        is_final: true,
                    });
                    index += 1;
                }
                ParseEvent::ObjectEnd { value: Some(v), .. } => {
                    out.push(StreamingValue {
                        index,
                        value: event_iter.factory.build_from_object(v),
                        is_final: true,
                    });
                    index += 1;
                }
                _ => {}
            }
        }
        if let Some(val) = event_iter.unstable_get_current_value_ref() {
            out.push(StreamingValue {
                index,
                value: val.clone(),
                is_final: false,
            });
        }
        Ok(out)
    }

    #[inline]
    fn collect_from_parser<F: JsonValueFactory<Value = V>>(
        &mut self,
        f: &mut F,
    ) -> Result<Vec<StreamingValue<V>>, ParserError> {
        let mut out = Vec::<StreamingValue<V>>::new();
        while let Some(evt) = self.parser.next_event_with(f) {
            let ev = evt?;
            match ev {
                ParseEvent::Null { .. } => {
                    let v = f.new_null();
                    out.push(StreamingValue {
                        index: self.next_index,
                        value: f.build_from_null(v),
                        is_final: true,
                    });
                    self.next_index += 1;
                }
                ParseEvent::Boolean { value, .. } => {
                    out.push(StreamingValue {
                        index: self.next_index,
                        value: f.build_from_bool(value),
                        is_final: true,
                    });
                    self.next_index += 1;
                }
                ParseEvent::Number { value, .. } => {
                    out.push(StreamingValue {
                        index: self.next_index,
                        value: f.build_from_num(value),
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
                        value: f.build_from_str(v),
                        is_final,
                    });
                    if is_final {
                        self.next_index += 1;
                    }
                }
                ParseEvent::ArrayEnd { value: Some(v), .. } => {
                    out.push(StreamingValue {
                        index: self.next_index,
                        value: f.build_from_array(v),
                        is_final: true,
                    });
                    self.next_index += 1;
                }
                ParseEvent::ObjectEnd { value: Some(v), .. } => {
                    out.push(StreamingValue {
                        index: self.next_index,
                        value: f.build_from_object(v),
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

impl StreamingValuesParserImpl<Value> {
    /// Feed a chunk of input and collect streaming values.
    #[inline]
    pub fn feed(&mut self, chunk: &str) -> Result<Vec<StreamingValue<Value>>, ParserError> {
        self.feed_with(StdValueFactory, chunk)
    }

    /// Signal end of input and collect remaining values.
    #[inline]
    pub fn finish(self) -> Result<Vec<StreamingValue<Value>>, ParserError> {
        self.finish_with(StdValueFactory)
    }
}
