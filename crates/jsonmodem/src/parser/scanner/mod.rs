//! Scanner: per‑feed owner for unread input and token state.
//!
//! Why this exists
//! - Borrowing vs owning is a performance tradeoff: we want to return borrowed
//!   slices of the current batch when possible, and seamlessly fall back to
//!   owned accumulation when selection or transforms (escapes/raw) make
//!   borrowing impossible. Centralizing this logic keeps the parser simple and
//!   prevents UTF‑8 rescans.
//!
//! What it does
//! - Reads from the unread input ring (UTF‑8 bytes) and the current batch
//!   (`&'src str`) via `peek()`/`consume()`/`skip()` while maintaining
//!   `pos/line/col`.
//! - Lazily anchors the start of a token (char index and batch byte offset) on
//!   first token‑affecting action, enabling O(1) borrowed slicing via
//!   [`try_borrow_slice`].
//! - Switches to owned accumulation exactly when needed: any `skip()` inside a
//!   token or any explicit transform (e.g., `ensure_raw()`/`push_char`) marks
//!   the token as owned without duplicating already captured data.
//! - Materializes token payloads via `emit()` or `emit_partial()` with no
//!   rescans.
//! - On iterator drop, coalesces an un‑emitted batch prefix into the scratch
//!   and pushes the unread batch tail back into the ring (`finish()`).
//!
//! Scope
//! - The scanner does not enforce token‑level policies (e.g., whether keys or
//!   numbers may fragment). The parser decides when to call `emit()`.
//!
//! Invariants
//! - The ring stores only valid UTF‑8 bytes (input and unread batch tails).
//! - Borrowed slices always come from the current batch (`&'src str`) and are
//!   never taken from the ring (ring bytes can’t be borrowed).
//! - `finish(self)` is single‑shot: it consumes `self` and writes back state.
//!
//! Notes
//! - This module is crate‑internal and not part of the public API. Examples are
//!   marked `ignore` to avoid doctest visibility issues.
//!
//! Example (number fully in batch)
//! ```ignore
//! use jsonmodem::parser::scanner::{Scanner, TokenBuf, Tape};
//!
//! // No unread ring; new batch "12345,"
//! let carry = Tape::default();
//! let mut s = Scanner::from_carryover(carry, "12345,");
//! s.consume_while_ascii(|b| (b as char).is_ascii_digit());
//! match s.emit() {
//!     TokenBuf::Borrowed(n) => assert_eq!(n, "12345"),
//!     _ => unreachable!(),
//! }
//! assert_eq!(s.peek().unwrap().ch, ',');
//! ```
#![allow(dead_code)]

use alloc::{collections::VecDeque, string::String, vec::Vec};
use core::cmp;

/// Where the next character comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Ring,
    Batch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Capture<'src> {
    Borrowed(&'src str),
    Owned(String),
    /// Raw bytes for a token fragment (e.g., surrogate-preserving output).
    /// The parser/backend owns the decode policy; `Scanner` does not
    /// attach hints.
    Raw(Vec<u8>),
}

/// The buffer used to build the current capture (lexeme).
///
/// - `Text(String)`: accumulate as UTF‑8 text.
/// - `Raw(Vec<u8>)`: accumulate as raw bytes (when you need byte‑level
///   control).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureBuf {
    Text(String),
    Raw(Vec<u8>),
}

impl Default for CaptureBuf {
    fn default() -> Self {
        CaptureBuf::Text(String::new())
    }
}

impl CaptureBuf {
    fn clear(&mut self) {
        match self {
            CaptureBuf::Text(s) => s.clear(),
            CaptureBuf::Raw(b) => b.clear(),
        }
    }

    fn push_char(&mut self, ch: char) {
        match self {
            CaptureBuf::Text(s) => s.push(ch),
            CaptureBuf::Raw(b) => {
                let mut tmp = [0u8; 4];
                let s = ch.encode_utf8(&mut tmp);
                b.extend_from_slice(s.as_bytes());
            }
        }
    }

    fn as_text_mut(&mut self) -> &mut String {
        match self {
            CaptureBuf::Text(s) => s,
            CaptureBuf::Raw(_) => panic!("scratch is raw"),
        }
    }

