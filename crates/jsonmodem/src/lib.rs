//! A faithful 1‑for‑1 Rust port of the incremental / online JSON parser
//! originally written in TypeScript (see source file in the prompt).

#![no_std]
#![allow(missing_docs)]
extern crate alloc;

#[cfg(test)]
extern crate std;

mod buffer;
mod escape_buffer;
mod event;
mod literal_buffer;
mod value;
mod value_zipper;

mod error;
mod event_stack;
mod options;
mod parser;

#[cfg(test)]
mod tests;

pub use error::ParserError;
pub use event::{ParseEvent, PathComponent, PathComponentFrom};
pub use options::{ParserOptions, StringValueMode};
pub use parser::StreamingParser;
pub use value::{Array, Map, Value};

#[doc(hidden)]
pub use alloc::vec;

/// Macro to build a `Vec<PathComponent>` from a heterogeneous list of keys and
/// indices.
///
/// ```rust
/// extern crate alloc;
/// # use jsonmodem::{path, PathComponent};
/// let p = path![0, "foo", 2];
/// assert_eq!(
///     p,
///     vec![
///         PathComponent::Index(0),
///         PathComponent::Key("foo".into()),
///         PathComponent::Index(2)
///     ]
/// );
/// ```
#[macro_export]
macro_rules! path {
    ( $( $elem:expr ),* $(,)? ) => {{
        use $crate::PathComponentFrom;
        $crate::vec![$($crate::PathComponent::from_path_component($elem)),*]
    }};
}
