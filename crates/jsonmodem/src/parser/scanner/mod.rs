//! Scanner: per-feed owner for unread input and token state.
//!
//! This internal module defines `Scanner`, a small, single‑owner façade
//! that the streaming parser uses during a single `feed(...)` to:
//!
//! - Read from the unread input ring (UTF‑8 bytes) and the current batch
//!   (`&'src str`) via `peek()`/`advance()` while maintaining `pos/line/col`.
//! - Anchor the start of a token in both character and byte coordinates using
//!   [`begin`], enabling O(1) borrowed slicing from the batch with
//!   [`try_borrow_slice`].
//! - Decide, in one place, whether a token fragment should be Borrowed, Owned
//!   UTF‑8 text, or Raw bytes (for surrogate‑preserving mode).
//! - Materialize token payloads via `emit_*` methods with no UTF‑8 rescans.
//! - On early drop of the iterator, persist in‑flight prefixes for tokens that
//!   cannot fragment (keys and numbers) and push the unread batch tail back
//!   into the ring by calling [`finish`].
//!
//! The API is intentionally small and stateful; the parser drives a
//! conventional state machine and invokes
//! `begin`/`mark_escape`/`ensure_raw`/`copy_while*` and a final `emit_*` to
//! obtain the token payload.
//!
//! Invariants
//! - The ring stores only valid UTF‑8 bytes (input and unread batch tails).
//! - Borrowed slices always come from the current batch (`&'src str`) and are
//!   never taken from the ring.
//! - Keys and numbers never fragment across feeds; value strings may fragment.
//! - `finish(self)` is single‑shot: it consumes `self` and writes back state.
//!
//! Notes
//! - This module is crate‑internal and not part of the public API. Examples are
//!   marked `ignore` to avoid doctest visibility issues.
//!
//! Example (number fully in batch)
//! ```ignore
//! use jsonmodem::parser::scanner::{Scanner, FragmentPolicy, TokenBuf, Tape};
//!
//! // No unread ring; new batch "12345,"
//! let carry = Tape::default();
//! let mut s = Scanner::from_carryover(carry, "12345,");
//! s.begin(FragmentPolicy::Disallowed);
//! s.copy_while_ascii(|b| (b as char).is_ascii_digit());
//! match s.emit_fragment(true, 0) {
//!     TokenBuf::Borrowed(n) => assert_eq!(n, "12345"),
//!     _ => unreachable!(),
//! }
//! assert_eq!(s.peek().unwrap().ch, ',');
//! ```
#![allow(dead_code)]

use alloc::{collections::VecDeque, string::String, vec::Vec};
use core::cmp;

// Internal session; no parser options are needed here.

/// Indicates the current source of characters: unread ring or active batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Ring,
    Batch,
}

/// Token policy informing fragmentation rules.
///
/// - `KeyString`: fragment‑disallowed; may be Borrowed or OwnedText, and Raw
///   when surrogate‑preserving is enabled.
/// - `ValueString`: fragment‑allowed; may be Borrowed, OwnedText, or Raw.
/// - `Number`: fragment‑disallowed; Borrowed or OwnedText.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FragmentPolicy {
    Disallowed,
    Allowed,
}

/// Token payload returned by `emit_*` methods.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenBuf<'src> {
    Borrowed(&'src str),
    OwnedText(String),
    /// Raw bytes for a token fragment (e.g., surrogate-preserving output).
    /// The parser/backend owns the decode policy; `Scanner` does not
    /// attach hints.
    Raw(Vec<u8>),
}

/// Owned accumulation buffer for the current token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenScratch {
    Text(String),
    Raw(Vec<u8>),
}

impl Default for TokenScratch {
    fn default() -> Self {
        TokenScratch::Text(String::new())
    }
}

impl TokenScratch {
    fn clear(&mut self) {
        match self {
            TokenScratch::Text(s) => s.clear(),
            TokenScratch::Raw(b) => b.clear(),
        }
    }

    fn push_char(&mut self, ch: char) {
        match self {
            TokenScratch::Text(s) => s.push(ch),
            TokenScratch::Raw(b) => {
                let mut tmp = [0u8; 4];
                let s = ch.encode_utf8(&mut tmp);
                b.extend_from_slice(s.as_bytes());
            }
        }
    }