    fn as_raw_mut(&mut self) -> &mut Vec<u8> {
        if let CaptureBuf::Text(s) = self {
            let mut out = Vec::with_capacity(s.len());
            out.extend_from_slice(s.as_bytes());
            *self = CaptureBuf::Raw(out);
        }
        match self {
            CaptureBuf::Raw(b) => b,
            CaptureBuf::Text(_) => unreachable!(),
        }
    }
}

/// The state that describes how the current capture can be returned.
///
/// As long as `owned == false` and `source == Source::Batch` and `raw ==
/// false`, and `start_byte_in_batch` is `Some`, the scanner can return a
/// borrowed `&str`.
#[derive(Debug, Clone)]
pub struct CaptureState {
    pub source: Source,
    pub start_char: usize,
    pub start_byte_in_batch: Option<usize>,
    pub owned: bool,
    pub raw: bool,
}

/// State persisted across feeds when the iterator is dropped or input ends.
///
/// The parser moves this state into a `Scanner` at the start of each
/// feed, and receives it back from [`finish`] at the end. It contains:
/// - the unread UTF‑8 ring,
/// - global position counters,
/// - token scratch (text or raw bytes),
/// - surrogate bookkeeping flags.
#[derive(Debug, Clone)]
pub struct ScannerState {
    pending: VecDeque<u8>,

    char_idx: usize,
    line: usize,
    col: usize,
    scratch: CaptureBuf,
}

impl Default for ScannerState {
    fn default() -> Self {
        Self {
            pending: VecDeque::new(),
            char_idx: 0,
            line: 1,
            col: 1,
            scratch: CaptureBuf::Text(String::new()),
        }
    }
}

impl ScannerState {
    /// Appends bytes to the unread ring.
    pub(crate) fn push_ring_bytes(&mut self, bytes: &[u8]) {
        self.pending.extend(bytes.iter().copied());
    }

    /// Appends UTF-8 text to the token scratch (text or raw bytes).
    pub(crate) fn append_scratch_text(&mut self, s: &str) {
        match &mut self.scratch {
            CaptureBuf::Text(buf) => buf.push_str(s),
            CaptureBuf::Raw(b) => b.extend_from_slice(s.as_bytes()),
        }
    }

    /// Updates position counters.
    pub(crate) fn set_positions(&mut self, pos_char: usize, line: usize, col: usize) {
        self.char_idx = pos_char;
        self.line = line;
        self.col = col;
    }

    #[cfg(debug_assertions)]
    pub(crate) fn debug_ring_bytes(&self) -> alloc::vec::Vec<u8> {
        self.pending.iter().copied().collect()
    }

    #[cfg(debug_assertions)]
    pub(crate) fn debug_scratch_bytes(&self) -> alloc::vec::Vec<u8> {
        match &self.scratch {
            CaptureBuf::Text(s) => s.as_bytes().to_vec(),
            CaptureBuf::Raw(b) => b.clone(),
        }
    }
}

// Test-only inspection helpers to validate session behavior without exposing
// internals in production.
#[cfg(test)]
impl ScannerState {
    pub fn test_ring_bytes(&self) -> Vec<u8> {
        self.pending.iter().copied().collect()
    }

    pub fn test_scratch_text(&self) -> Option<&str> {
        match &self.scratch {
            CaptureBuf::Text(s) => Some(s.as_str()),
            CaptureBuf::Raw(_) => None,
        }
    }
    pub fn test_scratch_raw(&self) -> Option<&[u8]> {
        match &self.scratch {
            CaptureBuf::Raw(b) => Some(b.as_slice()),
            CaptureBuf::Text(_) => None,
        }
    }
    pub fn test_positions(&self) -> (usize, usize, usize) {
        (self.char_idx, self.line, self.col)
    }
}

/// One decoded UTF‑8 scalar, its byte length, and the source it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharInfo {
    pub ch: char,
    /// Number of bytes in `ch`'s UTF-8 representation (1-4).
    pub ch_len: u8,
    pub source: Source,
}

