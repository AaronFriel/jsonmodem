#![allow(clippy::needless_borrow, clippy::single_match_else)]
use alloc::{
    borrow::{Cow, ToOwned},
    collections::BTreeMap,
    string::String,
    vec::Vec,
};
use core::num::ParseFloatError;

#[cfg(any(fuzzing, debug_assertions))]
use crate::backend::RootTransition;
#[cfg(any(fuzzing, debug_assertions))]
use crate::backend::TransitionAsserter;
use crate::{
    Path, PathItem,
    backend::ParserCursor,
    buffer_options::BufferOptions,
    context::{BuilderCtx, EventCtx, PathCtx, PathError, PathKind, ValueCtx},
    event::ParseEvent,
    jsonmodem_buffers::{
        BorrowedBufferedEvent, BufferAssembler, BufferedEvent, RootedBufferAssembler,
    },
    value::Value,
    value_tree::ValueTree,
};

#[derive(Debug, Default, PartialEq, Clone)]
#[doc(hidden)]
pub struct RawContext;

type RawPath = Vec<PathItem<Vec<u8>, usize>>;

impl PathCtx for RawContext {
    type PathState = Vec<PathItem<Vec<u8>, usize>>;
    type Path = RawPath;

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
        t.pop().map(|item| match item {
            PathItem::Key(_) => PathKind::Key,
            PathItem::Index(_) => PathKind::Index,
        })
    }

    #[inline]
    fn last_kind(&self, t: &Self::Path) -> Option<PathKind> {
        t.last().map(|item| match item {
            PathItem::Key(_) => PathKind::Key,
            PathItem::Index(_) => PathKind::Index,
        })
    }
}

impl ValueCtx for RawContext {
    type Null = ();
    type Bool = bool;
    type Num<'src> = f64;
    type Str<'src> = Cow<'src, [u8]>;
    type Value = Value;
}

impl EventCtx for RawContext {
    type Error = ParseFloatError;

    #[inline]
    fn push_key_from_raw_str(&mut self, t: &mut Self::Path, key: &[u8]) {
        t.push(PathItem::Key(key.into()));
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
        Ok(Cow::Borrowed(frag.as_bytes()))
    }

    #[inline]
    fn new_str_owned<'a>(&mut self, frag: String) -> Result<Self::Str<'a>, Self::Error> {
        Ok(Cow::Owned(frag.into_bytes()))
    }

    #[inline]
    fn new_str_raw_owned<'a>(&mut self, bytes: Vec<u8>) -> Result<Self::Str<'a>, Self::Error> {
        Ok(Cow::Owned(bytes))
    }
}

impl BuilderCtx for RawContext {
    type Array = Vec<Value>;
    type Object = BTreeMap<Vec<u8>, Value>;
}

#[derive(Debug)]
#[doc(hidden)]
pub struct RawBufferAssembler {
    values: ValueTree,
    string_scratch: Option<(Path, String)>,
    array_scratch: Option<Vec<Value>>,
    object_scratch: Option<BTreeMap<Vec<u8>, Value>>,
    cursor: ParserCursor,
    #[cfg(any(fuzzing, debug_assertions))]
    transitions: TransitionAsserter,
}

impl RawBufferAssembler {
    #[must_use]
    pub fn new(_options: BufferOptions) -> Self {
        Self {
            values: ValueTree::default(),
            string_scratch: None,
            array_scratch: None,
            object_scratch: None,
            cursor: ParserCursor::new(),
            #[cfg(any(fuzzing, debug_assertions))]
            transitions: TransitionAsserter::new(),
        }
    }

    #[inline]
    fn convert_path(path: &[crate::PathItem<Vec<u8>, usize>]) -> Path {
        path.iter()
            .map(|component| match component {
                crate::PathItem::Key(bytes) => PathItem::Key(String::from_utf8_lossy(bytes).into()),
                crate::PathItem::Index(index) => PathItem::Index(*index),
            })
            .collect()
    }

    #[inline]
    fn update_string_value(&mut self, path: &Path, fragment: &str, is_final: bool) -> String {
        // Always emit prefixes for raw assembler; return the full value on
        // final and the prefix otherwise.
        let scratch = self
            .string_scratch
            .get_or_insert_with(|| (path.clone(), String::new()));
        if scratch.0 != *path {
            path.clone_into(&mut scratch.0);
            scratch.1.clear();
        }
        scratch.1.push_str(fragment);
        if is_final {
            let value = scratch.1.clone();
            self.string_scratch = None;
            value
        } else {
            scratch.1.clone()
        }
    }

    #[inline]
    fn container_value(&self, path: &Path) -> Option<Value> {
        self.values.clone_at_path(path)
    }

    #[must_use]
    #[inline]
    pub fn read_root(&self) -> &Value {
        self.values.root()
    }
}

