use crate::{
    JsonValueFactory, ParseEvent, ParserError, ParserOptions, StdValueFactory, Value,
    jsonmodem::{JsonModem, JsonModemClosed, JsonModemIter},
    value_zipper::ValueBuilder,
};

/// A value produced during streaming parsing by `JsonModemValues`.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamingValue {
    pub index: usize,
    pub value: Value,
    pub is_final: bool,
}

/// Adapter that consumes core events and yields streaming values via an
/// iterator API.
#[derive(Debug)]
pub struct JsonModemValues {
    modem: JsonModem,
    builder: ValueBuilder<Value>,
    index: usize,
    partial: bool,
}

impl JsonModemValues {
    #[must_use]
    pub fn new(options: ParserOptions) -> Self {
        Self {
            modem: JsonModem::new(options),
            builder: ValueBuilder::default(),
            index: 0,
            partial: false,
        }
    }

    /// Create a new `JsonModemValues` with options controlling partial
    /// emission.
    #[must_use]
    pub fn with_options(options: ParserOptions, opts: ValuesOptions) -> Self {
        Self {
            modem: JsonModem::new(options),
            builder: ValueBuilder::default(),
            index: 0,
            partial: opts.partial,
        }
    }

    /// Feed a chunk and iterate over streaming values using a custom factory.
    pub fn feed_with<'a, F: JsonValueFactory<Value = Value> + Default>(
        &'a mut self,
        factory: F,
        chunk: &str,
    ) -> JsonModemValuesIter<'a, F> {
        let builder = core::mem::take(&mut self.builder);
        let index = self.index;
        let parent = self as *mut _;
        let inner = self.modem.feed(chunk);
        JsonModemValuesIter {
            parent,
            inner,
            builder,
            index,
            factory,
            partial: self.partial,
            emitted_partial: false,
            last_emit_final: false,
        }
    }

    /// Feed a chunk and iterate over streaming values with the standard
    /// factory.
    pub fn feed<'a>(&'a mut self, chunk: &str) -> JsonModemValuesIter<'a, StdValueFactory> {
        self.feed_with(StdValueFactory, chunk)
    }

    /// Finish the stream and iterate remaining values with a custom factory.
    pub fn finish_with<F: JsonValueFactory<Value = Value> + Default>(
        self,
        factory: F,
    ) -> JsonModemValuesClosed<F> {
        JsonModemValuesClosed {
            inner: self.modem.finish(),
            builder: self.builder,
            index: self.index,
            factory,
        }
    }

    /// Finish the stream and iterate remaining values with the standard
    /// factory.
    #[must_use]
    pub fn finish(self) -> JsonModemValuesClosed<StdValueFactory> {
        self.finish_with(StdValueFactory)
    }

    #[allow(clippy::too_many_lines)]
    fn apply_event<F: JsonValueFactory<Value = Value>>(
        builder: &mut ValueBuilder<Value>,
        index: &mut usize,
        f: &mut F,
        ev: ParseEvent<Value>,
        partial: bool,
    ) -> Option<StreamingValue> {
        let mut emit = None;
        match ev {
            ParseEvent::Null { path } => {
                let v = f.build_from_null(());
                builder.set(path.last(), v, f).unwrap();
                if path.is_empty() {
                    emit = Some(StreamingValue {
                        index: *index,
                        value: builder.read_root().unwrap().clone(),
                        is_final: true,
                    });
                    *index += 1;
                }
            }
            ParseEvent::Boolean { path, value } => {
                let v = f.build_from_bool(value);
                builder.set(path.last(), v, f).unwrap();
                if path.is_empty() {
                    emit = Some(StreamingValue {
                        index: *index,
                        value: builder.read_root().unwrap().clone(),
                        is_final: true,
                    });
                    *index += 1;
                }
            }
            ParseEvent::Number { path, value } => {
                let v = f.build_from_num(value);
                builder.set(path.last(), v, f).unwrap();
                if path.is_empty() {
                    emit = Some(StreamingValue {
                        index: *index,
                        value: builder.read_root().unwrap().clone(),
                        is_final: true,
                    });
                    *index += 1;
                }
            }
            ParseEvent::String {
                path,
                fragment,
                is_final,
                ..
            } => {
                builder
                    .mutate_with(
                        f,
                        path.last(),
                        |fac| {
                            let s0 = fac.new_string("");
                            fac.build_from_str(s0)
                        },
                        |val, fac| {
                            if let Value::String(s) = val {
                                fac.push_str(s, &fragment);
                                Ok(())
                            } else {
                                unreachable!("expected string at leaf")
                            }
                        },
                    )
                    .unwrap();
                if (partial && path.is_empty() && !is_final) || (is_final && path.is_empty()) {
                    emit = Some(StreamingValue {
                        index: *index,
                        value: builder.read_root().unwrap().clone(),
                        is_final,
                    });
                    if is_final {
                        *index += 1;
                    }
                }
            }
            ParseEvent::ArrayStart { path } => {
                builder
                    .enter_with(path.last(), f, |fac| {
                        let a0 = fac.new_array();
                        fac.build_from_array(a0)
                    })
                    .unwrap();
            }
            ParseEvent::ObjectBegin { path } => {
                builder
                    .enter_with(path.last(), f, |fac| {
                        let o0 = fac.new_object();
                        fac.build_from_object(o0)
                    })
                    .unwrap();
            }
            ParseEvent::ArrayEnd { path, .. } | ParseEvent::ObjectEnd { path, .. } => {
                if path.is_empty() {
                    if let Some(root) = builder.read_root() {
                        emit = Some(StreamingValue {
                            index: *index,
                            value: root.clone(),
                            is_final: true,
                        });
                        *index += 1;
                    }
                } else {
                    let _ = builder.pop().unwrap();
                }
            }
        }
        emit
    }
}