/// A chunked, UTF‑8 aware scanner with zero‑copy capture when possible.
///
/// Typical loop:
/// ```ignore
/// let mut scanner = Scanner::from_carry(carry, batch);
/// while let Peeked::Some(look) = scanner.peek() {
///     match look.char() {
///         c if c.is_whitespace() => { look.skip(); }              // consume, don't capture
///         c if c.is_ascii_digit() => { look.consume(); }      // consume into text capture
///         _ => break,
///     }
/// }
/// let token = scanner.finish_capture();       // returns Borrowed, OwnedText, or OwnedBytes
/// let carry = scanner.finalize();             // pass to next batch
/// ```
#[derive(Default)]
pub struct Scanner<'src> {
    // Unread input
    pending: VecDeque<u8>,
    // Current batch
    batch: &'src str,
    byte_idx: usize,

    // Positions
    char_idx: usize,
    line: usize,
    col: usize,

    // Token-local state
    scratch: CaptureBuf,
    anchor: Option<CaptureState>,
}

impl<'src> Scanner<'src> {
    /// Constructs a new session from prior carryover state and the current
    /// batch.
    ///
    /// The session takes ownership of the unread ring and token scratch, then
    /// reads from the ring (if non‑empty) followed by the batch.
    ///
    /// Complexity: O(1).
    pub fn from_state(carry: ScannerState, batch: &'src str) -> Self {
        Self {
            pending: carry.pending,
            batch,
            byte_idx: 0,
            char_idx: carry.char_idx,
            line: carry.line,
            col: carry.col,
            scratch: carry.scratch,
            anchor: None,
        }
    }

    /// Acknowledges that a partial borrowed fragment has been emitted up to the
    /// current position by advancing the anchor start. Used to avoid
    /// duplicating already-emitted prefixes across feeds.
    pub fn acknowledge_partial_borrow(&mut self) {
        if let Some(a) = &mut self.anchor {
            if a.source == Source::Batch && !a.owned {
                a.start_char = self.char_idx;
                a.start_byte_in_batch = Some(self.byte_idx);
            }
        }
    }

    /// Append UTF-8 text to the current token scratch, ensuring owned mode if
    /// needed.
    pub fn push_text(&mut self, s: &str) {
        self.switch_to_owned_prefix_if_needed();
        match &mut self.scratch {
            CaptureBuf::Text(buf) => buf.push_str(s),
            CaptureBuf::Raw(b) => b.extend_from_slice(s.as_bytes()),
        }
    }

    /// Append a single char to the current token scratch.
    pub fn push_char(&mut self, ch: char) {
        self.switch_to_owned_prefix_if_needed();
        self.scratch.push_char(ch);
    }

    /// Append raw bytes to the current token scratch in raw mode.
    pub fn push_raw_bytes(&mut self, bytes: &[u8]) {
        self.ensure_raw().extend_from_slice(bytes);
    }

    #[cfg(debug_assertions)]
    #[inline]
    pub fn debug_positions(&self) -> (usize, usize, usize) {
        (self.char_idx, self.line, self.col)
    }

    #[cfg(debug_assertions)]
    #[inline]
    pub fn debug_cur_source(&self) -> Source {
        self.cur_source()
    }

    /// Finalizes the session and returns carryover state for the next feed.
    ///
    /// Side effects:
    /// - For fragment‑disallowed tokens (`KeyString`, `Number`), if a token
    ///   started in the batch and has un‑emitted prefix, the prefix is copied
    ///   once into the scratch buffer so parsing can resume in owned mode.
    /// - The unread tail of the batch is appended (as UTF‑8 bytes) to the ring.
    ///
    /// Single‑shot: `finish(self)` consumes the session and should be called at
    /// most once per feed.
    pub fn finish(mut self) -> ScannerState {
        // If token started in batch and not yet owned, copy prefix into scratch
        // so the next feed can continue in owned mode and emit a single fragment.
        if let Some(anchor) = &mut self.anchor {
            if anchor.source == Source::Batch && !anchor.owned {
                if let Some(start) = anchor.start_byte_in_batch {
                    let end = cmp::min(self.byte_idx, self.batch.len());
                    if end > start {
                        let slice = &self.batch.as_bytes()[start..end];
                        match &mut self.scratch {
                            CaptureBuf::Text(s) => {
                                s.push_str(unsafe { core::str::from_utf8_unchecked(slice) })
                            }
                            CaptureBuf::Raw(b) => b.extend_from_slice(slice),
                        }
                        anchor.owned = true;
                    }
                }
            }
        }

        // Push unread tail of the batch into ring
        if self.byte_idx < self.batch.len() {
            let bytes = &self.batch.as_bytes()[self.byte_idx..];
            self.pending.extend(bytes.iter().copied());
        }

        ScannerState {
            pending: self.pending,
            char_idx: self.char_idx,
            line: self.line,
            col: self.col,
            scratch: self.scratch,
        }
    }