    fn as_text_mut(&mut self) -> &mut String {
        match self {
            TokenScratch::Text(s) => s,
            TokenScratch::Raw(_) => panic!("scratch is raw"),
        }
    }

    fn to_raw(&mut self) -> &mut Vec<u8> {
        if let TokenScratch::Text(s) = self {
            let mut out = Vec::with_capacity(s.len());
            out.extend_from_slice(s.as_bytes());
            *self = TokenScratch::Raw(out);
        }
        match self {
            TokenScratch::Raw(b) => b,
            TokenScratch::Text(_) => unreachable!(), // Should never happen
        }
    }

    fn take_text(self) -> String {
        match self {
            TokenScratch::Text(s) => s,
            TokenScratch::Raw(_) => panic!("expected text scratch, found raw"),
        }
    }

    fn take_raw(self) -> Vec<u8> {
        match self {
            TokenScratch::Raw(b) => b,
            TokenScratch::Text(s) => s.into_bytes(),
        }
    }
}

/// Byte/char anchors and token state captured at `begin`.
#[derive(Debug, Clone)]
pub struct TokenAnchor {
    pub source: Source,
    pub start_char: usize,
    pub start_byte_in_batch: Option<usize>,
    pub owned: bool,
    pub had_escape: bool,
    pub is_raw: bool,
    pub policy: FragmentPolicy,
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
pub struct Tape {
    ring: VecDeque<u8>,
    pos_char: usize,
    line: usize,
    col: usize,
    scratch: TokenScratch,
}

impl Default for Tape {
    fn default() -> Self {
        Self {
            ring: VecDeque::new(),
            pos_char: 0,
            line: 1,
            col: 1,
            scratch: TokenScratch::Text(String::new()),
        }
    }
}

impl Tape {
    /// Appends bytes to the unread ring.
    pub(crate) fn push_ring_bytes(&mut self, bytes: &[u8]) {
        self.ring.extend(bytes.iter().copied());
    }

    /// Appends UTF-8 text to the token scratch (text or raw bytes).
    pub(crate) fn append_scratch_text(&mut self, s: &str) {
        match &mut self.scratch {
            TokenScratch::Text(buf) => buf.push_str(s),
            TokenScratch::Raw(b) => b.extend_from_slice(s.as_bytes()),
        }
    }

    /// Updates position counters.
    pub(crate) fn set_positions(&mut self, pos_char: usize, line: usize, col: usize) {
        self.pos_char = pos_char;
        self.line = line;
        self.col = col;
    }

    #[cfg(debug_assertions)]
    pub(crate) fn debug_ring_bytes(&self) -> alloc::vec::Vec<u8> {
        self.ring.iter().copied().collect()
    }

    #[cfg(debug_assertions)]
    pub(crate) fn debug_scratch_bytes(&self) -> alloc::vec::Vec<u8> {
        match &self.scratch {
            TokenScratch::Text(s) => s.as_bytes().to_vec(),
            TokenScratch::Raw(b) => b.clone(),
        }
    }
}

// Test-only inspection helpers to validate session behavior without exposing
// internals in production.
#[cfg(test)]
impl Tape {
    pub fn test_ring_bytes(&self) -> Vec<u8> {
        self.ring.iter().copied().collect()
    }
    pub fn test_scratch_text(&self) -> Option<&str> {
        match &self.scratch {
            TokenScratch::Text(s) => Some(s.as_str()),
            _ => None,
        }
    }
    pub fn test_scratch_raw(&self) -> Option<&[u8]> {
        match &self.scratch {
            TokenScratch::Raw(b) => Some(b.as_slice()),
            _ => None,
        }
    }
    pub fn test_positions(&self) -> (usize, usize, usize) {
        (self.pos_char, self.line, self.col)
    }
}

/// One decoded UTF‑8 scalar, its byte length, and the source it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unit {
    pub ch: char,
    pub ch_len: u8,
    pub source: Source,
}

#[derive(Default)]
pub struct Scanner<'src> {
    // Unread input
    ring: VecDeque<u8>,
    // Current batch
    batch: &'src str,
    batch_bytes: usize,

    // Positions
    pos_char: usize,
    line: usize,
    col: usize,

    // Token-local state
    scratch: TokenScratch,
    anchor: Option<TokenAnchor>,
}