pub struct JsonModemValuesIter<'a, F: JsonValueFactory<Value = Value>> {
    parent: *mut JsonModemValues,
    inner: JsonModemIter<'a>,
    builder: ValueBuilder<Value>,
    index: usize,
    factory: F,
    partial: bool,
    emitted_partial: bool,
    last_emit_final: bool,
}

impl<F: JsonValueFactory<Value = Value>> Drop for JsonModemValuesIter<'_, F> {
    fn drop(&mut self) {
        unsafe {
            (*self.parent).builder = core::mem::take(&mut self.builder);
            (*self.parent).index = self.index;
        }
    }
}

impl<F: JsonValueFactory<Value = Value>> Iterator for JsonModemValuesIter<'_, F> {
    type Item = Result<StreamingValue, ParserError>;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.inner.next() {
                None => {
                    if self.partial && !self.emitted_partial {
                        if let Some(root) = self.builder.read_root() {
                            if !self.last_emit_final {
                                self.emitted_partial = true;
                                return Some(Ok(StreamingValue {
                                    index: self.index,
                                    value: root.clone(),
                                    is_final: false,
                                }));
                            }
                        }
                    }
                    return None;
                }
                Some(Err(e)) => return Some(Err(e)),
                Some(Ok(evt)) => {
                    if let Some(sv) = JsonModemValues::apply_event(
                        &mut self.builder,
                        &mut self.index,
                        &mut self.factory,
                        evt,
                        self.partial,
                    ) {
                        self.last_emit_final = sv.is_final;
                        return Some(Ok(sv));
                    }
                }
            }
        }
    }
}

pub struct JsonModemValuesClosed<F: JsonValueFactory<Value = Value>> {
    inner: JsonModemClosed,
    builder: ValueBuilder<Value>,
    index: usize,
    factory: F,
}

impl<F: JsonValueFactory<Value = Value>> Iterator for JsonModemValuesClosed<F> {
    type Item = Result<StreamingValue, ParserError>;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let ev = self.inner.next()?;
            match ev {
                Err(e) => return Some(Err(e)),
                Ok(evt) => {
                    if let Some(sv) = JsonModemValues::apply_event(
                        &mut self.builder,
                        &mut self.index,
                        &mut self.factory,
                        evt,
                        false,
                    ) {
                        return Some(Ok(sv));
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ValuesOptions {
    pub partial: bool,
}