    /// Decodes but does not consume the next character from ring or batch.
    pub fn peek(&self) -> Option<CharInfo> {
        if let Some(u) = self.peek_ring() {
            return Some(u);
        }
        self.peek_batch()
    }

    /// Returns the current source (`Ring` if non‑empty, else `Batch`).
    pub fn cur_source(&self) -> Source {
        if self.pending.is_empty() {
            Source::Batch
        } else {
            Source::Ring
        }
    }

    /// Consumes one character and records it into the token scratch.
    ///
    /// Why: consuming is an explicit selection signal — it means this scalar
    /// belongs to the token payload. We always capture it; borrowability is
    /// maintained separately (we don’t force a prefix copy here).
    pub fn consume(&mut self) -> Option<CharInfo> {
        self.ensure_anchor_started();
        let adv = Self::step_input(self)?;
        // Always capture into scratch when a token is active.
        self.scratch.push_char(adv.ch);
        Some(adv)
    }

    /// Internal: advance input by one character (no scratch effects).
    #[inline]
    fn step_input(&mut self) -> Option<CharInfo> {
        if self.pending.is_empty() {
            let (ch, len) = Self::decode_from(self.batch, self.byte_idx)?;
            self.byte_idx += len as usize;
            self.bump_pos(ch);
            Some(CharInfo {
                ch,
                ch_len: len as u8,
                source: Source::Batch,
            })
        } else {
            let (ch, len) = Self::decode_from_ring(&self.pending)?;
            // consume len bytes
            for _ in 0..len {
                self.pending.pop_front();
            }
            self.bump_pos(ch);
            Some(CharInfo {
                ch,
                ch_len: len as u8,
                source: Source::Ring,
            })
        }
    }

    /// Skips one character without recording it in the scratch.
    ///
    /// Why: skipping indicates selection with gaps; a single borrowed slice
    /// from the batch can’t represent gaps. We therefore flip to owned (once)
    /// but avoid copying any already‑captured prefix.
    #[inline]
    pub fn skip(&mut self) -> Option<CharInfo> {
        if let Some(a) = &mut self.anchor {
            // Once we skip within a token, we can no longer represent it as a
            // single borrowed slice; mark owned but avoid copying the already
            // read batch prefix (selective capture semantics).
            a.owned = true;
        }
        Self::step_input(self)
    }

    #[inline]
    fn bump_pos(&mut self, ch: char) {
        if ch == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        self.char_idx += 1;
    }

    fn peek_ring(&self) -> Option<CharInfo> {
        if self.pending.is_empty() {
            return None;
        }
        let (ch, len) = Self::decode_from_ring(&self.pending)?;
        Some(CharInfo {
            ch,
            ch_len: len as u8,
            source: Source::Ring,
        })
    }

    fn peek_batch(&self) -> Option<CharInfo> {
        let (ch, len) = Self::decode_from(self.batch, self.byte_idx)?;
        Some(CharInfo {
            ch,
            ch_len: len as u8,
            source: Source::Batch,
        })
    }

    // Decode first UTF-8 scalar from the ring without consuming
    fn decode_from_ring(r: &VecDeque<u8>) -> Option<(char, usize)> {
        if r.is_empty() {
            return None;
        }
        let (head, _) = r.as_slices();
        let (ch, len) = bstr::decode_utf8(&head);
        if len == 0 {
            return None;
        }
        let ch = ch.unwrap_or('\u{FFFD}'); // replace invalid
        Some((ch, len))
    }

