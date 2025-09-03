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

#[cfg(test)]
extern crate std;

mod backend;
#[cfg(feature = "todo")]
mod jsonmodem_buffers;
mod parser;

pub use parser::{Path, PathItem, PathItemFrom, StreamingParserImpl, DefaultStreamingParser};

#[cfg(feature = "todo")]
mod jsonmodem;

#[cfg(feature = "todo")]
pub use jsonmodem::JsonModem;

#[cfg(feature = "todo")]
mod buffered;

#[cfg(feature = "todo")]
pub use buffered::Value;

#[cfg(test)]
mod tests;

#[doc(hidden)]
pub use alloc::vec;

/// Macro to build a `Vec<PathComponent>` from a heterogeneous list of keys and
/// indices.
///
/// ```rust
/// extern crate alloc;
/// use std::ops::Deref;
///
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
