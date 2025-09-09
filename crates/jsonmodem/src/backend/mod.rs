pub mod raw;
mod std;

#[cfg(any(fuzzing, debug_assertions))]
mod transition_debug;
#[cfg(any(fuzzing, debug_assertions))]
pub(crate) use transition_debug::TransitionAsserter;

#[cfg(not(any(fuzzing, debug_assertions)))]
mod transition_debug {
    use alloc::vec::Vec;
    use core::fmt::Debug;

    use crate::{context::ValueCtx, event::ParseEvent, path::PathItem};

    #[allow(dead_code)]
    #[derive(Default, Debug)]
    pub(crate) struct TransitionAsserter;

    #[allow(dead_code)]
    impl TransitionAsserter {
        pub(crate) fn new() -> Self {
            Self
        }

        pub(crate) fn observe<K: Debug, Backend: ValueCtx>(
            &mut self,
            _event: &ParseEvent<'_, &'_ Vec<PathItem<K, usize>>, Backend>,
        ) {
        }
    }
}

mod zipper_transition;

pub use raw::RawContext;
pub(crate) use zipper_transition::{ParserCursor, RootTransition};

#[allow(unused_imports)]
pub use self::std::{
    StdBackend, StdBufferAssembler, StdPath, StdStringAssembler, StdValueAssembler, value,
    value_applicator, value_tree,
};
#[allow(unused_imports)]
pub use crate::context::{
    BuilderCtx, EventCtx, OwnedEventCtx, PathCtx, PathError, PathKind, ValueCtx,
};