    // Decode first UTF-8 scalar from batch starting at `offset`
    fn decode_from(s: &str, offset: usize) -> Option<(char, usize)> {
        if offset >= s.len() {
            return None;
        }
        let (ch, len) = bstr::decode_utf8(&s.as_bytes()[offset..]);
        if len == 0 {
            return None;
        }
        let ch = ch.unwrap_or('\u{FFFD}'); // replace invalid
        Some((ch, len))
    }

    /// Ensure an anchor exists; lazily record start coordinates and initial
    /// ownership. Why: delaying this lets callers decide by action whether a
    /// token will remain borrowable or must become owned.
    fn ensure_anchor_started(&mut self) {
        if self.anchor.is_some() {
            return;
        }
        let source = self.cur_source();
        let start_char = self.char_idx;
        let start_byte_in_batch = match source {
            Source::Batch => Some(self.byte_idx),
            Source::Ring => None,
        };
        let has_carry = match &self.scratch {
            CaptureBuf::Text(s) => !s.is_empty(),
            CaptureBuf::Raw(b) => !b.is_empty(),
        };
        if !has_carry {
            self.scratch.clear();
        }
        // If token starts in the ring or scratch already has carry, we must own.
        let owned = matches!(source, Source::Ring) || has_carry;
        self.anchor = Some(CaptureState {
            source,
            start_char,
            start_byte_in_batch,
            owned,
            raw: matches!(self.scratch, CaptureBuf::Raw(_)),
        });
    }

    // mark_escape removed: escape handling is expressed via selective
    // `advance` and explicit capture (`push_char`/`ensure_raw`).

    /// Switch to Raw accumulation (WTF‑8). Idempotent.
    ///
    /// Why: surrogate‑preserving or non‑UTF‑8 tolerant backends need raw
    /// bytes. We copy any batch prefix exactly once so subsequent appends are
    /// coherent.
    pub fn ensure_raw(&mut self) -> &mut Vec<u8> {
        // Ensure any existing prefix (possibly in batch) is copied into scratch before
        // switching representation so we don't lose it.
        self.switch_to_owned_prefix_if_needed();
        if let Some(a) = &mut self.anchor {
            a.raw = true;
        }
        self.scratch.as_raw_mut()
    }

    /// Appends UTF-8 text to the current token, switching to owned mode if
    /// needed.
    pub fn append_text(&mut self, s: &str) {
        self.switch_to_owned_prefix_if_needed();
        match &mut self.scratch {
            CaptureBuf::Text(buf) => buf.push_str(s),
            CaptureBuf::Raw(b) => b.extend_from_slice(s.as_bytes()),
        }
        if let Some(a) = &mut self.anchor {
            a.owned = true;
        }
    }

    /// Appends raw bytes to the current token, switching to raw/owned mode.
    pub fn append_raw_bytes(&mut self, bytes: &[u8]) {
        self.ensure_raw().extend_from_slice(bytes);
        if let Some(a) = &mut self.anchor {
            a.owned = true;
        }
    }

    /// Copy the already‑consumed batch prefix into scratch if not already
    /// owned (idempotent). No‑op for ring‑started tokens.
    ///
    /// Why: one‑time coalescing of the batch prefix allows the parser to
    /// continue in owned mode without duplicating data when a transform or
    /// selection boundary is crossed.
    pub fn switch_to_owned_prefix_if_needed(&mut self) {
        let Some(anchor) = &mut self.anchor else {
            return;
        };
        if anchor.owned {
            return;
        }
        if anchor.source == Source::Batch {
            // If we've already been selectively capturing into scratch (e.g.,
            // via `consume()`), avoid copying the batch prefix again.
            let scratch_has_data = match &self.scratch {
                CaptureBuf::Text(s) => !s.is_empty(),
                CaptureBuf::Raw(b) => !b.is_empty(),
            };
            if scratch_has_data {
                anchor.owned = true;
                return;
            }
            let start = anchor.start_byte_in_batch.unwrap_or(self.byte_idx);
            let end = self.byte_idx;
            if end > start {
                let slice = &self.batch.as_bytes()[start..end];
                match &mut self.scratch {
                    CaptureBuf::Text(s) => {
                        s.push_str(unsafe { core::str::from_utf8_unchecked(slice) })
                    }
                    CaptureBuf::Raw(b) => b.extend_from_slice(slice),
                }
            }
            anchor.owned = true;
        } else {
            // Source::Ring: owned already set at begin()
            anchor.owned = true;
        }
    }

