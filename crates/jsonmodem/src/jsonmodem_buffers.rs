use alloc::string::String;

use crate::{
    JsonModem,
    buffered::ValueBuilder,
    parser::{ParseEvent, ParserError, ParserOptions},
};

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

#[derive(Debug, Clone, PartialEq)]
pub enum BufferedEvent<B: ValueBuilder> {
    Null {
        path: B::Path,
    },
    Boolean {
        path: B::Path,
        value: B::Bool,
    },
    Number {
        path: B::Path,
        value: B::Num,
    },
    String {
        path: B::Path,
        fragment: B::Str,
        value: Option<B::Str>,
        is_initial: bool,
        is_final: bool,
    },
    ArrayBegin {
        path: B::Path,
    },
    ArrayEnd {
        path: B::Path,
        value: Option<B::Array>,
    },
    ObjectBegin {
        path: B::Path,
    },
    ObjectEnd {
        path: B::Path,
        value: Option<B::Object>,
    },
}

impl<V: ValueBuilder> From<ParseEvent<V>> for BufferedEvent<V> {
    fn from(event: ParseEvent<V>) -> Self {
        match event {
            ParseEvent::Null { path } => BufferedEvent::Null { path },
            ParseEvent::Boolean { path, value } => BufferedEvent::Boolean { path, value },
            ParseEvent::Number { path, value } => BufferedEvent::Number { path, value },
            ParseEvent::String {
                path,
                fragment,
                is_initial,
                is_final,
            } => BufferedEvent::String {
                path,
                fragment,
                value: None,
                is_initial,
                is_final,
            },
            ParseEvent::ArrayBegin { path } => BufferedEvent::ArrayBegin { path },
            ParseEvent::ArrayEnd { path, .. } => BufferedEvent::ArrayEnd { path, value: None },
            ParseEvent::ObjectBegin { path } => BufferedEvent::ObjectBegin { path },
            ParseEvent::ObjectEnd { path, .. } => BufferedEvent::ObjectEnd { path, value: None },
        }
    }
}

#[derive(Debug)]
pub struct JsonModemBuffers<B: ValueBuilder> {
    pub(crate) modem: JsonModem,
    pub(crate) opts: BufferOptions,
    // Pending string state persisted across feeds. Redundant if we have a ValueBuilder.
    pub(crate) scratch: Option<(B::Path, B::Str)>,
    // Value builder state lives on the parent and spans feeds
    pub(crate) assembler: B::Assembler,
}

impl JsonModemBuffers<B: ValueBuilder> {
    #[must_use]
    pub fn new(options: ParserOptions, opts: BufferOptions) -> Self {
        let assembler = match opts.non_scalar_mode {
            NonScalarMode::None => None,
            _ => Some(Default::default()),
        };
        Self {
            modem: JsonModem::new(options),
            opts,
            scratch: None,
            assembler,
        }
    }

