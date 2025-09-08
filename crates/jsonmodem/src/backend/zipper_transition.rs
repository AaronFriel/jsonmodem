use alloc::vec::Vec;
use core::fmt::Debug;

use rpds::Vector;

use crate::{context::ValueCtx, event::ParseEvent, path::PathItem};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootTransition<'a, K> {
    StartRootScalar,
    PushArray,
    PushObject,
    StayArray { index: usize },
    StayObject { key: &'a K },
    AppendString { is_final: bool },
    PopContainer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransitionOutcome<'a, K> {
    pub transition: RootTransition<'a, K>,
    pub completes_array_slot: bool,
}

#[derive(Debug, Default)]
pub struct ParserCursor {
    frames: Vec<FrameContext>,
    string_in_progress: bool,
    previous_depth: usize,
}

pub trait ParserPath<K> {
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool;
    fn last(&self) -> Option<&PathItem<K, usize>>;
}

impl<K> ParserPath<K> for Vec<PathItem<K, usize>> {
    fn len(&self) -> usize {
        Vec::len(self)
    }

    fn is_empty(&self) -> bool {
        Vec::is_empty(self)
    }

    fn last(&self) -> Option<&PathItem<K, usize>> {
        self.as_slice().last()
    }
}

impl<K> ParserPath<K> for Vector<PathItem<K, usize>> {
    fn len(&self) -> usize {
        self.len()
    }

    fn is_empty(&self) -> bool {
        self.is_empty()
    }

    fn last(&self) -> Option<&PathItem<K, usize>> {
        self.last()
    }
}

impl<K, T> ParserPath<K> for &T
where
    T: ParserPath<K> + ?Sized,
{
    fn len(&self) -> usize {
        (**self).len()
    }

    fn is_empty(&self) -> bool {
        (**self).is_empty()
    }

    fn last(&self) -> Option<&PathItem<K, usize>> {
        (**self).last()
    }
}

impl ParserCursor {
    pub fn new() -> Self {
        Self::default()
    }

    #[allow(clippy::too_many_lines)]
    pub fn classify_transition<'src, 'path, K, Backend, P>(
        &mut self,
        event: &'path ParseEvent<'src, &'path P, Backend>,
    ) -> TransitionOutcome<'path, K>
    where
        'src: 'path,
        K: 'path + Debug,
        Backend: ValueCtx,
        P: ParserPath<K> + Debug,
    {
        let path = event.path();
        let depth = path.len();
        #[cfg(any(debug_assertions, fuzzing))]
        {
            let delta = if depth >= self.previous_depth {
                isize::try_from(depth - self.previous_depth).unwrap_or(isize::MAX)
            } else {
                -isize::try_from(self.previous_depth - depth).unwrap_or(isize::MAX)
            };
            assert!(
                (-1..=1).contains(&delta),
                "parser path depth changed by {delta}; previous depth={}, current depth={depth}",
                self.previous_depth
            );
        }
        self.previous_depth = depth;

        match event {
            ParseEvent::Null { .. } | ParseEvent::Boolean { .. } | ParseEvent::Number { .. } => {
                self.scalar_transition(path)
            }
            ParseEvent::String {
                path,
                is_initial,
                is_final,
                ..
            } => {
                if *is_initial {
                    let outcome = self.scalar_transition(path);
                    self.string_in_progress = !*is_final;
                    TransitionOutcome {
                        completes_array_slot: outcome.completes_array_slot && *is_final,
                        ..outcome
                    }
                } else {
                    #[cfg(any(debug_assertions, fuzzing))]
                    assert!(
                        self.string_in_progress,
                        "received continued string fragment without initial transition"
                    );
                    self.string_in_progress = !*is_final;
                    TransitionOutcome {
                        transition: RootTransition::AppendString {
                            is_final: *is_final,
                        },
                        completes_array_slot: path
                            .last()
                            .is_some_and(|item| matches!(item, PathItem::Index(_)))
                            && *is_final,
                    }
                }
            }
            ParseEvent::ArrayBegin { path } => {
                self.string_in_progress = false;
                let completes_array_slot = false;
                let transition = if path.is_empty() {
                    RootTransition::PushArray
                } else {
                    match path
                        .last()
                        .expect("non-empty path must have a final component")
                    {
                        PathItem::Index(index) => RootTransition::StayArray { index: *index },
                        PathItem::Key(key) => RootTransition::StayObject { key },
                    }
                };

                self.frames.push(FrameContext {
                    kind: ContainerKind::Array,
                    parent_is_array: path
                        .last()
                        .is_some_and(|item| matches!(item, PathItem::Index(_))),
                });

                TransitionOutcome {
                    transition,
                    completes_array_slot,
                }
            }
            ParseEvent::ObjectBegin { path } => {
                self.string_in_progress = false;
                let completes_array_slot = false;
                let transition = if path.is_empty() {
                    RootTransition::PushObject
                } else {
                    match path
                        .last()
                        .expect("non-empty path must have a final component")
                    {
                        PathItem::Index(index) => RootTransition::StayArray { index: *index },
                        PathItem::Key(key) => RootTransition::StayObject { key },
                    }
                };

                self.frames.push(FrameContext {
                    kind: ContainerKind::Object,
                    parent_is_array: path
                        .last()
                        .is_some_and(|item| matches!(item, PathItem::Index(_))),
                });

                TransitionOutcome {
                    transition,
                    completes_array_slot,
                }
            }
            ParseEvent::ArrayEnd { .. } => self.finish_container(ContainerKind::Array),
            ParseEvent::ObjectEnd { .. } => self.finish_container(ContainerKind::Object),
        }
    }

    fn scalar_transition<'path, K, P>(&mut self, path: &'path P) -> TransitionOutcome<'path, K>
    where
        P: ParserPath<K>,
    {
        self.string_in_progress = false;
        if path.is_empty() {
            TransitionOutcome {
                transition: RootTransition::StartRootScalar,
                completes_array_slot: false,
            }
        } else {
            let completes_array_slot = path
                .last()
                .is_some_and(|item| matches!(item, PathItem::Index(_)));
            let transition = match path
                .last()
                .expect("non-empty path must have a final component")
            {
                PathItem::Index(index) => RootTransition::StayArray { index: *index },
                PathItem::Key(key) => RootTransition::StayObject { key },
            };
            TransitionOutcome {
                transition,
                completes_array_slot,
            }
        }
    }

    #[cfg_attr(not(any(debug_assertions, fuzzing)), expect(unused_variables))]
    fn finish_container<'path, K>(
        &mut self,
        expected: ContainerKind,
    ) -> TransitionOutcome<'path, K> {
        self.string_in_progress = false;
        let Some(frame) = self.frames.pop() else {
            #[cfg(any(debug_assertions, fuzzing))]
            unreachable!("parser emitted container end without matching begin");
            #[cfg(not(any(debug_assertions, fuzzing)))]
            return TransitionOutcome {
                transition: RootTransition::PopContainer,
                completes_array_slot: false,
            };
        };

        #[cfg(any(debug_assertions, fuzzing))]
        assert_eq!(
            frame.kind, expected,
            "expected container {:?} but observed {:?}",
            expected, frame.kind
        );

        TransitionOutcome {
            transition: RootTransition::PopContainer,
            completes_array_slot: frame.parent_is_array,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FrameContext {
    kind: ContainerKind,
    parent_is_array: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ContainerKind {
    Array,
    Object,
}
