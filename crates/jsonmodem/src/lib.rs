//! A faithful 1‑for‑1 Rust port of the incremental / online JSON parser
//! originally written in TypeScript (see source file in the prompt).

#![no_std]
#![expect(missing_docs)]
extern crate alloc;

#[cfg(test)]
extern crate std;

mod buffer;
mod escape_buffer;
mod event;
mod factory;
mod literal_buffer;
mod value;
mod value_zipper;

mod chunk_utils;
mod error;
mod event_stack;
mod options;
mod parser;
mod streaming_values;

#[cfg(test)]
mod tests;

#[doc(hidden)]
pub use alloc::vec;

pub use chunk_utils::{produce_chunks, produce_prefixes};
pub use error::ParserError;
pub use event::{ParseEvent, PathComponent, PathComponentFrom};
pub use factory::{JsonValue, JsonValueFactory, StdValueFactory, ValueKind};
pub use options::{NonScalarValueMode, ParserOptions, StringValueMode};
pub use parser::{StreamingParser, StreamingParserImpl};
pub use streaming_values::{StreamingValue, StreamingValuesParser};
pub use value::{Array, Map, Value};

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
