//! Streaming JSON parsing with a lean event core and small adapters.
//!
//! Layers:
//! - `JsonModem`: minimal, low‑overhead event parser. Emits fragment‑only
//!   strings and never builds composite values.
//! - `JsonModemBuffers`: adapter that coalesces string fragments per path and
//!   can attach either the full value (on final) or a growing prefix.
//! - `JsonModemValues`: adapter that incrementally builds low‑overhead partial
//!   values and yields them via an iterator.
//!
//! Most users only need these three types plus `ParseEvent`, `Path`, and
//! `Value`.

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
mod jsonmodem;
mod jsonmodem_buffers;
mod jsonmodem_values;
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
// New core and adapters
pub use jsonmodem::{JsonModem, JsonModemClosed, JsonModemIter};
pub use jsonmodem_buffers::{
    BufferOptions, BufferStringMode, BufferedEvent, JsonModemBuffers, JsonModemBuffersIter,
};
pub use jsonmodem_values::{JsonModemValues, StreamingValue as JsonModemStreamingValue};
pub use options::{NonScalarValueMode, ParserOptions};
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
