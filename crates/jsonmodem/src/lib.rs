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
extern crate alloc;

#[cfg(any(test, fuzzing))]
extern crate std;

mod backend;
mod buffer_options;
mod context;
mod event;
mod jsonmodem_buffers;
mod jsonmodem_values;
pub mod lending_iterator;
mod parser;
mod path;
mod value;
mod value_tree;

#[doc(hidden)]
pub use backend::raw::RawBufferAssembler;
pub use backend::{RawContext, StdBackend};
pub use buffer_options::BufferOptions;
// Expose core parser types publicly for users building custom adapters, while
// keeping the low-level `JsonModem` constructor out of the docs surface.
pub use event::ParseEvent;
#[cfg(test)]
#[allow(unused_imports)]
pub use event::test_util;
pub use jsonmodem_buffers::{BufferedEvent, JsonModemBuffers};
pub use jsonmodem_values::{JsonModemValues, StreamingValue, ValuesOptions};
#[doc(hidden)]
pub use parser::JsonModem;
pub use parser::{DecodeMode, ParserError, ParserOptions};
pub use path::{Path, PathItem, PathItemFrom, PathLike};
pub use value::Value;

/// Adapter configuration helpers re-exported for convenience.
pub mod options {
    pub use crate::BufferOptions;
}

#[cfg(test)]
mod tests;

#[doc(hidden)]
pub use alloc::vec;

/// Macro to build a `Path` (a `Vec<PathItem>`) from a heterogeneous list of
/// keys and indices.
///
/// ```rust
/// use jsonmodem::{PathItem, path};
/// let p = path![0, "foo", 2];
/// assert_eq!(
///     p,
///     vec![
///         PathItem::Index(0),
///         PathItem::Key("foo".into()),
///         PathItem::Index(2)
///     ]
/// );
/// ```
#[macro_export]
macro_rules! path {
    ( $( $elem:expr ),* $(,)? ) => {{
        #[allow(unused_imports)]
        use $crate::PathItemFrom;
        $crate::vec![$($crate::PathItem::from_path_component($elem)),*] as $crate::Path
    }};
}
