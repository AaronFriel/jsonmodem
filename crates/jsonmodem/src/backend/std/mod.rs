pub mod value;
pub mod value_applicator;
pub mod value_tree;
pub mod value_zipper;

use alloc::{borrow::Cow, collections::BTreeMap, string::String, sync::Arc, vec::Vec};
use core::num::ParseFloatError;

use self::{
    value::Value,
    value_applicator::{AppliedRef, ValueApplicator},
};
use crate::{
    buffer_options::BufferOptions,
    context::{BuilderCtx, EventCtx, OwnedEventCtx, PathCtx, PathError, PathKind, ValueCtx},
    event::ParseEvent,
    jsonmodem_buffers::{
        BorrowedBufferedEvent, BufferAssembler, BufferedEvent, RootedBufferAssembler,
    },
    path::PathItem,
};
type StdBufferedEvent<'a> = BorrowedBufferedEvent<'a, StdBackend>;

/// Context type for the Rust backend used by the JSON streaming parser.
///
/// It defines how strings are decoded from raw bytes and provides
/// concrete types for path and event data emitted during parsing.
#[derive(Debug, PartialEq, Clone)]
#[non_exhaustive]
pub struct StdBackend {
    decode_mode: RustDecodeMode,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
/// Modes for decoding raw bytes into strings.
#[non_exhaustive]
pub enum RustDecodeMode {
    /// Strict UTF-8: invalid sequences are preserved only if already valid;
    /// otherwise rejected.
    StrictUnicode,
    /// Lossy decoding: invalid sequences are replaced (e.g., with the
    /// replacement character).
    ReplaceInvalid,
}

impl Default for StdBackend {
    fn default() -> Self {
        Self {
            decode_mode: RustDecodeMode::ReplaceInvalid,
        }
    }
}

impl StdBackend {
    /// Constructs a backend with the default lossy decoding strategy.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            decode_mode: RustDecodeMode::ReplaceInvalid,
        }
    }

    /// Overrides the Unicode decoding mode used for raw string fragments.
    #[must_use]
    pub fn with_decode_mode(mut self, decode_mode: RustDecodeMode) -> Self {
        self.decode_mode = decode_mode;
        self
    }

    /// Returns the configured decode mode.
    #[must_use]
    pub fn decode_mode(&self) -> RustDecodeMode {
        self.decode_mode
    }
}

pub type StdPath = Vec<PathItem>;

impl PathCtx for StdBackend {
    type PathState = Vec<PathItem>;
    type Path = StdPath;

    #[inline]
    fn frozen_new(&mut self) -> Self::PathState {
        Vec::new()
    }

    #[inline]
    fn thaw(&mut self, frozen: Self::PathState) -> Self::Path {
        frozen
    }

    #[inline]
    fn freeze(&mut self, thawed: Self::Path) -> Self::PathState {
        thawed
    }

    #[inline]
    fn push_key_from_str(&mut self, t: &mut Self::Path, key: &str) {
        t.push(PathItem::Key(key.into()));
    }

    #[inline]
    fn push_index_zero(&mut self, t: &mut Self::Path) {
        t.push(PathItem::Index(0));
    }

    #[inline]
    fn bump_last_index(&mut self, t: &mut Self::Path) -> Result<(), PathError> {
        let Some(PathItem::Index(i)) = t.last_mut() else {
            return Err(PathError::NotArrayFrame);
        };
        *i += 1;
        Ok(())
    }

    #[inline]
    fn pop_kind(&mut self, t: &mut Self::Path) -> Option<PathKind> {
        t.pop().map(
            #[inline]
            |item| match item {
                PathItem::Key(_) => PathKind::Key,
                PathItem::Index(_) => PathKind::Index,
            },
        )
    }

    #[inline]
    fn last_kind(&self, t: &Self::Path) -> Option<PathKind> {
        t.last().map(
            #[inline]
            |item| match item {
                PathItem::Key(_) => PathKind::Key,
                PathItem::Index(_) => PathKind::Index,
            },
        )
    }
}

impl ValueCtx for StdBackend {
    type Null = ();
    type Bool = bool;
    type Num<'src> = f64;
    type Str<'src> = Cow<'src, str>;
    type Value = Value;
}

impl EventCtx for StdBackend {
    type Error = ParseFloatError;

    #[inline]
    fn push_key_from_raw_str(&mut self, t: &mut Self::Path, key: &[u8]) {
        t.push(PathItem::Key(String::from_utf8_lossy(key).into()));
    }

