//! ByteBuffer (design and implementation guide)
//!
//! Overview
//! - Replace the current char-based ring (`VecDeque<char>`) with a byte-based
//!   ring (`VecDeque<u8>`) to better align with raw-mode string handling and to
//!   avoid char↔byte round-trips on hot paths.
//! - This module documents the intended API, invariants, and tests for a
//!   `ByteBuffer` type and related helpers. It is a design-and-implementation
//!   guide; actual code can be authored following this blueprint.
//!
//! Why a byte ring?
//! - JSON input is always valid UTF-8; unread batch tails that we push back
//!   into the ring are UTF-8 substrings. Keeping the ring as bytes lets us:
//!   - Decode chars on demand without allocating intermediate `String`s.
//!   - Append decoded UTF-8 bytes directly to a raw buffer when the parser is
//!     in surrogate-preserving mode (WTF-8 output), without re-encoding from
//!     `char` to bytes.
//!   - Maintain a single internal representation for unread input that matches
//!     how batch slices are addressed (by bytes).
//!
//! Non-goals of the ring
//! - The ring does not and should not hold invalid UTF-8; raw-mode only affects
//!   how string payloads are materialized (owned bytes), not the encoding of
//!   unread input. The ring stores only valid UTF-8 bytes.
//! - The ring does not track `pos/line/column`; that is responsibility of the
//!   caller (e.g., `InputSession`) when calling `advance()`.
//!
//! Core type and API (to implement)
//! ```rust
//! use alloc::collections::VecDeque;
//!
//! pub(crate) struct ByteBuffer {
//!     data: VecDeque<u8>,
//! }
//!
//! impl ByteBuffer {
//!     // Construction
//!     pub fn new() -> Self;
//!     pub fn with_capacity(bytes: usize) -> Self; // optional convenience
//!
//!     // Appending unread input back to the ring
//!     // - `push_str` is the standard API: input is known-valid UTF-8.
//!     // - `push_bytes` is allowed only for known-valid UTF-8 and should be
//!     //   guarded with debug assertions or an infallible `unsafe` internal
//!     //   helper used from safe wrappers that validated via `str::from_utf8`.
//!     pub fn push_str(&mut self, s: &str);
//!     pub fn push_bytes(&mut self, bytes: &[u8]);
//!
//!     // Peeking and consuming
//!     // - `peek_char` decodes but does not consume the next UTF-8 scalar.
//!     // - `next_char` decodes and removes the next UTF-8 scalar.
//!     pub fn peek_char(&self) -> Option<char>;
//!     pub fn next_char(&mut self) -> Option<char>;
//!
//!     // Fast-path copying from ring into owned accumulators
//!     // - For UTF-8 text accumulation: decode chars and push into `String`.
//!     // - For raw accumulation: decode chars and append their UTF-8 bytes
//!     //   into `Vec<u8>` (equivalent to encoding the char and extending).
//!     // The predicate is evaluated on decoded chars; iteration stops when it
//!     // returns false or when the ring drains.
//!     pub fn copy_utf8_while_to_string<F: Fn(char) -> bool>(&mut self, dst: &mut String, pred: F) -> usize;
//!     pub fn copy_utf8_while_to_raw<F: Fn(char) -> bool>(&mut self, dst: &mut alloc::vec::Vec<u8>, pred: F) -> usize;
//!
//!     // Iteration support (optional):
//!     // Implement `Iterator<Item = char>` for simple consumers.
//! }
//! ```
//!
//! Implementation notes
//! - Internal storage is `VecDeque<u8>`. It can expose two contiguous slices at
//!   any time via `as_slices()`. Decoding must handle the case where a single
//!   UTF-8 code point spans both slices.
//! - Prefer a small stack buffer (e.g., `[u8; 4]`) to assemble up to four bytes
//!   when a scalar crosses the slice boundary.
//! - Decoding algorithm for `peek_char`/`next_char`:
//!   1. If `data.is_empty()`, return `None`.
//!   2. Read the first byte (`b0`) from the front slice.
//!   3. Determine expected length `len` from `b0` (1, 2, 3, or 4) using the
//!      standard UTF-8 leading byte masks.
//!   4. If the front slice contains at least `len` bytes, decode directly from
//!      that slice; otherwise, copy `len` bytes into a small stack buffer by
//!      reading from both slices, then decode.
//!   5. For `peek_char`, validate and return `Some(char)` without consuming.
//!      For `next_char`, additionally `drain(len)` from the front of `data` and
//!      return the decoded `char`.
//!   6. Decoding should be strict: any invalid sequence should `debug_assert!`
//!      (since upstream guarantees valid UTF-8), and return `None` in release
//!      builds if encountered.
//! - `copy_utf8_while_to_string`/`copy_utf8_while_to_raw` can be built on top of
//!   `peek_char`/`next_char` in a loop, but consider an ASCII fast-path:
//!   - Check the first slice (`front`) and copy contiguous ASCII bytes (<= 0x7F)
//!     that match the predicate into the destination in bulk to reduce per-char
//!     overhead. Stop at the first non-ASCII or predicate failure, then fall
//!     back to scalar decoding. Repeat until exhaustion or predicate failure.
//! - Capacity/reservation: `push_str` can reserve `s.len()` additional bytes
//!   before pushing; `push_bytes` should do the same.
//!
//! API invariants
//! - All bytes stored in `ByteBuffer` form a valid UTF-8 string; pushes must
//!   not split a code point across multiple operations unless the combined
//!   result is valid. Because callers push `&str` (or validated bytes) and
//!   unread batch tails are pushed at char boundaries, this invariant holds.
//! - `peek_char`/`next_char` never panic on invalid UTF-8: use `debug_assert!`
//!   for development and return `None` gracefully on unexpected invalid input.
//! - The predicate for `copy_*_while_*` is applied to decoded `char`s. For raw
//!   accumulation, we still decode to know “character boundaries” and then
//!   append the UTF-8 bytes of those characters to `dst`.
//!
//! Integration with the parser
//! - Replace `crates/jsonmodem/src/parser/buffer.rs` usages with `ByteBuffer` in
//!   the new `InputSession`. Methods that previously consumed `Buffer` by char
//!   should call the corresponding `ByteBuffer` helpers.
//! - Unread batch tails should be appended via `push_str(&batch[bytes_used..])`.
//! - Owned accumulation in raw mode: when reading from the ring, prefer
//!   `copy_utf8_while_to_raw` into `TokenScratch::Raw`. For text mode, use
//!   `copy_utf8_while_to_string` into `TokenScratch::Text`.
//! - The existing char-based `Buffer::copy_while` call sites translate naturally
//!   to the two copy methods above.
//!
//! Test plan (to author in `crates/jsonmodem/src/parser/byte_buffer/tests.rs`)
//! 1) Construction and basic operations
//!    - new() creates empty buffer; `peek_char`/`next_char` return None.
//!    - push_str("abc"); peek/next yield 'a','b','c' in order; then None.
//! 2) Unicode scalars (single slice)
//!    - push_str("åβ👍"); iterate: returns the same sequence; ensure decoding
//!      rounds trips through `String` when re-collected.
//! 3) Split across slices (wrap-around)
//!    - Force a two-slice internal state: e.g., push a large string, pop a few
//!      chars, then push more so `as_slices()` returns non-empty second slice.
//!      Ensure characters that cross the slice boundary are still decoded
//!      correctly by `peek_char`/`next_char`.
//! 4) copy_utf8_while_to_string (ASCII fast path)
//!    - push_str("abc,def"); copy while `!= ','` into a `String`; assert the
//!      destination is "abc" and ring now peeks `','`.
//!    - Continue copying while `char::is_ascii_lowercase` to get "def".
//! 5) copy_utf8_while_to_string (non-ASCII)
//!    - push_str("ååå|ΩΩ"); copy while `!= '|'`; destination is "ååå" and ring
//!      peeks `'|'` next.
//! 6) copy_utf8_while_to_raw (UTF-8 bytes)
//!    - push_str("åβ"); copy while `true` into `Vec<u8>`; assert bytes equal to
//!      `"åβ".as_bytes()`.
//! 7) Mixed operations (peek vs next)
//!    - After pushing "ab", `peek_char` is 'a'; subsequent `peek_char` still 'a';
//!      `next_char` consumes 'a'; `peek_char` then shows 'b'.
//! 8) Invalid UTF-8 push_bytes (guarded)
//!    - If `push_bytes` validates, feed an invalid sequence and assert it is
//!      rejected (panic in debug or Err in a `Result`-returning variant, if you
//!      choose that signature). Ensure the buffer remains unchanged on failure.
//! 9) Property-based fuzz (optional, behind `cfg(test)` or `proptest` feature)
//!    - Generate random valid UTF-8 strings, push them, then pop all chars and
//!      assert equality to the original string. Repeat across random chunking to
//!      exercise two-slice decoding.
//! 10) Performance sanity (optional, `#[bench]` or micro-tests)
//!    - Ensure ASCII fast path yields fewer allocations and fewer `String`
//!      pushes compared to char-by-char decoding.
//!
//! Practical tips
//! - Use `VecDeque::as_slices()` to minimize copies. For multi-byte sequences
//!   split across slices, copy at most 4 bytes into a small stack buffer for
//!   decoding.
//! - Prefer `core::str::from_utf8` for validation if you assemble a small chunk
//!   to decode, or implement the standard UTF-8 decode logic for the first
//!   scalar yourself with branchless bit masks. In both cases, keep it small and
//!   well-tested.
//! - Keep `debug_assert!`s around invariants (valid first byte category; enough
//!   continuation bytes; continuation byte masks) to catch mistakes early while
//!   keeping release builds permissive.
//! - If you expose `Iterator<Item = char>` for `ByteBuffer`, implement it in
//!   terms of `next_char` to avoid duplication.
//!
//! Integration milestones
//! - Implement `ByteBuffer` in this module following the API above.
//! - Add `mod byte_buffer;` in `parser/mod.rs` and switch new code paths to use
//!   it (behind a feature flag initially, if preferred).
//! - Migrate existing `Buffer` call sites used by the upcoming `InputSession` to
//!   `ByteBuffer` equivalents.
//! - Keep the public surface unchanged; all changes are internal.
//!
//! Maintenance/Extensibility
//! - If later we decide to store arbitrary bytes (e.g., to accept non-UTF-8
//!   inputs for lenient modes), split APIs into `push_bytes_unchecked` (unsafe)
//!   and maintain a validation flag to decide whether to decode or pass bytes
//!   through; this is out of scope for strict JSON parsing but a possible
//!   extension.
//!
//! Status
//! - This file currently documents the design only. Implement the `ByteBuffer`
//!   type and the tests outlined above in `byte_buffer/tests.rs` before
//!   switching parser code to use it.

