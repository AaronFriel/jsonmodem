#![allow(
    clippy::needless_borrow,
    clippy::redundant_closure_for_method_calls,
    clippy::needless_match,
    dead_code
)]

use super::{
    ImBackend, ImPath,
    value::{Array, Map, Str, Value},
    value_zipper::ValueZipper,
};
#[cfg(debug_assertions)]
use crate::backend::TransitionAsserter;
use crate::{
    backend::{ParserCursor, RootTransition},
    buffer_options::BufferOptions,
    event::ParseEvent,
};

pub enum AppliedRef<'a> {
    Scalar {
        path: &'a ImPath,
        leaf: &'a Value,
    },
    String {
        path: &'a ImPath,
        leaf: &'a Value,
        fragment: Str,
        is_initial: bool,
        is_final: bool,
        buffered: Option<Str>,
    },
    ArrayBegin {
        path: &'a ImPath,
        leaf: &'a Value,
    },
    ArrayEnd {
        path: &'a ImPath,
        leaf: &'a Value,
        root_completed: bool,
    },
    ObjectBegin {
        path: &'a ImPath,
        leaf: &'a Value,
    },
    ObjectEnd {
        path: &'a ImPath,
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
    #[cfg(debug_assertions)]
    transitions: TransitionAsserter,
}

impl ValueApplicator {
    pub fn new(options: BufferOptions) -> Self {
        Self {
            zipper: ValueZipper::new(),
            options,
            cursor: ParserCursor::new(),
            #[cfg(debug_assertions)]
            transitions: TransitionAsserter::new(),
        }
    }

    pub fn push<'a, 'src>(
        &'a mut self,
        event: &ParseEvent<'src, &'a ImPath, ImBackend>,
    ) -> AppliedRef<'a>
    where
        'src: 'a,
    {
        #[cfg(debug_assertions)]
        self.transitions.observe(event);

        let outcome = self.cursor.classify_transition(event);

        let applied = match event {
            ParseEvent::Null { path } => {
                let path = *path;
                self.apply_scalar(path, Value::Null)
            }
            ParseEvent::Boolean { path, value } => {
                let path = *path;
                self.apply_scalar(path, Value::Boolean(*value))
            }
            ParseEvent::Number { path, value } => {
                let path = *path;
                self.apply_scalar(path, Value::Number(*value))
            }
            ParseEvent::String {
                path,
                fragment,
                is_initial,
                is_final,
            } => {
                let path = *path;
                match outcome.transition {
                    RootTransition::AppendString { .. }
                    | RootTransition::StartRootScalar
                    | RootTransition::StayArray { .. }
                    | RootTransition::StayObject { .. } => {}
                    RootTransition::PushArray
                    | RootTransition::PushObject
                    | RootTransition::PopContainer => {
                        debug_assert!(false, "unexpected transition for string event");
                    }
                }
                self.apply_string(path, fragment.clone(), *is_initial, *is_final)
            }
            ParseEvent::ArrayBegin { path } => {
                let path = *path;
                self.apply_container_begin(path, ContainerKind::Array)
            }
            ParseEvent::ArrayEnd { path } => {
                let path = *path;
                self.apply_container_end(path, ContainerKind::Array)
            }
            ParseEvent::ObjectBegin { path } => {
                let path = *path;
                self.apply_container_begin(path, ContainerKind::Object)
            }
            ParseEvent::ObjectEnd { path } => {
                let path = *path;
                self.apply_container_end(path, ContainerKind::Object)
            }
        };

        let _ = outcome.completes_array_slot;

        applied
    }

    pub fn read_root(&self) -> &Value {
        self.zipper.read_root()
    }

    pub fn take_root(&mut self) -> Value {
        self.zipper.take_root()
    }

    pub fn options(&self) -> BufferOptions {
        self.options
    }

    fn apply_scalar<'a>(&'a mut self, path: &'a ImPath, value: Value) -> AppliedRef<'a> {
        let (path, leaf) = self.zipper.with_leaf_mut(path, |slot| *slot = value);
        AppliedRef::Scalar { path, leaf }
    }

    fn apply_string<'a>(
        &'a mut self,
        path: &'a ImPath,
        fragment: Str,
        is_initial: bool,
        is_final: bool,
    ) -> AppliedRef<'a> {
        let (path, leaf) = self.zipper.with_leaf_mut(path, |slot| match slot {
            Value::String(existing) if !is_initial => {
                existing.push_str(fragment.as_ref());
            }
            _ => {
                *slot = Value::String(fragment.clone());
            }
        });

        AppliedRef::String {
            path,
            leaf,
            fragment,
            is_initial,
            is_final,
            buffered: None,
        }
    }

    fn apply_container_begin<'a>(
        &'a mut self,
        path: &'a ImPath,
        kind: ContainerKind,
    ) -> AppliedRef<'a> {
        let (path, leaf) = self.zipper.with_leaf_mut(path, |slot| {
            *slot = match kind {
                ContainerKind::Array => Value::Array(Array::new_sync()),
                ContainerKind::Object => Value::Object(Map::new_sync()),
            };
        });

        match kind {
            ContainerKind::Array => AppliedRef::ArrayBegin { path, leaf },
            ContainerKind::Object => AppliedRef::ObjectBegin { path, leaf },
        }
    }

    fn apply_container_end<'a>(
        &'a mut self,
        path: &'a ImPath,
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