impl<'src> Scanner<'src> {
    /// Constructs a new session from prior carryover state and the current
    /// batch.
    ///
    /// The session takes ownership of the unread ring and token scratch, then
    /// reads from the ring (if non‑empty) followed by the batch.
    ///
    /// Complexity: O(1).
    pub fn from_carryover(carry: Tape, batch: &'src str) -> Self {
        Self {
            ring: carry.ring,
            batch,
            batch_bytes: 0,
            pos_char: carry.pos_char,
            line: carry.line,
            col: carry.col,
            scratch: carry.scratch,
            anchor: None,
        }
    }

    /// Acknowledges that a partial borrowed fragment has been emitted up to the
    /// current position. This advances the anchor's start to the current
    /// `pos_char`/`batch_bytes` so that `finish()` will not copy the already
    /// emitted prefix into the scratch, preserving borrow-first behavior across
    /// feeds.
    pub fn acknowledge_partial_borrow(&mut self) {
        if let Some(a) = &mut self.anchor {
            if a.source == Source::Batch && !a.owned {
                a.start_char = self.pos_char;
                a.start_byte_in_batch = Some(self.batch_bytes);
            }
        }
    }

    /// Ensure an anchor exists; begin if not started.
    #[inline]
    pub fn ensure_begun(&mut self, policy: FragmentPolicy) {
        if self.anchor.is_none() {
            self.begin(policy);
        }
    }

