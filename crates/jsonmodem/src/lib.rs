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

mod json_path;
mod path;
mod path_component;
#[cfg(test)]
mod tests;

#[doc(hidden)]
pub use alloc::vec;

pub use chunk_utils::{produce_chunks, produce_prefixes};
pub use error::ParserError;
pub use event::ParseEvent;
pub use factory::{JsonValue, JsonValueFactory, StdValueFactory, ValueKind};
pub use json_path::JsonPath;
pub use options::{NonScalarValueMode, ParserOptions, StringValueMode};
pub use parser::{DefaultStreamingParser, StreamingParserImpl};
pub use path::Path;
pub use path_component::{Index, Key, PathComponent, PathComponentFrom};
pub use streaming_values::{StreamingValue, StreamingValuesParser};
pub use value::{Array, Map, Str, Value};

/// Macro to build a `Vec<PathComponent>` from a heterogeneous list of keys and
/// indices.
///
/// ```rust
/// extern crate alloc;
/// use std::ops::Deref;
///
/// use jsonmodem::{PathComponent, path};
/// let p = path![0, "foo", 2];
/// assert_eq!(
///     p.deref(),
///     &vec![
///         PathComponent::Index(0),
///         PathComponent::Key("foo".into()),
///         PathComponent::Index(2)
///     ]
/// );
/// ```
#[macro_export]
macro_rules! path {
    ( $( $elem:expr ),* $(,)? ) => {{
        #[allow(unused_imports)]
        use $crate::PathComponentFrom;
        $crate::vec![$($crate::PathComponent::from_path_component($elem)),*]
    }};
}
