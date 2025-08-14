use alloc::vec::Vec;

use crate::{
    JsonModem, ParseEvent, ParserOptions, Path, Str, jsonmodem::JsonModemIter, value::Value,
};

/// Controls buffering behavior for the `JsonModemBuffers` adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BufferOptions {
    pub string_values: BufferStringMode,
}

/// Buffering policy for string values in the `JsonModemBuffers` adapter.
///
/// - `None`: never attach a buffered `value` (emit fragments only).
/// - `Values`: attach the full string only when the string ends.
/// - `Prefixes`: attach the growing prefix with every flush.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferStringMode {
    None,
    Values,
    Prefixes,
}
impl Default for BufferStringMode {
    fn default() -> Self {
        Self::None
    }
}

pub struct JsonModemBuffersIter<'a> {
    pub(crate) inner: JsonModemIter<'a>,
    pub(crate) opts: BufferOptions,
    pub(crate) pending_path: Option<Path>,
    pub(crate) pending_buf: Str,
    pub(crate) pending_final: bool,
    pub(crate) stash_non_string: Option<BufferedEvent>,
}

impl Iterator for JsonModemBuffersIter<'_> {
    type Item = Result<BufferedEvent, crate::parser::ParserError>;
    #[allow(
        clippy::too_many_lines,
        clippy::manual_let_else,
        clippy::single_match_else,
        clippy::match_same_arms
    )]
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(e) = self.stash_non_string.take() {
            return Some(Ok(e));
        }
        loop {
            let ev = match self.inner.next() {
                Some(ev) => ev,
                None => {
                    // End of chunk: flush pending for Prefixes mode, or when final
                    if let Some(path) = self.pending_path.take() {
                        let fragment = core::mem::take(&mut self.pending_buf);
                        let is_final = core::mem::take(&mut self.pending_final);
                        let value = match (self.opts.string_values, is_final) {
                            (BufferStringMode::Values, true) | (BufferStringMode::Prefixes, _) => {
                                Some(fragment.clone())
                            }
                            _ => None,
                        };
                        return Some(Ok(BufferedEvent::String {
                            path,
                            fragment,
                            value,
                            is_final,
                        }));
                    }
                    return None;
                }
            };
            match ev {
                Err(e) => return Some(Err(e)),
                Ok(ParseEvent::String {
                    path,
                    fragment,
                    is_final,
                    ..
                }) => {
                    match &self.pending_path {
                        None => {
                            self.pending_path = Some(path);
                            self.pending_buf.push_str(&fragment);
                            self.pending_final = is_final;
                        }
                        Some(p) if *p == path => {
                            self.pending_buf.push_str(&fragment);
                            self.pending_final |= is_final;
                        }
                        Some(_) => {
                            let out_evt = {
                                let cur_path = core::mem::take(&mut self.pending_path).unwrap();
                                let cur_buf = core::mem::take(&mut self.pending_buf);
                                let cur_final = core::mem::take(&mut self.pending_final);
                                let value = match (self.opts.string_values, cur_final) {
                                    (BufferStringMode::Values, true)
                                    | (BufferStringMode::Prefixes, _) => Some(cur_buf.clone()),
                                    _ => None,
                                };
                                BufferedEvent::String {
                                    path: cur_path,
                                    fragment: cur_buf,
                                    value,
                                    is_final: cur_final,
                                }
                            };
                            // seed new pending
                            self.pending_path = Some(path);
                            self.pending_buf.push_str(&fragment);
                            self.pending_final = is_final;
                            return Some(Ok(out_evt));
                        }
                    }
                }
                Ok(other) => {
                    if self.pending_path.is_some() {
                        let out_evt = {
                            let cur_path = core::mem::take(&mut self.pending_path).unwrap();
                            let cur_buf = core::mem::take(&mut self.pending_buf);
                            let cur_final = core::mem::take(&mut self.pending_final);
                            let value = match (self.opts.string_values, cur_final) {
                                (BufferStringMode::Values, true)
                                | (BufferStringMode::Prefixes, _) => Some(cur_buf.clone()),
                                _ => None,
                            };
                            BufferedEvent::String {
                                path: cur_path,
                                fragment: cur_buf,
                                value,
                                is_final: cur_final,
                            }
                        };
                        // map other into stash
                        self.stash_non_string = Some(match other {
                            ParseEvent::Null { path } => BufferedEvent::Null { path },
                            ParseEvent::Boolean { path, value } => {
                                BufferedEvent::Boolean { path, value }
                            }
                            ParseEvent::Number { path, value } => {
                                BufferedEvent::Number { path, value }
                            }
                            ParseEvent::ArrayStart { path } => BufferedEvent::ArrayStart { path },
                            ParseEvent::ArrayEnd { path, .. } => BufferedEvent::ArrayEnd { path },
                            ParseEvent::ObjectBegin { path } => BufferedEvent::ObjectBegin { path },
                            ParseEvent::ObjectEnd { path, .. } => BufferedEvent::ObjectEnd { path },
                            ParseEvent::String { .. } => unreachable!(),
                        });
                        return Some(Ok(out_evt));
                    }
                    let mapped = match other {
                        ParseEvent::Null { path } => BufferedEvent::Null { path },
                        ParseEvent::Boolean { path, value } => {
                            BufferedEvent::Boolean { path, value }
                        }
                        ParseEvent::Number { path, value } => BufferedEvent::Number { path, value },
                        ParseEvent::ArrayStart { path } => BufferedEvent::ArrayStart { path },
                        ParseEvent::ArrayEnd { path, .. } => BufferedEvent::ArrayEnd { path },
                        ParseEvent::ObjectBegin { path } => BufferedEvent::ObjectBegin { path },
                        ParseEvent::ObjectEnd { path, .. } => BufferedEvent::ObjectEnd { path },
                        ParseEvent::String { .. } => unreachable!(),
                    };
                    return Some(Ok(mapped));
                }
            }
        }
    }
}

