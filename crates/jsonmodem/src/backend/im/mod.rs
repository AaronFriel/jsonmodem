/// Immutable value definitions, zipper, and applicator for [`ImBackend`].
pub mod value;
/// Event-to-value applicator that builds immutable trees from parser events.
pub mod value_applicator;
/// Persistent zipper utilities that mutate immutable values incrementally.
pub mod value_zipper;

use alloc::{string::String, vec::Vec};
use core::num::ParseFloatError;

use rpds::Vector;

pub use self::value::{Array, Map, Str};
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

pub type ImPath = Vector<PathItem>;
type ImBufferedEvent<'a> = BorrowedBufferedEvent<'a, ImBackend>;

/// Backend that builds immutable JSON values using persistent containers.
#[derive(Debug, PartialEq, Clone)]
#[non_exhaustive]
pub struct ImBackend {
    decode_mode: RustDecodeMode,
}

/// Modes controlling how raw string fragments are decoded for [`ImBackend`].
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RustDecodeMode {
    StrictUnicode,
    ReplaceInvalid,
}

impl Default for ImBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl ImBackend {
    /// Creates a backend with lossy UTF-8 decoding for string fragments.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            decode_mode: RustDecodeMode::ReplaceInvalid,
        }
    }

    /// Overrides the decode mode used when decoding raw string fragments.
    #[must_use]
    pub fn with_decode_mode(mut self, decode_mode: RustDecodeMode) -> Self {
        self.decode_mode = decode_mode;
        self
    }

    /// Returns the active decode mode.
    #[must_use]
    pub fn decode_mode(&self) -> RustDecodeMode {
        self.decode_mode
    }
}

impl PathCtx for ImBackend {
    type PathState = Vector<PathItem>;
    type Path = ImPath;

    fn frozen_new(&mut self) -> Self::PathState {
        Vector::new()
    }

    fn thaw(&mut self, frozen: Self::PathState) -> Self::Path {
        frozen
    }

    fn freeze(&mut self, thawed: Self::Path) -> Self::PathState {
        thawed
    }

    fn push_key_from_str(&mut self, t: &mut Self::Path, key: &str) {
        t.push_back_mut(PathItem::Key(key.into()));
    }

    fn push_index_zero(&mut self, t: &mut Self::Path) {
        t.push_back_mut(PathItem::Index(0));
    }

    fn bump_last_index(&mut self, t: &mut Self::Path) -> Result<(), PathError> {
        let Some(last_index) = t.len().checked_sub(1) else {
            return Err(PathError::NotArrayFrame);
        };
        match t.get_mut(last_index) {
            Some(PathItem::Index(index)) => {
                *index += 1;
                Ok(())
            }
            _ => Err(PathError::NotArrayFrame),
        }
    }

    fn pop_kind(&mut self, t: &mut Self::Path) -> Option<PathKind> {
        let kind = t.last().map(|component| match component {
            PathItem::Key(_) => PathKind::Key,
            PathItem::Index(_) => PathKind::Index,
        });
        if kind.is_some() {
            let _ = t.drop_last_mut();
        }
        kind
    }

    fn last_kind(&self, t: &Self::Path) -> Option<PathKind> {
        t.last().map(|component| match component {
            PathItem::Key(_) => PathKind::Key,
            PathItem::Index(_) => PathKind::Index,
        })
    }
}

impl ValueCtx for ImBackend {
    type Null = ();
    type Bool = bool;
    type Num<'src> = f64;
    type Str<'src> = Str;
    type Value = Value;
}

impl EventCtx for ImBackend {
    type Error = ParseFloatError;

    fn push_key_from_raw_str(&mut self, t: &mut Self::Path, key: &[u8]) {
        t.push_back_mut(PathItem::Key(String::from_utf8_lossy(key).into()));
    }

    fn new_null(&mut self) -> Result<Self::Null, Self::Error> {
        Ok(())
    }

    fn new_bool(&mut self, b: bool) -> Result<Self::Bool, Self::Error> {
        Ok(b)
    }

    fn new_number<'src>(&mut self, n: &'src str) -> Result<Self::Num<'src>, Self::Error> {
        n.parse()
    }

    fn new_number_owned<'a>(&mut self, n: String) -> Result<Self::Num<'a>, Self::Error> {
        n.parse()
    }

    fn new_str<'src>(&mut self, frag: &'src str) -> Result<Self::Str<'src>, Self::Error> {
        Ok(Str::from(frag))
    }

    fn new_str_owned<'a>(&mut self, frag: String) -> Result<Self::Str<'a>, Self::Error> {
        Ok(Str::from(frag))
    }

    fn new_str_raw_owned<'a>(&mut self, bytes: Vec<u8>) -> Result<Self::Str<'a>, Self::Error> {
        match self.decode_mode {
            RustDecodeMode::StrictUnicode => {
                let owned = String::from_utf8(bytes)
                    .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
                Ok(Str::from(owned))
            }
            RustDecodeMode::ReplaceInvalid => {
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
                Ok(Str::from(owned))
            }
        }
    }
}

