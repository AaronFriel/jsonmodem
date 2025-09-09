use alloc::vec::Vec;
use core::{cmp::Ordering, fmt::Debug};

use crate::{context::ValueCtx, event::ParseEvent, path::PathItem};

#[derive(Default, Debug)]
pub(crate) struct TransitionAsserter {
    previous: Option<ObservedEvent>,
    string_in_progress: Option<Vec<PathSlot>>,
}

impl TransitionAsserter {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn observe<K: Debug, Backend: ValueCtx>(
        &mut self,
        event: &ParseEvent<'_, &'_ Vec<PathItem<K, usize>>, Backend>,
    ) {
        let (path, kind) = match event {
            ParseEvent::Null { path }
            | ParseEvent::Boolean { path, .. }
            | ParseEvent::Number { path, .. } => (path.as_slice(), EventKind::Scalar),
            ParseEvent::String {
                path,
                is_initial,
                is_final,
                ..
            } => (
                path.as_slice(),
                EventKind::String {
                    is_initial: *is_initial,
                    is_final: *is_final,
                },
            ),
            ParseEvent::ArrayBegin { path } => (path.as_slice(), EventKind::ArrayBegin),
            ParseEvent::ArrayEnd { path } => (path.as_slice(), EventKind::ArrayEnd),
            ParseEvent::ObjectBegin { path } => (path.as_slice(), EventKind::ObjectBegin),
            ParseEvent::ObjectEnd { path } => (path.as_slice(), EventKind::ObjectEnd),
        };

        self.observe_path(path, kind);
    }

    fn observe_path<K>(&mut self, path: &[PathItem<K, usize>], kind: EventKind) {
        let slots = path
            .iter()
            .map(|component| match component {
                PathItem::Key(_) => PathSlot::Key,
                PathItem::Index(index) => PathSlot::Index(*index),
            })
            .collect::<Vec<_>>();

        self.observe_slots(slots, kind);
    }

    fn observe_slots(&mut self, path: Vec<PathSlot>, kind: EventKind) {
        if let Some(active) = &self.string_in_progress {
            match kind {
                EventKind::String { is_initial, .. } => {
                    #[cfg(any(fuzzing, debug_assertions))]
                    assert!(
                        active == &path,
                        "string fragment path changed while buffering: {active:?} -> {path:?}"
                    );
                    #[cfg(any(fuzzing, debug_assertions))]
                    assert!(
                        !is_initial,
                        "continued string fragment unexpectedly marked as initial",
                    );
                }
                _ => {
                    #[cfg(any(fuzzing, debug_assertions))]
                    unreachable!("non-string event {kind:?} while buffering string at {active:?}");
                }
            }
        }

        if let Some(previous) = &self.previous {
            self.validate_transition(previous, &path, kind);
        }

        self.update_string_state(&path, kind);

        self.previous = Some(ObservedEvent { path, kind });
    }