/// `BufferedEvent` mirrors `ParseEvent` but adds an optional full string value
/// on string events.
#[cfg_attr(
    any(test, feature = "serde"),
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Debug, Clone, PartialEq)]
pub enum BufferedEvent {
    Null {
        path: Path,
    },
    Boolean {
        path: Path,
        value: bool,
    },
    Number {
        path: Path,
        value: f64,
    },
    String {
        path: Path,
        fragment: Str,
        value: Option<Str>,
        is_final: bool,
    },
    ArrayStart {
        path: Path,
    },
    ArrayEnd {
        path: Path,
    },
    ObjectBegin {
        path: Path,
    },
    ObjectEnd {
        path: Path,
    },
}

/// `JsonModemBuffers`: wraps `JsonModem` and buffers string fragments per-path.
#[derive(Debug)]
pub struct JsonModemBuffers {
    pub(crate) modem: JsonModem,
    pub(crate) opts: BufferOptions,
    // Pending coalesced string across core iterator pulls for collect()/finish()
    pub(crate) scratch: Option<(Path, Str)>, // (path, buffer)
}

impl JsonModemBuffers {
    #[must_use]
    pub fn new(options: ParserOptions, opts: BufferOptions) -> Self {
        Self {
            modem: JsonModem::new(options),
            opts,
            scratch: None,
        }
    }