impl OwnedEventCtx for ImBackend {
    type OwnedNum = f64;
    type OwnedStr = Str;

    fn num_into_owned(n: Self::Num<'_>) -> Self::OwnedNum {
        n
    }

    fn str_into_owned(s: Self::Str<'_>) -> Self::OwnedStr {
        s
    }
}

impl BuilderCtx for ImBackend {
    type Array = Array;
    type Object = Map;
}

/// String-buffering assembler for [`ImBackend`].
#[allow(dead_code)]
#[derive(Debug)]
pub struct ImStringAssembler {
    scratch: Str,
    options: BufferOptions,
}

impl ImStringAssembler {
    /// Creates a new assembler with the provided buffering options.
    #[must_use]
    pub fn new(options: BufferOptions) -> Self {
        Self {
            scratch: Str::default(),
            options,
        }
    }

    /// Returns the configured buffering behaviour.
    #[must_use]
    pub fn options(&self) -> BufferOptions {
        self.options
    }

    fn string_value(&self) -> Str {
        self.scratch.clone()
    }
}

impl BufferAssembler<ImBackend> for ImStringAssembler {
    fn on_event<'a, 'src>(
        &'a mut self,
        event: ParseEvent<'src, &'a ImPath, ImBackend>,
    ) -> Result<ImBufferedEvent<'a>, ParseFloatError>
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
                let value = Some(self.string_value());
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

/// Immutable value assembler built around rpds containers.
#[derive(Debug)]
pub struct ImValueAssembler {
    applicator: ValueApplicator,
}

impl ImValueAssembler {
    /// Creates a new value assembler for the supplied buffering options.
    #[must_use]
    pub fn new(options: BufferOptions) -> Self {
        Self {
            applicator: ValueApplicator::new(options),
        }
    }

    /// Returns the current root value without consuming it.
    #[must_use]
    pub fn read_root(&self) -> &Value {
        self.applicator.read_root()
    }

    /// Consumes the accumulated root value, replacing it with `null`.
    pub fn take_root(&mut self) -> Value {
        self.applicator.take_root()
    }

    fn map_scalar<'a>(path: &'a ImPath, leaf: &'a Value) -> ImBufferedEvent<'a> {
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

    fn map_string(
        path: &ImPath,
        fragment: Str,
        is_initial: bool,
        is_final: bool,
        buffered: Option<Str>,
    ) -> ImBufferedEvent<'_> {
        BufferedEvent::String {
            path,
            fragment,
            value: buffered,
            is_initial,
            is_final,
        }
    }

    fn map_array_begin(path: &ImPath) -> ImBufferedEvent<'_> {
        BufferedEvent::ArrayBegin { path }
    }

    fn map_array_end<'a>(path: &'a ImPath, value: &'a Value) -> ImBufferedEvent<'a> {
        BufferedEvent::ArrayEnd {
            path,
            value: value.as_array(),
        }
    }

    fn map_object_begin(path: &ImPath) -> ImBufferedEvent<'_> {
        BufferedEvent::ObjectBegin { path }
    }

    fn map_object_end<'a>(path: &'a ImPath, value: &'a Value) -> ImBufferedEvent<'a> {
        BufferedEvent::ObjectEnd {
            path,
            value: value.as_object(),
        }
    }

    fn map_event(applied: AppliedRef<'_>) -> ImBufferedEvent<'_> {
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

impl BufferAssembler<ImBackend> for ImValueAssembler {
    fn on_event<'a, 'src>(
        &'a mut self,
        event: ParseEvent<'src, &'a ImPath, ImBackend>,
    ) -> Result<ImBufferedEvent<'a>, ParseFloatError>
    where
        'src: 'a,
    {
        let applied = self.applicator.push(&event);
        Ok(Self::map_event(applied))
    }
}

impl RootedBufferAssembler<ImBackend> for ImValueAssembler
where
    <ImBackend as PathCtx>::Path: crate::jsonmodem_buffers::PathRoot,
{
    fn root(&self) -> &Value {
        self.read_root()
    }
}
