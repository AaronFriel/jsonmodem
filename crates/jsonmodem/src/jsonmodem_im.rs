//! Immutable snapshot adapter built on top of [`ImBackend`].
//!
//! Enable the `im` Cargo feature to opt into immutable snapshots that reuse
//! persistent data structures (`EcoString`, `rpds`) instead of cloning `String`
//! or `Vec` values. `JsonModemIm` wraps [`JsonModemValues`] and records each
//! completed root so callers can iterate over [`StreamingValue`] instances
//! without juggling lifetimes. This is ideal when you want deterministic
//! snapshots (e.g. logging, caching, or diffing) while still feeding the parser
//! in streaming chunks.
//!
//! The immutable backend differs from [`JsonModemValues`] in one key way: the
//! iterators returned by [`feed`](JsonModemIm::feed) and
//! [`finish`](JsonModemIm::finish) yield owned values, not borrowed snapshots.
//! Because the underlying data structures are persistent, cloning a completed
//! value is cheap, which allows us to expose a standard iterator API that
//! callers can move across threads or stash for later use.
//!
//! ```rust
//! # #[cfg(feature = "im")]
//! # {
//! use jsonmodem::{JsonModemIm, ParserOptions, ValuesOptions};
//!
//! let mut snapshots = JsonModemIm::with_options(
//!     ParserOptions::default(),
//!     ValuesOptions::default().with_partial(true),
//! );
//!
//! for view in snapshots.feed("{\"title\":\"he") {
//!     println!("partial: {}", view.value);
//! }
//! for view in snapshots.feed("llo\",\"active\":true}") {
//!     println!("partial: {}", view.value);
//! }
//! for view in snapshots.finish() {
//!     if view.is_final {
//!         println!("final: {}", view.value);
//!     }
//! }
//! # }
//! ```
//!
//! The adapter shares the same parser and buffering machinery as the standard
//! backend, so benchmarks and `QuickCheck` suites that exercise
//! `JsonModemValues` automatically cover the immutable path as well.

use crate::{
    ImBackend, ImValueAssembler, JsonModemValues, ParserOptions, StreamingValue, ValuesOptions,
    buffer_options::BufferOptions,
    im_value,
    jsonmodem_values::{JsonModemValuesClosed, JsonModemValuesIter, ValuesError},
};

/// Immutable snapshot adapter that exposes persistent JSON roots via standard
/// iterators.
///
/// `JsonModemIm` wraps [`JsonModemValues`] configured with [`ImBackend`]. Each
/// call to [`feed`](Self::feed) or [`finish`](Self::finish) clones any
/// completed values into persistent structures so callers can hold or move them
/// without borrow checker constraints.
pub struct JsonModemIm {
    values: Option<JsonModemValues<ImBackend, ImValueAssembler>>,
}

impl Default for JsonModemIm {
    fn default() -> Self {
        Self::new()
    }
}

impl JsonModemIm {
    /// Creates a snapshot adapter with default parser and buffering options.
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(
            ParserOptions::default(),
            ValuesOptions::default(),
            BufferOptions::default(),
        )
    }

    /// Builds a snapshot adapter with explicit parser and values configuration.
    #[must_use]
    pub fn with_options(parser: ParserOptions, values: ValuesOptions) -> Self {
        Self::with_config(parser, values, BufferOptions::default())
    }

    /// Builds a snapshot adapter with explicit parser, values, and buffering
    /// options.
    #[must_use]
    pub fn with_config(
        parser: ParserOptions,
        values: ValuesOptions,
        buffers: BufferOptions,
    ) -> Self {
        let assembler = ImValueAssembler::new(buffers);
        let parser = parser.with_allow_multiple_json_values(true);
        let inner = JsonModemValues::with_buffer_builder(parser, values, assembler);
        Self {
            values: Some(inner),
        }
    }

    /// Returns `true` once [`finish`](Self::finish) has been called.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.values.is_none()
    }

    /// Feeds a chunk of JSON and returns an iterator over the newly produced
    /// snapshots.
    ///
    /// # Panics
    ///
    /// Panics if [`finish`](Self::finish) has already been invoked.
    #[must_use]
    pub fn feed<'a>(&'a mut self, chunk: &'a str) -> JsonModemImIter<'a> {
        let values = self
            .values
            .as_mut()
            .expect("JsonModemIm::feed called after finish");
        JsonModemImIter {
            inner: values.feed(chunk),
        }
    }

    /// Completes parsing and yields any remaining snapshots.
    ///
    /// # Panics
    ///
    /// Panics if called more than once.
    #[must_use]
    pub fn finish(&mut self) -> JsonModemImClosed {
        let values = self
            .values
            .take()
            .expect("JsonModemIm::finish called more than once");
        JsonModemImClosed {
            inner: values.finish(),
        }
    }
}

/// Iterator over immutable snapshots produced by [`JsonModemIm`].
pub struct JsonModemImIter<'a> {
    inner: JsonModemValuesIter<'a, ImBackend, ImValueAssembler>,
}

impl Iterator for JsonModemImIter<'_> {
    type Item = StreamingValue<im_value::Value>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner
            .next()
            .map(|result| result.expect("JsonModemIm iterator error"))
    }
}

impl<'a> JsonModemImIter<'a> {
    /// Returns an iterator yielding [`Result`] if the caller prefers explicit
    /// error handling.
    #[must_use]
    pub fn into_results(self) -> JsonModemImResultIter<'a> {
        JsonModemImResultIter { inner: self.inner }
    }
}

/// Iterator yielding [`Result`] values for immutable snapshots.
pub struct JsonModemImResultIter<'a> {
    inner: JsonModemValuesIter<'a, ImBackend, ImValueAssembler>,
}

impl Iterator for JsonModemImResultIter<'_> {
    type Item = Result<StreamingValue<im_value::Value>, ValuesError<ImBackend>>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}
/// Iterator returned after [`JsonModemIm::finish`].
pub struct JsonModemImClosed {
    inner: JsonModemValuesClosed<ImBackend, ImValueAssembler>,
}

impl Iterator for JsonModemImClosed {
    type Item = StreamingValue<im_value::Value>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner
            .next()
            .map(|result| result.expect("JsonModemIm finish iterator error"))
    }
}

impl JsonModemImClosed {
    /// Returns an iterator yielding [`Result`] values for callers that prefer
    /// explicit error handling.
    #[must_use]
    pub fn into_results(self) -> JsonModemImClosedResultIter {
        JsonModemImClosedResultIter { inner: self.inner }
    }
}

/// Result iterator returned after [`JsonModemIm::finish`].
pub struct JsonModemImClosedResultIter {
    inner: JsonModemValuesClosed<ImBackend, ImValueAssembler>,
}

impl Iterator for JsonModemImClosedResultIter {
    type Item = Result<StreamingValue<im_value::Value>, ValuesError<ImBackend>>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}