    /// Iterator over buffered events for this chunk, coalescing consecutive
    /// string events.
    pub fn feed<'a>(&'a mut self, chunk: &str) -> JsonModemBuffersIter<'a> {
        JsonModemBuffersIter {
            inner: self.modem.feed(chunk),
            opts: self.opts,
            pending_path: None,
            pending_buf: Str::new(),
            pending_final: false,
            stash_non_string: None,
        }
    }

    /// Collect buffered events from a chunk.
    ///
    /// # Errors
    /// Returns any parser error encountered while consuming the inner iterator.
    pub fn collect(
        &mut self,
        chunk: &str,
    ) -> Result<Vec<BufferedEvent>, crate::parser::ParserError> {
        let mut out = Vec::new();
        let events: alloc::vec::Vec<_> = self.modem.feed(chunk).collect();
        for ev in events {
            self.push_buffered(ev?, &mut out);
        }
        Ok(out)
    }

    /// Finish the stream and collect remaining buffered events.
    ///
    /// # Errors
    /// Returns any parser error encountered while consuming the inner iterator.
    pub fn finish(self) -> Result<Vec<BufferedEvent>, crate::parser::ParserError> {
        let JsonModemBuffers {
            modem,
            opts,
            mut scratch,
        } = self;
        let mut out = Vec::new();
        let mut closed = modem.finish();
        for ev in &mut closed {
            let ev = ev?;
            match ev {
                ParseEvent::String {
                    path,
                    fragment,
                    is_final,
                    ..
                } => {
                    match &mut scratch {
                        Some((p, buf)) if *p == path => {
                            buf.push_str(&fragment);
                        }
                        Some((p, buf)) => {
                            // Flush previous pending before switching
                            let prev_path = core::mem::take(p);
                            let prev_buf = core::mem::take(buf);
                            let prev_value = match opts.string_values {
                                BufferStringMode::Prefixes => Some(prev_buf.clone()),
                                BufferStringMode::Values | BufferStringMode::None => None,
                            };
                            out.push(BufferedEvent::String {
                                path: prev_path,
                                fragment: prev_buf,
                                value: prev_value,
                                is_final: false,
                            });
                            // Start new pending
                            p.clone_from(&path);
                            buf.clone_from(&fragment);
                        }
                        None => scratch = Some((path.clone(), fragment.clone())),
                    }
                    // If final, flush immediately
                    if is_final {
                        if let Some((p, buf)) = scratch.take() {
                            let val = Some(buf.clone());
                            out.push(BufferedEvent::String {
                                path: p,
                                fragment: buf,
                                value: val,
                                is_final: true,
                            });
                        }
                    }
                }
                ParseEvent::Null { path } => out.push(BufferedEvent::Null { path }),
                ParseEvent::Boolean { path, value } => {
                    out.push(BufferedEvent::Boolean { path, value });
                }
                ParseEvent::Number { path, value } => {
                    out.push(BufferedEvent::Number { path, value });
                }
                ParseEvent::ArrayStart { path } => out.push(BufferedEvent::ArrayStart { path }),
                ParseEvent::ArrayEnd { path, .. } => {
                    if let Some((p, _)) = &scratch {
                        if p.len() > path.len() {
                            scratch = None;
                        }
                    }
                    out.push(BufferedEvent::ArrayEnd { path });
                }
                ParseEvent::ObjectBegin { path } => out.push(BufferedEvent::ObjectBegin { path }),
                ParseEvent::ObjectEnd { path, .. } => {
                    if let Some((p, _)) = &scratch {
                        if p.len() > path.len() {
                            scratch = None;
                        }
                    }
                    out.push(BufferedEvent::ObjectEnd { path });
                }
            }
        }
        // End-of-input: if any pending remains (e.g., prefixes mode across chunk),
        // flush
        if let Some((path, fragment)) = scratch.take() {
            let value = match opts.string_values {
                BufferStringMode::Prefixes => Some(fragment.clone()),
                BufferStringMode::Values | BufferStringMode::None => None,
            };
            out.push(BufferedEvent::String {
                path,
                fragment,
                value,
                is_final: false,
            });
        }
        Ok(out)
    }

    pub(crate) fn push_buffered(&mut self, ev: ParseEvent<Value>, out: &mut Vec<BufferedEvent>) {
        match ev {
            ParseEvent::String {
                path,
                fragment,
                is_final,
                ..
            } => {
                match &mut self.scratch {
                    Some((p, buf)) if *p == path => {
                        buf.push_str(&fragment);
                    }
                    Some((p, buf)) => {
                        let prev_path = core::mem::take(p);
                        let prev_buf = core::mem::take(buf);
                        let prev_value = match self.opts.string_values {
                            BufferStringMode::Prefixes => Some(prev_buf.clone()),
                            BufferStringMode::Values | BufferStringMode::None => None,
                        };
                        out.push(BufferedEvent::String {
                            path: prev_path,
                            fragment: prev_buf,
                            value: prev_value,
                            is_final: false,
                        });
                        p.clone_from(&path);
                        buf.clone_from(&fragment);
                    }
                    None => self.scratch = Some((path.clone(), fragment.clone())),
                }
                if is_final {
                    if let Some((p, buf)) = self.scratch.take() {
                        let frag = buf.clone();
                        let val = Some(frag.clone());
                        out.push(BufferedEvent::String {
                            path: p,
                            fragment: frag,
                            value: val,
                            is_final: true,
                        });
                    }
                }
            }
            ParseEvent::Null { path } => out.push(BufferedEvent::Null { path }),
            ParseEvent::Boolean { path, value } => out.push(BufferedEvent::Boolean { path, value }),
            ParseEvent::Number { path, value } => out.push(BufferedEvent::Number { path, value }),
            ParseEvent::ArrayStart { path } => out.push(BufferedEvent::ArrayStart { path }),
            ParseEvent::ArrayEnd { path, .. } => {
                if let Some((p, _)) = &self.scratch {
                    if p.len() > path.len() {
                        self.scratch = None;
                    }
                }
                out.push(BufferedEvent::ArrayEnd { path });
            }
            ParseEvent::ObjectBegin { path } => out.push(BufferedEvent::ObjectBegin { path }),
            ParseEvent::ObjectEnd { path, .. } => {
                if let Some((p, _)) = &self.scratch {
                    if p.len() > path.len() {
                        self.scratch = None;
                    }
                }
                out.push(BufferedEvent::ObjectEnd { path });
            }
        }
    }
}