    #[inline]
    fn new_null(&mut self) -> Result<Self::Null, Self::Error> {
        Ok(())
    }

    #[inline]
    fn new_bool(&mut self, b: bool) -> Result<Self::Bool, Self::Error> {
        Ok(b)
    }

    #[inline]
    fn new_number<'src>(&mut self, n: &'src str) -> Result<Self::Num<'src>, Self::Error> {
        n.parse()
    }

    #[inline]
    fn new_number_owned<'a>(&mut self, n: String) -> Result<Self::Num<'a>, Self::Error> {
        n.parse()
    }

    #[inline]
    fn new_str<'src>(&mut self, frag: &'src str) -> Result<Self::Str<'src>, Self::Error> {
        Ok(Cow::Borrowed(frag))
    }

    #[inline]
    fn new_str_owned<'a>(&mut self, frag: String) -> Result<Self::Str<'a>, Self::Error> {
        Ok(Cow::Owned(frag))
    }

    #[inline]
    fn new_str_raw_owned<'a>(&mut self, bytes: Vec<u8>) -> Result<Self::Str<'a>, Self::Error> {
        match self.decode_mode {
            RustDecodeMode::StrictUnicode => {
                // In strict mode, reject non-UTF8 raw input.
                // Parser should avoid calling this in strict mode; if it does,
                // we still avoid panicking by producing an error-like lossy string.
                let owned = String::from_utf8(bytes)
                    .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
                Ok(Cow::Owned(owned))
            }
            RustDecodeMode::ReplaceInvalid => {
                // Decode lossily; special-case WTF-8 surrogate code units to U+FFFD.
                let mut norm = Vec::with_capacity(bytes.len());
                let mut i = 0;
                while i < bytes.len() {
                    if i + 2 < bytes.len()
                        && bytes[i] == 0xED
                        && (bytes[i + 1] >= 0xA0 && bytes[i + 1] <= 0xBF)
                        && (bytes[i + 2] & 0xC0) == 0x80
                    {
                        norm.extend_from_slice(&[0xEF, 0xBF, 0xBD]);
                        i += 3;
                    } else {
                        norm.push(bytes[i]);
                        i += 1;
                    }
                }
                let owned = match String::from_utf8(norm) {
                    Ok(s) => s,
                    Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
                };
                Ok(Cow::Owned(owned))
            }
        }
    }
}

impl OwnedEventCtx for StdBackend {
    type OwnedNum = f64;
    type OwnedStr = String;

    #[inline]
    fn num_into_owned(n: Self::Num<'_>) -> Self::OwnedNum {
        n
    }

    #[inline]
    fn str_into_owned(s: Self::Str<'_>) -> Self::OwnedStr {
        s.into_owned()
    }
}

impl BuilderCtx for StdBackend {
    type Array = Vec<Value>;
    type Object = BTreeMap<Arc<str>, Value>;
}

#[derive(Debug)]
pub struct StdStringAssembler {
    scratch: String,
    options: BufferOptions,
}

impl StdStringAssembler {
    #[inline]
    pub fn new(options: BufferOptions) -> Self {
        Self {
            scratch: String::new(),
            options,
        }
    }

    #[inline]
    pub fn options(&self) -> BufferOptions {
        self.options
    }

    #[inline]
    fn string_value(&mut self, _is_final: bool) -> Cow<'_, str> {
        // Always emit prefixes for the string-accumulating assembler.
        Cow::Borrowed(self.scratch.as_str())
    }
}

impl BufferAssembler<StdBackend> for StdStringAssembler {
    #[inline]
    fn on_event<'a, 'src>(
        &'a mut self,
        event: ParseEvent<'src, &'a StdPath, StdBackend>,
    ) -> Result<StdBufferedEvent<'a>, ParseFloatError>
    where
        'src: 'a,
    {
        match event {
            ParseEvent::Null { path } => Ok(BufferedEvent::Null { path }),
            ParseEvent::Boolean { path, value } => Ok(BufferedEvent::Boolean { path, value }),
            ParseEvent::Number { path, value } => Ok(BufferedEvent::Number { path, value }),
            ParseEvent::String {
                path,
                fragment,
                is_initial,
                is_final,
            } => {
                if is_initial {
                    self.scratch.clear();
                }
                self.scratch.push_str(fragment.as_ref());
                let value = Some(self.string_value(is_final));
                Ok(BufferedEvent::String {
                    path,
                    fragment,
                    value,
                    is_initial,
                    is_final,
                })
            }
            ParseEvent::ArrayBegin { path } => Ok(BufferedEvent::ArrayBegin { path }),
            ParseEvent::ArrayEnd { path } => Ok(BufferedEvent::ArrayEnd { path, value: None }),
            ParseEvent::ObjectBegin { path } => Ok(BufferedEvent::ObjectBegin { path }),
            ParseEvent::ObjectEnd { path } => Ok(BufferedEvent::ObjectEnd { path, value: None }),
        }
    }
}

