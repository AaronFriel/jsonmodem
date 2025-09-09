#![allow(
    clippy::needless_borrow,
    clippy::redundant_closure_for_method_calls,
    clippy::needless_match,
    dead_code
)]

use alloc::{
    borrow::{Cow, ToOwned},
    collections::BTreeMap,
    vec::Vec,
};

use super::{StdBackend, StdPath, value::Value, value_zipper::ValueZipper};
#[cfg(any(fuzzing, debug_assertions))]
use crate::backend::TransitionAsserter;
use crate::{
    backend::{ParserCursor, RootTransition},
    buffer_options::BufferOptions,
    event::ParseEvent,
};

pub enum AppliedRef<'a> {
    Scalar {
        path: &'a StdPath,
        leaf: &'a Value,
    },
    String {
        path: &'a StdPath,
        leaf: &'a Value,
        fragment: Cow<'a, str>,
        is_initial: bool,
        is_final: bool,
        buffered: Option<&'a str>,
    },
    ArrayBegin {
        path: &'a StdPath,
        leaf: &'a Value,
    },
    ArrayEnd {
        path: &'a StdPath,
        leaf: &'a Value,
        root_completed: bool,
    },
    ObjectBegin {
        path: &'a StdPath,
        leaf: &'a Value,
    },
    ObjectEnd {
        path: &'a StdPath,
        leaf: &'a Value,
        root_completed: bool,
    },
    Nothing,
}

#[derive(Debug)]
pub struct ValueApplicator {
    zipper: ValueZipper,
    options: BufferOptions,
    cursor: ParserCursor,
    #[cfg(any(fuzzing, debug_assertions))]
    transitions: TransitionAsserter,
}

impl ValueApplicator {
    #[inline]
    pub fn new(options: BufferOptions) -> Self {
        Self {
            zipper: ValueZipper::new(),
            options,
            cursor: ParserCursor::new(),
            #[cfg(any(fuzzing, debug_assertions))]
            transitions: TransitionAsserter::new(),
        }
    }

    #[inline]
    pub fn push<'a, 'src>(
        &'a mut self,
        event: ParseEvent<'src, &'a StdPath, StdBackend>,
    ) -> AppliedRef<'a>
    where
        'src: 'a,
    {
        #[cfg(any(fuzzing, debug_assertions))]
        self.transitions.observe(&event);

        let outcome = self.cursor.classify_transition(&event);

        let applied = match event {
            ParseEvent::Null { path } => self.apply_scalar(path, Value::Null),
            ParseEvent::Boolean { path, value } => self.apply_scalar(path, Value::Boolean(value)),
            ParseEvent::Number { path, value } => self.apply_scalar(path, Value::Number(value)),
            ParseEvent::String {
                path,
                fragment,
                is_initial,
                is_final,
            } => {
                let fragment = match fragment {
                    Cow::Borrowed(text) => Cow::Borrowed(text),
                    Cow::Owned(text) => Cow::Owned(text),
                };
                match outcome.transition {
                    RootTransition::AppendString { .. }
                    | RootTransition::StartRootScalar
                    | RootTransition::StayArray { .. }
                    | RootTransition::StayObject { .. } => {}
                    RootTransition::PushArray
                    | RootTransition::PushObject
                    | RootTransition::PopContainer => {
                        #[cfg(any(fuzzing, debug_assertions))]
                        unreachable!("unexpected transition for string event");
                    }
                }
                self.apply_string(path, fragment, is_initial, is_final)
            }
            ParseEvent::ArrayBegin { path } => {
                #[cfg(any(fuzzing, debug_assertions))]
                assert!(matches!(
                    outcome.transition,
                    RootTransition::PushArray
                        | RootTransition::StayArray { .. }
                        | RootTransition::StayObject { .. }
                ));
                self.apply_container_begin(path, ContainerKind::Array)
            }
            ParseEvent::ArrayEnd { path } => {
                #[cfg(any(fuzzing, debug_assertions))]
                assert!(matches!(outcome.transition, RootTransition::PopContainer));
                self.apply_container_end(path, ContainerKind::Array)
            }
            ParseEvent::ObjectBegin { path } => {
                #[cfg(any(fuzzing, debug_assertions))]
                assert!(matches!(
                    outcome.transition,
                    RootTransition::PushObject
                        | RootTransition::StayObject { .. }
                        | RootTransition::StayArray { .. }
                ));
                self.apply_container_begin(path, ContainerKind::Object)
            }
            ParseEvent::ObjectEnd { path } => {
                #[cfg(any(fuzzing, debug_assertions))]
                assert!(matches!(outcome.transition, RootTransition::PopContainer));
                self.apply_container_end(path, ContainerKind::Object)
            }
        };

        let _ = outcome.completes_array_slot;

        applied
    }

    #[inline]
    pub fn read_root(&self) -> &Value {
        self.zipper.read_root()
    }

    #[inline]
    pub fn take_root(&mut self) -> Value {
        self.zipper.take_root()
    }

    #[inline]
    pub fn options(&self) -> BufferOptions {
        self.options
    }

    #[inline]
    fn apply_scalar<'a>(&'a mut self, path: &StdPath, value: Value) -> AppliedRef<'a> {
        let (path, leaf) = self.zipper.with_leaf_mut(path, |slot| *slot = value);
        AppliedRef::Scalar { path, leaf }
    }

    #[inline]
    fn apply_string<'a>(
        &'a mut self,
        path: &StdPath,
        fragment: Cow<'a, str>,
        is_initial: bool,
        is_final: bool,
    ) -> AppliedRef<'a> {
        let fragment_ref = fragment.as_ref();

        let (path, leaf) = self.zipper.with_leaf_mut(path, |slot| match slot {
            Value::String(existing) => {
                if is_initial {
                    existing.clear();
                }
                existing.push_str(fragment_ref);
            }
            _ => {
                *slot = Value::String(fragment_ref.to_owned());
            }
        });

        let buffered = None;

        AppliedRef::String {
            path,
            leaf,
            fragment,
            is_initial,
            is_final,
            buffered,
        }
    }

    #[inline]
    fn apply_container_begin<'a>(
        &'a mut self,
        path: &StdPath,
        kind: ContainerKind,
    ) -> AppliedRef<'a> {
        let (path, leaf) = self.zipper.with_leaf_mut(path, |slot| {
            *slot = match kind {
                ContainerKind::Array => Value::Array(Vec::new()),
                ContainerKind::Object => Value::Object(BTreeMap::default()),
            };
        });

        match kind {
            ContainerKind::Array => AppliedRef::ArrayBegin { path, leaf },
            ContainerKind::Object => AppliedRef::ObjectBegin { path, leaf },
        }
    }

    #[inline]
    fn apply_container_end<'a>(
        &'a mut self,
        path: &StdPath,
        kind: ContainerKind,
    ) -> AppliedRef<'a> {
        let (path, leaf) = self.zipper.with_leaf(path);
        let root_completed = path.is_empty();

        match kind {
            ContainerKind::Array => AppliedRef::ArrayEnd {
                path,
                leaf,
                root_completed,
            },
            ContainerKind::Object => AppliedRef::ObjectEnd {
                path,
                leaf,
                root_completed,
            },
        }
    }
}

#[derive(Clone, Copy)]
enum ContainerKind {
    Array,
    Object,
}
