JSON Parser Buffer System Redesign (Proposal BCDA)

Status: proposal
Audience: parser maintainers and contributors

Summary
- Combine the strengths of BCD (explicit finalize; no RAII back-references) and CDA (parser-persistent token state; single scratch buffer; non-fragmenting keys/numbers) into a single, practical design.
- Two orthogonal building blocks:
  1) InputSession: per-`next()`, owning controller for ring→batch with explicit `finish()`; no mutable references into the parser during lexing.
  2) TokenState + TokenScratch: parser-persistent, per-token state that encodes borrow/own rules, escape/raw mode, and uses one shared scratch buffer for owned accumulation across sources.
- Ephemeral CaptureHandle wraps those for ergonomic lexing (borrow-first, upgrade-on-demand) without owning state.
- Iterator-drop persists in-flight tokens by copying any batch prefix into owned scratch and pushes unread batch tail back into the ring. No RAII mutation of the parser.

Goals
- Keep ring-first semantics and batch borrowing simple and explicit: never borrow from the ring; borrow from the batch only when safe.
- Decouple FSM from ownership mechanics: parser drives states; InputSession and TokenState handle where bytes come from and how they’re materialized.
- Eliminate duplicated buffers/flags and char→byte rescans on hot paths.
- Preserve current external behavior, event lifetimes, and decode options (including surrogate-preserving mode).

Operating Modes
- A. Ring-first: ring has unread chars; consume only from it; all fragments are owned.
- B. Batch-borrow: ring empty and a `&str` batch is provided; parse from the batch; borrow slices when fully contained and no decode differences; upgrade to owned on the first disqualifier.
- C. End-of-input: ring empty and no batch; drive to completion/EOI and errors.

Abstractions

1) InputSession<'src>
- Owns the ring during one `next()` (obtained via `core::mem::take(&mut parser.source)`) and holds an optional borrowed batch `&'src str`.
- Tracks location locally (`pos`, `line`, `col`) and a `batch_bytes` cursor for O(1) batch slicing.
- Exposes a minimal, priority-ordered read API: ring → batch → EOI/Empty.
- Finalization is explicit via `finish()`; there is no RAII writeback.

Sketch
  struct InputSession<'src> {
    ring: Buffer,                // moved out of parser
    batch: Option<&'src str>,    // borrowed; never referenced after finish()
    batch_bytes: usize,          // bytes consumed from batch
    pos: usize, line: usize, col: usize,
    end_of_input: bool,
  }

  enum Peeked { Empty, EndOfInput, Char(char) }

  impl<'src> InputSession<'src> {
    fn peek(&self) -> Peeked;                              // ring → batch → EOI/Empty
    fn advance(&mut self) -> Option<char>;                 // updates pos/line/col, batch_bytes
    fn advance_while<F: Fn(char)->bool>(&mut self, F) -> usize;
    fn in_ring(&self) -> bool;                             // next char from ring
    fn batch_byte(&self) -> usize;                         // current batch byte cursor
    fn slice_batch(&self, start: usize, end: usize) -> Option<&'src str>; // utf-8 safe slicing
    fn finish(self) -> (Buffer, usize, usize, usize, usize); // (ring, bytes_used, pos, line, col)
  }