    #[must_use]
    pub fn feed<'a>(&'a mut self, chunk: &str) -> JsonModemBuffersIter<'a> {
        self.modem.inner.feed_str(chunk);
        JsonModemBuffersIter { parser: self }
    }

    #[must_use]
    pub fn finish(mut self) -> JsonModemBuffersClosed {
        self.modem.inner.close();
        JsonModemBuffersClosed { parser: self }
    }

    // fn apply(&mut self, event: &ParseEvent<Value>) -> Result<AppliedEventOutput,
    // ZipperError> {     let Some(builder) = self.assembler.as_mut() else {
    //         return Ok(AppliedEventOutput::Nothing);
    //     };

    //     let f = &mut StdValueFactory;

    //     match event {
    //         // scalars
    //         ParseEvent::Null { path } => {
    //             builder.set(path.last(), f.build_from_null(()), f)?;
    //         }
    //         ParseEvent::Boolean { path, value } => {
    //             let v = f.build_from_bool(*value);
    //             builder.set(path.last(), v, f)?;
    //         }
    //         ParseEvent::Number { path, value } => {
    //             let v = f.build_from_num(*value);
    //             builder.set(path.last(), v, f)?;
    //         }
    //         ParseEvent::String { fragment, path, .. } => {
    //             builder.mutate_with(
    //                 f,
    //                 path.last(),
    //                 |fac| {
    //                     let v = fac.new_string("");
    //                     fac.build_from_str(v)
    //                 },
    //                 |v, fac| {
    //                     if let Some(s) = JsonValue::as_string_mut(v) {
    //                         fac.push_string(s, fragment);
    //                         Ok(())
    //                     } else {
    //                         Err(ZipperError::ExpectedString)
    //                     }
    //                 },
    //             )?;

    //             // TODO optimize this, return a AppliedOutputEvent::String
    //             // taking into account the string buffer mode
    //         }

    //         // ── container starts ───────────────────────────────────────
    //         ParseEvent::ObjectBegin { path } => {
    //             builder.enter_with(path.last(), f, |fac| {
    //                 let v = fac.new_object();
    //                 fac.build_from_object(v)
    //             })?;
    //         }
    //         ParseEvent::ArrayBegin { path } => {
    //             builder.enter_with(path.last(), f, |fac| {
    //                 let v = fac.new_array();
    //                 fac.build_from_array(v)
    //             })?;
    //         }

    //         // ── container ends ─────────────────────────────────────────
    //         ParseEvent::ArrayEnd { path, .. } => {
    //             if path.is_empty() {
    //                 // Path is empty, use the root:
    //                 let root = core::mem::take(builder).into_value();
    //                 if let Some(Some(root_array)) = root.map(Value::into_array) {
    //                     if self.opts.non_scalar_mode == NonScalarMode::None {
    //                         return Ok(AppliedEventOutput::Nothing);
    //                     }

    //                     return Ok(AppliedEventOutput::Array(root_array));
    //                 }

    //                 #[cfg(test)]
    //                 panic!("Expected root to be an array");

    //                 #[cfg(not(test))]
    //                 return Err(ZipperError::ExpectedArray);
    //             } else if let Some(leaf_array) =
    // JsonValue::as_array_mut(builder.pop()?) {                 if
    // self.opts.non_scalar_mode != NonScalarMode::All {
    // return Ok(AppliedEventOutput::Nothing);                 }

    //                 return Ok(AppliedEventOutput::Array(leaf_array.clone()));
    //             }

    //             return Err(ZipperError::ExpectedArray);
    //         }
    //         ParseEvent::ObjectEnd { path, .. } => {
    //             if path.is_empty() {
    //                 // Path is empty, use the root:
    //                 let root = core::mem::take(builder).into_value();
    //                 if let Some(Some(root_object)) =
    // root.map(JsonValue::into_object) {                     if
    // self.opts.non_scalar_mode == NonScalarMode::None {
    // return Ok(AppliedEventOutput::Nothing);                     }

    //                     return Ok(AppliedEventOutput::Object(root_object));
    //                 }

    //                 #[cfg(test)]
    //                 panic!("Expected root to be an object");
    //                 #[cfg(not(test))]
    //                 return Err(ZipperError::ExpectedObject);
    //             } else if let Some(leaf_object) =
    // JsonValue::as_object_mut(builder.pop()?) {                 if
    // self.opts.non_scalar_mode != NonScalarMode::All {
    // return Ok(AppliedEventOutput::Nothing);                 }

    //                 return Ok(AppliedEventOutput::Object(leaf_object.clone()));
    //             }
    //             return Err(ZipperError::ExpectedObject);
    //         }
    //     }

    //     Ok(AppliedEventOutput::Nothing)
    // }

    fn consume_event(&mut self, event: ParseEvent<B: Value>) -> Result<BufferedEvent, BufferError> {
        match self.apply(&event)? {
            AppliedEventOutput::Nothing => {}
            AppliedEventOutput::Array(value) => {
                return match event {
                    ParseEvent::ArrayEnd { path, .. } => Ok(BufferedEvent::ArrayEnd {
                        path,
                        value: Some(value),
                    }),
                    _ => Err(ZipperError::ExpectedArray.into()),
                };
            }
            AppliedEventOutput::Object(value) => {
                return match event {
                    ParseEvent::ObjectEnd { path, .. } => Ok(BufferedEvent::ObjectEnd {
                        path,
                        value: Some(value),
                    }),
                    _ => Err(ZipperError::ExpectedObject.into()),
                };
            }
        }

        let mut buffered_event: BufferedEvent = event.into();

        if let BufferedEvent::String {
            path,
            fragment,
            value,
            is_final,
            ..
        } = &mut buffered_event
        {
            let new_value = if let Some((stash_path, stash_buf)) = self.scratch.as_mut()
                && stash_path == path
            {
                stash_buf.push_str(fragment);
                if *is_final {
                    Some(core::mem::take(stash_buf))
                } else if self.opts.string_buffer_mode == StringBufferMode::Prefixes {
                    Some(stash_buf.clone())
                } else {
                    None
                }
            } else {
                if self.opts.string_buffer_mode != StringBufferMode::None {
                    self.scratch = Some((path.clone(), fragment.clone()));
                }
                None
            };

            *value = new_value;
        }

        Ok(buffered_event)
    }
}

pub struct JsonModemBuffersIter<'a> {
    parser: &'a mut JsonModemBuffers,
}

impl Iterator for JsonModemBuffersIter<'_> {
    type Item = Result<BufferedEvent, BufferError>;

    fn next(&mut self) -> Option<Self::Item> {
        match self
            .parser
            .modem
            .inner
            .next_event_with(&mut StdValueFactory)
        {
            Some(Ok(ev)) => Some(self.parser.consume_event(ev)),
            Some(Err(e)) => Some(Err(e.into())),
            None => None,
        }
    }
}

pub struct JsonModemBuffersClosed {
    parser: JsonModemBuffers,
}

impl Iterator for JsonModemBuffersClosed {
    type Item = Result<BufferedEvent, BufferError>;

    fn next(&mut self) -> Option<Self::Item> {
        match self
            .parser
            .modem
            .inner
            .next_event_with(&mut StdValueFactory)
        {
            Some(Ok(ev)) => Some(self.parser.consume_event(ev)),
            Some(Err(e)) => Some(Err(e.into())),
            None => None,
        }
    }
}