    /// Marks the current token as owned without copying any already-read
    /// batch prefix. This is used by selective capture operations to avoid
    /// pulling previously skipped characters into the scratch.
    #[inline]
    fn ensure_owned_without_prefix_copy(&mut self) {
        if let Some(a) = &mut self.anchor {
            a.owned = true;
        }
    }

    /// Batch‑only ASCII fast path: consumes consecutive ASCII bytes satisfying
    /// `pred`, advancing positions. Appends to scratch only in owned mode to
    /// preserve borrow eligibility. Creates the anchor lazily.
    pub fn consume_while_ascii(&mut self, pred: impl Fn(u8) -> bool) -> usize {
        if self.cur_source() != Source::Batch {
            return 0;
        }
        self.ensure_anchor_started();
        let bytes = self.batch.as_bytes();
        let mut i = self.byte_idx;
        let end = bytes.len();
        let mut copied = 0usize;
        while i < end {
            let b = bytes[i];
            if b < 0x80 && pred(b) {
                // advance
                self.byte_idx += 1;
                self.bump_pos(b as char);
                if let Some(a) = &self.anchor {
                    if a.owned {
                        self.scratch.push_char(b as char);
                    }
                }
                i += 1;
                copied += 1;
            } else {
                break;
            }
        }
        copied
    }

    /// Source‑stable char loop: copies while `pred` holds and the source
    /// (ring/batch) doesn’t change. Appends only in owned mode.
    pub fn consume_while_char(&mut self, pred: impl Fn(char) -> bool) -> usize {
        self.ensure_anchor_started();
        let mut copied = 0usize;
        let start_source = self.cur_source();
        loop {
            let Some(u) = self.peek() else { break };
            if u.source != start_source {
                break;
            }
            if !pred(u.ch) {
                break;
            }
            // advance
            let _ = self.step_input();
            if let Some(a) = &self.anchor {
                if a.owned {
                    self.scratch.push_char(u.ch);
                }
            }
            copied += 1;
        }
        copied
    }

    /// Returns a borrowed batch slice if the token started in `Batch`, is still
    /// borrow‑eligible (not raw, not owned), and the byte range is
    /// valid.
    pub fn try_borrow_slice(&self) -> Option<&'src str> {
        let a = self.anchor.as_ref()?;
        if a.source != Source::Batch || a.owned || a.raw {
            return None;
        }
        let start = a.start_byte_in_batch?;
        let end = self.byte_idx;
        if end < start || end > self.batch.len() {
            return None;
        }
        Some(&self.batch[start..end])
    }

    /// Emits a token fragment.
    ///
    /// - If `is_final` is true and the token is still borrow‑eligible, returns
    ///   `Borrowed(&batch[start..end])`.
    /// - Otherwise, returns either `OwnedText(String)` or `Raw(Vec<u8>, hint)`
    ///   depending on the current accumulation mode and decode mode.
    pub fn emit_fragment(&mut self, is_final: bool) -> Capture<'src> {
        if is_final {
            if let Some(s) = self.try_borrow_slice() {
                return Capture::Borrowed(s);
            }
        }
        match core::mem::replace(&mut self.scratch, CaptureBuf::Text(String::new())) {
            CaptureBuf::Text(s) => Capture::Owned(s),
            CaptureBuf::Raw(b) => Capture::Raw(b),
        }
    }

    // --- Emission helpers --------------------------------------------------

    /// Emits the final fragment for the current token (no delimiter adjustment)
    /// and clears the anchor so `finish()` will not coalesce it again.
    pub fn emit(&mut self) -> Capture<'src> {
        // Lazily create an anchor if none exists so empty fragments can borrow
        // correctly from the current batch position.
        self.ensure_anchor_started();
        let buf = self.emit_fragment(true);
        // Token is complete; drop the anchor to avoid finish() copying prefixes.
        self.anchor = None;
        buf
    }

    /// Emits a non-empty partial fragment if any data has accumulated.
    /// - If borrow-eligible, returns a borrowed slice and acknowledges it so
    ///   later `finish()` will not duplicate it.
    /// - Otherwise, switches to owned (idempotent), and returns
    ///   `OwnedText`/`Raw` if the scratch is non-empty. Returns `None` if there
    ///   is nothing to emit.
    pub fn emit_partial(&mut self) -> Option<Capture<'src>> {
        if let Some(s) = self.try_borrow_slice() {
            if !s.is_empty() {
                self.acknowledge_partial_borrow();
                return Some(Capture::Borrowed(s));
            }
            return None;
        }

        // Ensure any batch prefix is captured before checking scratch.
        self.switch_to_owned_prefix_if_needed();
        let is_empty = match &self.scratch {
            CaptureBuf::Text(s) => s.is_empty(),
            CaptureBuf::Raw(b) => b.is_empty(),
        };
        if is_empty {
            return None;
        }
        Some(self.emit_fragment(false))
    }
}