    #[allow(clippy::unused_self)]
    fn validate_transition(&self, previous: &ObservedEvent, path: &[PathSlot], kind: EventKind) {
        let prev_depth = previous.path.len();
        let depth = path.len();
        let delta = if depth >= prev_depth {
            isize::try_from(depth - prev_depth).unwrap_or(isize::MAX)
        } else {
            -isize::try_from(prev_depth - depth).unwrap_or(isize::MAX)
        };

        #[cfg(any(fuzzing, debug_assertions))]
        assert!(
            (-1..=1).contains(&delta),
            "parser path depth changed by {delta}; previous={:?}, next={:?}",
            previous.path,
            path
        );

        match delta.cmp(&0) {
            Ordering::Greater => {
                #[cfg(any(fuzzing, debug_assertions))]
                assert_eq!(delta, 1, "parser depth advanced by more than one: {delta}");
                let is_new_root = prev_depth == 0;
                let follows_container_boundary = matches!(
                    previous.kind,
                    EventKind::ArrayBegin
                        | EventKind::ObjectBegin
                        | EventKind::ArrayEnd
                        | EventKind::ObjectEnd
                );
                #[cfg(any(fuzzing, debug_assertions))]
                assert!(
                    follows_container_boundary || is_new_root,
                    "depth +1 transition must follow a container boundary: prev={:?}, next_kind={:?}",
                    previous.kind,
                    kind
                );
                #[cfg(any(fuzzing, debug_assertions))]
                assert!(
                    prefix_matches(&previous.path, path),
                    "depth +1 transition must extend previous path: prev={:?}, next={:?}",
                    previous.path,
                    path
                );
            }
            Ordering::Less => {
                #[cfg(any(fuzzing, debug_assertions))]
                assert_eq!(
                    delta, -1,
                    "parser depth decreased by more than one: {delta}"
                );
                #[cfg(any(fuzzing, debug_assertions))]
                assert!(
                    matches!(kind, EventKind::ArrayEnd | EventKind::ObjectEnd),
                    "depth -1 transition must be a container end: prev={:?}, next_kind={:?}",
                    previous.kind,
                    kind
                );
                #[cfg(any(fuzzing, debug_assertions))]
                assert!(
                    prefix_matches(path, &previous.path),
                    "depth -1 transition must trim the previous path: prev={:?}, next={:?}",
                    previous.path,
                    path
                );
            }
            Ordering::Equal => self.validate_same_depth(previous, path, kind),
        }
    }

    #[allow(clippy::unused_self)]
    fn validate_same_depth(&self, previous: &ObservedEvent, path: &[PathSlot], kind: EventKind) {
        if path.is_empty() || previous.path.is_empty() {
            return;
        }

        let depth = path.len();
        if previous.path.len() != depth {
            return;
        }

        if previous.path[..depth - 1] != path[..depth - 1] {
            return;
        }

        if let (PathSlot::Index(prev_index), PathSlot::Index(next_index)) =
            (previous.path[depth - 1], path[depth - 1])
        {
            if prev_index == next_index {
                if previous.path == path {
                    let allowed = matches!(
                        (previous.kind, kind),
                        (EventKind::ArrayBegin, EventKind::ArrayEnd)
                            | (EventKind::ObjectBegin, EventKind::ObjectEnd)
                            | (
                                EventKind::String {
                                    is_final: false,
                                    ..
                                },
                                EventKind::String { .. }
                            )
                    );
                    #[cfg(any(fuzzing, debug_assertions))]
                    assert!(
                        allowed,
                        "array slot reused without an in-progress string: prev_kind={:?}, next_kind={:?}",
                        previous.kind, kind
                    );
                }
            } else {
                #[cfg(any(fuzzing, debug_assertions))]
                assert_eq!(
                    next_index,
                    prev_index + 1,
                    "array indices must advance monotonically: {prev_index} -> {next_index}"
                );
            }
        }
    }

    fn update_string_state(&mut self, path: &[PathSlot], kind: EventKind) {
        match kind {
            EventKind::String {
                is_initial,
                is_final,
            } => {
                if self.string_in_progress.is_none() {
                    #[cfg(any(fuzzing, debug_assertions))]
                    assert!(
                        is_initial,
                        "string fragment missing initial flag at path {path:?}"
                    );
                }

                if is_final {
                    self.string_in_progress = None;
                } else {
                    self.string_in_progress = Some(path.to_vec());
                }
            }
            _ => {
                #[cfg(any(fuzzing, debug_assertions))]
                assert!(
                    self.string_in_progress.is_none(),
                    "non-string event {:?} while buffering string at {:?}",
                    kind,
                    self.string_in_progress
                );
            }
        }
    }
}

#[derive(Clone, Debug)]
struct ObservedEvent {
    path: Vec<PathSlot>,
    kind: EventKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PathSlot {
    Key,
    Index(usize),
}

#[derive(Clone, Copy, Debug)]
enum EventKind {
    ArrayBegin,
    ArrayEnd,
    ObjectBegin,
    ObjectEnd,
    String { is_initial: bool, is_final: bool },
    Scalar,
}

fn prefix_matches(shorter: &[PathSlot], longer: &[PathSlot]) -> bool {
    shorter.len() <= longer.len() && shorter == &longer[..shorter.len()]
}