2) TokenScratch + TokenState (parser-persistent)
- Single owned scratch buffer reused across tokens; supports UTF‑8 text and raw bytes.
- TokenState captures borrow/own eligibility and mode for the current token and persists across calls until the token completes.

  enum TokenScratch { Text(String), Raw(Vec<u8>) }
  impl TokenScratch {
    fn clear(&mut self);
    fn ensure_raw(&mut self);            // migrate Text→Raw once, preserving content
    fn as_text_mut(&mut self) -> &mut String;  // panics if Raw
    fn as_raw_mut(&mut self) -> &mut Vec<u8>;  // panics if Text
  }

  enum CaptureKind { String { raw: bool }, Number, PropertyName }

  struct TokenState {
    kind: CaptureKind,
    started_in_ring: bool,
    start_batch_byte: Option<usize>,     // None if ring; Some(offset) if batch
    had_escape: bool,
    emitted_prefix: bool,                // only for string values
    scratch: TokenScratch,               // owned accumulation
  }

  impl TokenState {
    fn reset(&mut self);
    fn start_in_ring(&mut self, kind: CaptureKind) { self.reset(); self.kind = kind; self.started_in_ring = true; self.start_batch_byte = None; }
    fn start_in_batch(&mut self, kind: CaptureKind, start_byte: usize) { self.reset(); self.kind = kind; self.started_in_ring = false; self.start_batch_byte = Some(start_byte); }
    fn mark_escape(&mut self) { self.had_escape = true; }
    fn ensure_raw(&mut self) { self.scratch.ensure_raw(); if let CaptureKind::String { raw } = &mut self.kind { *raw = true; } }
    fn must_own(&self) -> bool { self.started_in_ring || self.had_escape || matches!(self.kind, CaptureKind::String { raw: true }) }
    fn can_borrow_final(&self, end_byte: usize) -> bool { self.start_batch_byte.is_some() && !self.must_own() && self.start_batch_byte.unwrap() <= end_byte }
  }

3) CaptureHandle<'s>
- Ephemeral shim that operates with `&mut TokenState` and `&mut InputSession` to perform borrow-first scanning and explicit upgrades. It owns no buffers and cannot outlive the iterator call.

  struct CaptureHandle<'a, 'src> {
    st: &'a mut TokenState,
    sess: &'a mut InputSession<'src>,
  }

  impl<'a, 'src> CaptureHandle<'a, 'src> {
    // For string values: if there is a non-empty borrowed prefix, return it once and flip to owned for the remainder.
    fn borrow_prefix_and_switch_to_owned(&mut self) -> Option<&'src str> {
      if self.st.emitted_prefix { return None; }
      let Some(start) = self.st.start_batch_byte else { return None; };
      let upto = self.sess.batch_byte();
      if upto > start && !self.st.must_own() {
        self.st.emitted_prefix = true;
        return self.sess.slice_batch(start, upto);
      }
      None
    }
    fn push_char(&mut self, ch: char) { match &mut self.st.scratch { TokenScratch::Text(s) => s.push(ch), TokenScratch::Raw(b) => { let mut buf = [0u8;4]; let n = ch.encode_utf8(&mut buf).len(); b.extend_from_slice(&buf[..n]); } } }
    fn push_raw(&mut self, bs: &[u8]) { let b = self.st.scratch.as_raw_mut(); b.extend_from_slice(bs); }
    fn advance_while_borrowable<F: FnMut(char)->bool>(&mut self, mut pred: F) -> usize { self.sess.advance_while(|c| pred(c)) }
    fn copy_batch_while_owned<F: FnMut(char)->bool>(&mut self, mut pred: F) -> usize {
      // Owned path inside batch: fast append to Text scratch
      let s = self.st.scratch.as_text_mut();
      let mut copied = 0;
      while let Peeked::Char(c) = self.sess.peek() {
        if !pred(c) { break; }
        if self.sess.in_ring() { break; } // copy only from batch
        self.sess.advance();
        s.push(c);
        copied += 1;
      }
      copied
    }
  }

Borrow/Own Rules (harmonized)
- Borrowing allowed only when the token started in the batch, no escape occurred, not in raw mode, and the token ends within this batch.
- Starting in ring permanently disables borrowing for the token, even if the ring drains into the batch mid-token.
- Keys and numbers never fragment; strings may fragment.
- String values: on first escape while borrowable, emit a single borrowed prefix via `borrow_prefix_and_switch_to_owned`, then continue owned; subsequent fragments are owned (UTF‑8 or Raw).

Iterator Drop and Finalize
- Inside `Iterator::next()`:
  1) Build `InputSession` from parser state (move out ring; copy pos/line/col and end_of_input).
  2) Drive lexing using `CaptureHandle` and produce at most one `LexToken`.
  3) Call `finish()` and write back `(ring, pos, line, col)` to the parser; if a batch was provided, push `&batch[bytes_used..]` into the ring.