#[derive(Debug)]
pub struct StdValueAssembler {
    applicator: ValueApplicator,
}

impl StdValueAssembler {
    #[inline]
    pub fn new(options: BufferOptions) -> Self {
        Self {
            applicator: ValueApplicator::new(options),
        }
    }

    #[inline]
    pub fn read_root(&self) -> &Value {
        self.applicator.read_root()
    }

    #[inline]
    pub fn take_root(&mut self) -> Value {
        self.applicator.take_root()
    }

    #[inline]
    fn map_scalar<'a>(path: &'a StdPath, leaf: &'a Value) -> StdBufferedEvent<'a> {
        match leaf {
            Value::Null => BufferedEvent::Null { path },
            Value::Boolean(flag) => BufferedEvent::Boolean { path, value: *flag },
            Value::Number(number) => BufferedEvent::Number {
                path,
                value: *number,
            },
            Value::String(_) | Value::Array(_) | Value::Object(_) => {
                unreachable!("scalar value expected")
            }
        }
    }

    #[inline]
    fn map_string<'a>(
        path: &'a StdPath,
        fragment: Cow<'a, str>,
        is_initial: bool,
        is_final: bool,
        buffered: Option<&'a str>,
    ) -> StdBufferedEvent<'a> {
        BufferedEvent::String {
            path,
            fragment,
            value: buffered.map(Cow::Borrowed),
            is_initial,
            is_final,
        }
    }

    #[inline]
    fn map_array_begin(path: &StdPath) -> StdBufferedEvent<'_> {
        BufferedEvent::ArrayBegin { path }
    }

    #[inline]
    fn map_array_end<'a>(path: &'a StdPath, value: &'a Value) -> StdBufferedEvent<'a> {
        BufferedEvent::ArrayEnd {
            path,
            value: value.as_array(),
        }
    }

    #[inline]
    fn map_object_begin(path: &StdPath) -> StdBufferedEvent<'_> {
        BufferedEvent::ObjectBegin { path }
    }

    #[inline]
    fn map_object_end<'a>(path: &'a StdPath, value: &'a Value) -> StdBufferedEvent<'a> {
        BufferedEvent::ObjectEnd {
            path,
            value: value.as_object(),
        }
    }

    #[inline]
    fn map_event(applied: AppliedRef<'_>) -> StdBufferedEvent<'_> {
        match applied {
            AppliedRef::Scalar { path, leaf } => Self::map_scalar(path, leaf),
            AppliedRef::String {
                path,
                fragment,
                is_initial,
                is_final,
                buffered,
                ..
            } => Self::map_string(path, fragment, is_initial, is_final, buffered),
            AppliedRef::ArrayBegin { path, .. } => Self::map_array_begin(path),
            AppliedRef::ArrayEnd { path, leaf, .. } => Self::map_array_end(path, leaf),
            AppliedRef::ObjectBegin { path, .. } => Self::map_object_begin(path),
            AppliedRef::ObjectEnd { path, leaf, .. } => Self::map_object_end(path, leaf),
            AppliedRef::Nothing => unreachable!("applicator is 1:1"),
        }
    }
}

impl BufferAssembler<StdBackend> for StdValueAssembler {
    #[inline]
    fn on_event<'a, 'src>(
        &'a mut self,
        event: ParseEvent<'src, &'a StdPath, StdBackend>,
    ) -> Result<StdBufferedEvent<'a>, ParseFloatError>
    where
        'src: 'a,
    {
        let applied = self.applicator.push(event);
        Ok(Self::map_event(applied))
    }
}

impl RootedBufferAssembler<StdBackend> for StdValueAssembler
where
    <StdBackend as PathCtx>::Path: crate::jsonmodem_buffers::PathRoot,
{
    #[inline]
    fn root(&self) -> &Value {
        self.read_root()
    }
}

#[allow(dead_code)]
pub type StdBufferAssembler = StdValueAssembler;