    /// Append UTF-8 text to the current token scratch, ensuring owned mode if
    /// needed.
    pub fn push_text(&mut self, s: &str) {
        self.switch_to_owned_prefix_if_needed();
        match &mut self.scratch {
            TokenScratch::Text(buf) => buf.push_str(s),
            TokenScratch::Raw(b) => b.extend_from_slice(s.as_bytes()),
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
        (self.pos_char, self.line, self.col)
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
    pub fn finish(mut self) -> Tape {
        // For fragment-disallowed tokens, if started in batch and
        // not yet owned, copy prefix into scratch so the next feed continues owned.
        if let Some(anchor) = &mut self.anchor {
            match anchor.policy {
                FragmentPolicy::Disallowed => {
                    if anchor.source == Source::Batch && !anchor.owned {
                        if let Some(start) = anchor.start_byte_in_batch {
                            let end = cmp::min(self.batch_bytes, self.batch.len());
                            if end > start {
                                // ensure text mode scratch
                                if matches!(self.scratch, TokenScratch::Raw(_)) {
                                    // keep bytes as-is when raw; but for
                                    // numbers/keys we shouldn't be in Raw.
                                }
                                let slice = &self.batch.as_bytes()[start..end];
                                match &mut self.scratch {
                                    TokenScratch::Text(s) => {
                                        s.push_str(unsafe { core::str::from_utf8_unchecked(slice) })
                                    }
                                    TokenScratch::Raw(b) => b.extend_from_slice(slice),
                                }
                                anchor.owned = true;
                            }
                        }
                    }
                }
                FragmentPolicy::Allowed => {
                    // Coalesce value string prefixes on iterator drop: copy batch prefix
                    // so the next feed can continue in owned mode and emit a single fragment.
                    if anchor.source == Source::Batch && !anchor.owned {
                        if let Some(start) = anchor.start_byte_in_batch {
                            let end = cmp::min(self.batch_bytes, self.batch.len());
                            if end > start {
                                let slice = &self.batch.as_bytes()[start..end];
                                match &mut self.scratch {
                                    TokenScratch::Text(s) => {
                                        s.push_str(unsafe { core::str::from_utf8_unchecked(slice) })
                                    }
                                    TokenScratch::Raw(b) => b.extend_from_slice(slice),
                                }
                                anchor.owned = true;
                            }
                        }
                    }
                }
            }
        }

        // Push unread tail of the batch into ring
        if self.batch_bytes < self.batch.len() {
            let bytes = &self.batch.as_bytes()[self.batch_bytes..];
            self.ring.extend(bytes.iter().copied());
        }

        Tape {
            ring: self.ring,
            pos_char: self.pos_char,
            line: self.line,
            col: self.col,
            scratch: self.scratch,
        }
    }

    /// Decodes but does not consume the next character from ring or batch.
    pub fn peek(&self) -> Option<Unit> {
        if let Some(u) = self.peek_ring() {
            return Some(u);
        }
        self.peek_batch()
    }

    /// Returns the current source (`Ring` if non‑empty, else `Batch`).
    pub fn cur_source(&self) -> Source {
        if !self.ring.is_empty() {
            Source::Ring
        } else {
            Source::Batch
        }
    }

    /// Consumes one character from the current source and updates
    /// `pos/line/col`.
    pub fn consume(&mut self) -> Option<Unit> {
        if !self.ring.is_empty() {
            let (ch, len) = Self::decode_from_ring(&self.ring)?;
            // consume len bytes
            for _ in 0..len {
                self.ring.pop_front();
            }
            self.bump_pos(ch);
            Some(Unit {
                ch,
                ch_len: len as u8,
                source: Source::Ring,
            })
        } else {
            let (ch, len) = Self::decode_from_batch(self.batch, self.batch_bytes)?;
            self.batch_bytes += len;
            self.bump_pos(ch);
            Some(Unit {
                ch,
                ch_len: len as u8,
                source: Source::Batch,
            })
        }
    }

    /// Skips one character from the current source, identical to `consume()`
    /// with respect to position/ring/batch cursors. Provided for clarity when
    /// the caller intends to advance without touching token scratch.
    #[inline]
    pub fn skip(&mut self) -> Option<Unit> {
        self.consume()
    }

    #[inline]
    fn bump_pos(&mut self, ch: char) {
        if ch == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        self.pos_char += 1;
    }

    fn peek_ring(&self) -> Option<Unit> {
        if self.ring.is_empty() {
            return None;
        }
        let (ch, len) = Self::decode_from_ring(&self.ring)?;
        Some(Unit {
            ch,
            ch_len: len as u8,
            source: Source::Ring,
        })
    }

    fn peek_batch(&self) -> Option<Unit> {
        let (ch, len) = Self::decode_from_batch(self.batch, self.batch_bytes)?;
        Some(Unit {
            ch,
            ch_len: len as u8,
            source: Source::Batch,
        })
    }

    // Decode first UTF-8 scalar from the ring without consuming
    fn decode_from_ring(r: &VecDeque<u8>) -> Option<(char, usize)> {
        let b0 = *r.get(0)?;
        let len = utf8_len_from_lead(b0)?;
        let mut tmp = [0u8; 4];
        for i in 0..len {
            tmp[i] = *r.get(i)?;
        }
        let s = core::str::from_utf8(&tmp[..len]).ok()?;
        let ch = s.chars().next()?;
        Some((ch, len))
    }

    // Decode first UTF-8 scalar from batch starting at `offset`
    fn decode_from_batch(s: &str, offset: usize) -> Option<(char, usize)> {
        if offset >= s.len() {
            return None;
        }
        let tail = &s.as_bytes()[offset..];
        let b0 = tail[0];
        let len = utf8_len_from_lead(b0)?;
        let ch = core::str::from_utf8(&tail[..len]).ok()?.chars().next()?;
        Some((ch, len))
    }

    /// Marks the start of the next token, capturing anchors and policy.
    ///
    /// If the token begins in the ring, the session immediately switches to
    /// owned mode (ring content can’t be borrowed).
    pub fn begin(&mut self, policy: FragmentPolicy) {
        let source = self.cur_source();
        let start_char = self.pos_char;
        let start_byte_in_batch = match source {
            Source::Batch => Some(self.batch_bytes),
            Source::Ring => None,
        };
        // If token starts in the ring, we can never borrow; start owned immediately.
        let owned = matches!(source, Source::Ring);
        // If scratch already has a carried prefix (from a previous feed), preserve it
        // and continue in owned mode rather than clearing it.
        let has_carry = match &self.scratch {
            TokenScratch::Text(s) => !s.is_empty(),
            TokenScratch::Raw(b) => !b.is_empty(),
        };
        if !has_carry {
            self.scratch.clear();
        }
        self.anchor = Some(TokenAnchor {
            source,
            start_char,
            start_byte_in_batch,
            owned: owned || has_carry,
            had_escape: false,
            is_raw: matches!(self.scratch, TokenScratch::Raw(_)),
            policy,
        });
    }

    /// Marks that an escape/transform was encountered in the current token.
    ///
    /// This sets `had_escape = true` and ensures the batch prefix (if any) is
    /// copied into the scratch in an idempotent manner.
    pub fn mark_escape(&mut self) {
        if let Some(a) = &mut self.anchor {
            a.had_escape = true;
        }
        self.switch_to_owned_prefix_if_needed();
    }

    /// Ensures the current token switches to Raw accumulation (WTF‑8) and moves
    /// any previously accumulated UTF‑8 text into the raw buffer. Idempotent.
    pub fn ensure_raw(&mut self) -> &mut Vec<u8> {
        // Ensure any existing prefix (possibly in batch) is copied into scratch before
        // switching representation so we don't lose it.
        self.switch_to_owned_prefix_if_needed();
        if let Some(a) = &mut self.anchor {
            a.is_raw = true;
        }
        self.scratch.to_raw()
    }

    /// Appends UTF-8 text to the current token, switching to owned mode if
    /// needed.
    pub fn append_text(&mut self, s: &str) {
        self.switch_to_owned_prefix_if_needed();
        match &mut self.scratch {
            TokenScratch::Text(buf) => buf.push_str(s),
            TokenScratch::Raw(b) => b.extend_from_slice(s.as_bytes()),
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

    /// Copies the already‑consumed batch prefix into the scratch if not already
    /// owned. Idempotent; safe to call multiple times.
    ///
    /// No-op if the token began in the ring (already owned).
    pub fn switch_to_owned_prefix_if_needed(&mut self) {
        let Some(anchor) = &mut self.anchor else {
            return;
        };
        if anchor.owned {
            return;
        }
        if anchor.source == Source::Batch {
            let start = anchor.start_byte_in_batch.unwrap_or(self.batch_bytes);
            let end = self.batch_bytes;
            if end > start {
                let slice = &self.batch.as_bytes()[start..end];
                match &mut self.scratch {
                    TokenScratch::Text(s) => {
                        s.push_str(unsafe { core::str::from_utf8_unchecked(slice) })
                    }
                    TokenScratch::Raw(b) => b.extend_from_slice(slice),
                }
            }
            anchor.owned = true;
        } else {
            // Source::Ring: owned already set at begin()
            anchor.owned = true;
        }
    }

    /// Batch‑only ASCII fast path: copies consecutive ASCII bytes satisfying
    /// `pred`. If the token is in owned mode, appends to the scratch; otherwise
    /// only advances cursors to keep borrow eligibility.
    pub fn copy_while_ascii(&mut self, pred: impl Fn(u8) -> bool) -> usize {
        if self.cur_source() != Source::Batch {
            return 0;
        }
        let bytes = self.batch.as_bytes();
        let mut i = self.batch_bytes;
        let end = bytes.len();
        let mut copied = 0usize;
        while i < end {
            let b = bytes[i];
            if b < 0x80 && pred(b) {
                // advance
                self.batch_bytes += 1;
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
    pub fn copy_while_char(&mut self, pred: impl Fn(char) -> bool) -> usize {
        let mut copied = 0usize;
        let start_source = self.cur_source();
        loop {
            let Some(u) = self.peek() else {
                break;
            };
            if u.source != start_source {
                break;
            }
            if !pred(u.ch) {
                break;
            }
            // For ring path, begin() should have set owned=true; batch path may still be
            // borrow-eligible.
            if let Some(a) = &self.anchor {
                if a.owned {
                    self.scratch.push_char(u.ch);
                }
            }
            let _ = self.consume();
            copied += 1;
        }
        copied
    }

    /// Returns a borrowed batch slice if the token started in `Batch`, is still
    /// borrow‑eligible (no escapes, not raw, not owned), and the byte range is
    /// valid.
    pub fn try_borrow_slice(&self) -> Option<&'src str> {
        let a = self.anchor.as_ref()?;
        if a.source != Source::Batch || a.owned || a.had_escape || a.is_raw {
            return None;
        }
        let start = a.start_byte_in_batch?;
        let end = self.batch_bytes;
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
    pub fn emit_fragment(&mut self, is_final: bool) -> TokenBuf<'src> {
        if is_final {
            if let Some(s) = self.try_borrow_slice() {
                return TokenBuf::Borrowed(s);
            }
        }
        match core::mem::replace(&mut self.scratch, TokenScratch::Text(String::new())) {
            TokenScratch::Text(s) => TokenBuf::OwnedText(s),
            TokenScratch::Raw(b) => TokenBuf::Raw(b),
        }
    }

    // --- Simplified helpers (emit-then-advance semantics) -----------------

    /// Emits the final fragment for the current token (no delimiter adjustment)
    /// and clears the anchor so `finish()` will not coalesce it again.
    pub fn emit_final(&mut self) -> TokenBuf<'src> {
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
    pub fn emit_partial(&mut self) -> Option<TokenBuf<'src>> {
        if let Some(s) = self.try_borrow_slice() {
            if !s.is_empty() {
                self.acknowledge_partial_borrow();
                return Some(TokenBuf::Borrowed(s));
            }
            return None;
        }

        // Ensure any batch prefix is captured before checking scratch.
        self.switch_to_owned_prefix_if_needed();
        let is_empty = match &self.scratch {
            TokenScratch::Text(s) => s.is_empty(),
            TokenScratch::Raw(b) => b.is_empty(),
        };
        if is_empty {
            return None;
        }
        Some(self.emit_fragment(false))
    }

    /// For transform boundaries (e.g., escape start):
    /// - For Allowed strings, if still borrow-eligible, returns the borrowed
    ///   prefix and acknowledges it; otherwise switches to owned and returns
    ///   None.
    /// - For Disallowed tokens (keys/numbers), switches to owned and returns
    ///   None.
    pub fn yield_prefix(&mut self) -> Option<TokenBuf<'src>> {
        let policy = self.anchor.as_ref().map(|a| a.policy);
        match policy {
            Some(FragmentPolicy::Allowed) => {
                if let Some(s) = self.try_borrow_slice() {
                    if !s.is_empty() {
                        self.acknowledge_partial_borrow();
                        return Some(TokenBuf::Borrowed(s));
                    }
                    return None;
                }
                // Not borrow-eligible: commit to owned.
                self.switch_to_owned_prefix_if_needed();
                None
            }
            Some(FragmentPolicy::Disallowed) | None => {
                self.switch_to_owned_prefix_if_needed();
                None
            }
        }
    }

    /// Explicitly switch to owned mode by copying the batch prefix once.
    pub fn own_prefix(&mut self) {
        self.switch_to_owned_prefix_if_needed();
    }
}

// -------------------------- Peek Guard API --------------------------

/// Result of a guarded peek; either a character with a guard that can be
/// consumed exactly once, or empty.
pub enum Peeked<'a, 'src> {
    Char(PeekGuard<'a, 'src>),
    Empty,
}

/// Guard tying a peeked Unit to the Scanner borrow. Consuming the guard
/// advances the scanner exactly once and returns the same Unit.
pub struct PeekGuard<'a, 'src> {
    scanner: &'a mut Scanner<'src>,
    unit: Unit,
}

impl<'a, 'src> PeekGuard<'a, 'src> {
    #[inline]
    pub fn ch(&self) -> char {
        self.unit.ch
    }

    #[inline]
    pub fn unit(&self) -> Unit {
        self.unit
    }

    /// Advance the underlying scanner and return the peeked Unit. In debug
    /// builds, asserts that the advanced character matches the guard.
    #[inline]
    pub fn consume(self) -> Unit {
        #[cfg(debug_assertions)]
        {
            let adv = self.scanner.consume().expect("scanner advanced after peek");
            debug_assert_eq!(adv.ch, self.unit.ch, "peek/advance mismatch");
            adv
        }
        #[cfg(not(debug_assertions))]
        {
            let _ = self.scanner.consume();
            self.unit
        }
    }
}

impl<'src> Scanner<'src> {
    /// Returns a guard over the next character if present. The guard ensures
    /// the scanner can be advanced exactly once via `consume()`.
    pub fn peek_guard(&mut self) -> Peeked<'_, 'src> {
        match self.peek() {
            Some(u) => Peeked::Char(PeekGuard {
                scanner: self,
                unit: u,
            }),
            None => Peeked::Empty,
        }
    }
}

#[inline]
fn utf8_len_from_lead(b0: u8) -> Option<usize> {
    if b0 < 0x80 {
        Some(1)
    } else if (0xC2..=0xDF).contains(&b0) {
        Some(2)
    } else if (0xE0..=0xEF).contains(&b0) {
        Some(3)
    } else if (0xF0..=0xF4).contains(&b0) {
        Some(4)
    } else {
        None
    }
}

#[cfg(test)]
mod tests;