// -------------------------- Peek Guard API --------------------------

/// Guard tying a peeked Unit to the Scanner borrow. Consuming the guard
/// advances the scanner exactly once and returns the same Unit.
pub struct Peeked<'a, 'src> {
    scanner: &'a mut Scanner<'src>,
    unit: CharInfo,
}

impl Peeked<'_, '_> {
    #[inline]
    pub fn ch(&self) -> char {
        self.unit.ch
    }

    #[inline]
    pub fn unit(&self) -> CharInfo {
        self.unit
    }

    /// Consume the guarded character: advances the scanner and records it into
    /// the token scratch (if a token is active). In debug builds, asserts
    /// that the advanced character matches the guard.
    #[inline]
    pub fn consume(self) -> CharInfo {
        let adv = self
            .scanner
            .consume()
            .expect("scanner.consume(): no char after peek");
        debug_assert_eq!(adv.ch, self.unit.ch, "peek/consume mismatch");
        adv
    }

    /// Skip the guarded character: advances positions without modifying token
    /// scratch, returning the same Unit. This also forces owned mode for the
    /// current token (if active) without copying any prior prefix.
    #[inline]
    pub fn skip(self) -> CharInfo {
        let adv = self
            .scanner
            .skip()
            .expect("scanner.skip(): no char after peek");
        debug_assert_eq!(adv.ch, self.unit.ch, "peek/skip mismatch");
        adv
    }

    /// Capture this character as UTF-8 text into the token scratch, switching
    /// to owned if needed, then advance.
    #[inline]
    pub fn capture_text(self) -> CharInfo {
        self.scanner.ensure_owned_without_prefix_copy();
        self.scanner.scratch.push_char(self.unit.ch);
        let adv =
            Scanner::step_input(self.scanner).expect("scanner.step_input(): no char after peek");
        debug_assert_eq!(adv.ch, self.unit.ch, "peek/capture_text mismatch");
        adv
    }

    /// Capture this character as raw bytes (WTF-8) into the token scratch,
    /// switching to raw/owned if needed, then advance.
    #[inline]
    pub fn capture_raw(self) -> CharInfo {
        self.scanner.ensure_raw();
        // Append UTF-8 bytes for this scalar into raw buffer
        let mut tmp = [0u8; 4];
        let s = self.unit.ch.encode_utf8(&mut tmp);
        match &mut self.scanner.scratch {
            CaptureBuf::Raw(b) => b.extend_from_slice(s.as_bytes()),
            CaptureBuf::Text(t) => t.push_str(s),
        }
        let adv =
            Scanner::step_input(self.scanner).expect("scanner.step_input(): no char after peek");
        debug_assert_eq!(adv.ch, self.unit.ch, "peek/capture_raw mismatch");
        adv
    }
}

impl<'src> Scanner<'src> {
    /// Returns a guard over the next character if present. The guard ensures
    /// the scanner can be advanced exactly once via `consume()`.
    pub fn peek_guard(&mut self) -> Option<Peeked<'_, 'src>> {
        self.peek().map(|u| Peeked {
            scanner: self,
            unit: u,
        })
    }
}

#[cfg(test)]
mod tests;