- In `StreamingParserIteratorWith::drop`:
  - If a token is in progress and started in the batch, compute `upto = session.batch_byte()` (saved in the iterator before finish), copy `&batch[start_batch_byte..upto]` into `TokenState.scratch` (Text), and set `had_escape = true` (forces owned continuation). Then push the unread tail `&batch[upto..]` to ring.
  - No borrowed content is persisted; only owned state remains in `TokenState`.

Event Emission (unchanged externally)
- LexToken → ParseEvent mapping retains existing borrowed/owned forms.
- Borrowed `&'src str` slices derive from `InputSession::slice_batch` using `start_batch_byte` and current `batch_byte()`.
- Owned fragments come from `TokenScratch` as `String` or `Vec<u8>` (with `RawStrHint`).

Positions and Diagnostics
- `InputSession` maintains `pos/line/col` (char-based), updated by every `advance()`; those are copied back on `finish()`.
- No parser field mutation occurs during lexing, avoiding aliasing; error messages use `InputSession` positions.

Surrogate-Preserving Mode
- Before emitting unpaired surrogates for value strings, call `TokenState::ensure_raw()` and push WTF‑8 bytes through `push_raw`.
- Keys degrade invalid sequences to U+FFFD as today; do not switch to raw for keys.
- Existing parser fields for pending surrogate halves remain and signal when to switch.

Numbers
- Begin in owned mode when `InputSession::in_ring()` is true; else begin borrowable in batch.
- Complete as borrowed when fully within batch; otherwise own (accumulated across ring and/or feeds) and emit as owned.
- On EOI/Empty mid-number, remain owned in `TokenState` for continuation.

Fast Paths
- Ring: `Buffer::copy_while(&mut String, pred)` copies directly into `TokenScratch::Text`.
- Batch (borrowed): `advance_while` scans without copying for long unescaped spans.
- Batch (owned): `copy_batch_while_owned` appends to `TokenScratch::Text` for contiguous spans.

Migration Plan
1) Introduce `TokenScratch` and `TokenState`; route existing owned writes (ring/batch) through `TokenScratch`, add `ensure_raw`.
2) Add `InputSession` with `finish()` and convert numeric lexing first (least branching) to use it.
3) Update iterator-drop to persist in-flight using `TokenState` and to push unread batch tail; remove ad-hoc copy code.
4) Convert string value lexing to `CaptureHandle` and implement `borrow_prefix_and_switch_to_owned`; validate no duplicate fragments.
5) Convert property names; ensure no partials are ever emitted; confirm escapes copy prefix silently.
6) Remove legacy fields (`token_buffer`, `owned_batch_buffer`, `owned_batch_raw`, `token_is_owned`, `token_start_pos`, `string_had_escape`, `batch_cursor`).
7) Remove `BatchView::slice_chars` and any char→byte rescans; rely on `start_batch_byte` and `batch_bytes` only.
8) Benchmark and evaluate a byte-ring as a separate optimization.

Why This Resolves Prior Concerns
- No RAII back-references: all writeback is explicit via `finish()` and iterator-drop helpers.
- No duplicate string fragments: value strings emit a borrowed prefix once, then flip to owned.
- Safe in-flight persistence: only owned state is kept; no borrowed references cross iterator boundaries.
- Clear borrowing rules: tokens started in ring remain owned; borrow requires batch-contained, decode-identical content.
- No `char_indices()` rescans: byte offsets recorded at token start and advanced by the session.
- External API stability: event types, lifetimes, and backend interfaces remain unchanged.

Open Questions
- Always-own numbers for simplicity? This design keeps borrow-first for numbers but can switch to owned-only later without changing interfaces.
- Pool `TokenScratch` capacity across tokens or provide a tiny allocator for fewer reallocations.
- Consider a byte-ring; the abstraction boundaries stay the same.