impl BufferAssembler<RawContext> for RawBufferAssembler {
    #[allow(clippy::cognitive_complexity)]
    #[allow(clippy::too_many_lines)]
    #[inline]
    fn on_event<'a, 'src>(
        &'a mut self,
        event: ParseEvent<'src, &'a RawPath, RawContext>,
    ) -> Result<BorrowedBufferedEvent<'a, RawContext>, ParseFloatError>
    where
        'src: 'a,
    {
        #[cfg(any(fuzzing, debug_assertions))]
        self.transitions.observe(&event);

        let outcome = self.cursor.classify_transition(&event);

        let result = match event {
            ParseEvent::Null { path } => {
                #[cfg(any(fuzzing, debug_assertions))]
                assert!(matches!(
                    outcome.transition,
                    RootTransition::StartRootScalar
                        | RootTransition::StayArray { .. }
                        | RootTransition::StayObject { .. }
                ));
                let canonical = Self::convert_path(&path);
                self.values.insert_value(&canonical, Value::Null);
                Ok(BufferedEvent::Null { path })
            }
            ParseEvent::Boolean { path, value } => {
                #[cfg(any(fuzzing, debug_assertions))]
                assert!(matches!(
                    outcome.transition,
                    RootTransition::StartRootScalar
                        | RootTransition::StayArray { .. }
                        | RootTransition::StayObject { .. }
                ));
                let canonical = Self::convert_path(&path);
                self.values.insert_value(&canonical, Value::Boolean(value));
                Ok(BufferedEvent::Boolean { path, value })
            }
            ParseEvent::Number { path, value } => {
                #[cfg(any(fuzzing, debug_assertions))]
                assert!(matches!(
                    outcome.transition,
                    RootTransition::StartRootScalar
                        | RootTransition::StayArray { .. }
                        | RootTransition::StayObject { .. }
                ));
                let canonical = Self::convert_path(&path);
                self.values.insert_value(&canonical, Value::Number(value));
                Ok(BufferedEvent::Number { path, value })
            }
            ParseEvent::String {
                path,
                fragment,
                is_initial,
                is_final,
            } => {
                if !is_initial {
                    #[cfg(any(fuzzing, debug_assertions))]
                    assert!(matches!(
                        outcome.transition,
                        RootTransition::AppendString { .. }
                    ));
                }
                let canonical = Self::convert_path(&path);
                let fragment_text = String::from_utf8_lossy(fragment.as_ref()).into_owned();
                self.values.append_string(&canonical, &fragment_text);
                let buffered = self.update_string_value(&canonical, &fragment_text, is_final);
                let fragment_owned = Cow::Owned(fragment.into_owned());
                let value = Some(Cow::Owned(buffered.into_bytes()));
                Ok(BufferedEvent::String {
                    path,
                    fragment: fragment_owned,
                    value,
                    is_initial,
                    is_final,
                })
            }
            ParseEvent::ArrayBegin { path } => {
                #[cfg(any(fuzzing, debug_assertions))]
                assert!(matches!(
                    outcome.transition,
                    RootTransition::PushArray
                        | RootTransition::StayArray { .. }
                        | RootTransition::StayObject { .. }
                ));
                let canonical = Self::convert_path(&path);
                self.values
                    .insert_value(&canonical, Value::Array(Vec::new()));
                Ok(BufferedEvent::ArrayBegin { path })
            }
            ParseEvent::ArrayEnd { path } => {
                #[cfg(any(fuzzing, debug_assertions))]
                assert!(matches!(outcome.transition, RootTransition::PopContainer));
                let canonical = Self::convert_path(&path);
                let value = match self.container_value(&canonical) {
                    Some(Value::Array(array)) => {
                        self.array_scratch = Some(array);
                        self.array_scratch.as_ref()
                    }
                    _ => {
                        self.array_scratch = None;
                        None
                    }
                };
                Ok(BufferedEvent::ArrayEnd { path, value })
            }
            ParseEvent::ObjectBegin { path } => {
                #[cfg(any(fuzzing, debug_assertions))]
                assert!(matches!(
                    outcome.transition,
                    RootTransition::PushObject
                        | RootTransition::StayObject { .. }
                        | RootTransition::StayArray { .. }
                ));
                let canonical = Self::convert_path(&path);
                self.values
                    .insert_value(&canonical, Value::Object(BTreeMap::default()));
                Ok(BufferedEvent::ObjectBegin { path })
            }
            ParseEvent::ObjectEnd { path } => {
                #[cfg(any(fuzzing, debug_assertions))]
                assert!(matches!(outcome.transition, RootTransition::PopContainer));
                let canonical = Self::convert_path(&path);
                let value = match self.container_value(&canonical) {
                    Some(Value::Object(map)) => {
                        self.object_scratch = Some(
                            map.into_iter()
                                .map(|(key, value)| (key.as_bytes().to_vec(), value))
                                .collect(),
                        );
                        self.object_scratch.as_ref()
                    }
                    _ => {
                        self.object_scratch = None;
                        None
                    }
                };
                Ok(BufferedEvent::ObjectEnd { path, value })
            }
        };

        let _ = outcome.completes_array_slot;

        result
    }
}

impl RootedBufferAssembler<RawContext> for RawBufferAssembler
where
    <RawContext as PathCtx>::Path: crate::jsonmodem_buffers::PathRoot,
{
    #[inline]
    fn root(&self) -> &Value {
        self.read_root()
    }
}
