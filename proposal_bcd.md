JSON Parser Buffer System Redesign (Proposal BCD)

Summary
- Provide a clear, minimal model for where bytes come from and how a token’s
  bytes are materialized, while preserving existing external behavior.
- Two orthogonal building blocks:
  1) InputSession: per-`next()` source controller (ring → batch) with explicit
     finalize, not RAII magic.
  2) TokenCapture: per-token borrow-first accumulator with explicit upgrades
     (UTF‑8 vs raw bytes), and no surprises for keys/numbers.
- Iterator drop preserves in‑flight tokens safely (owned only) and pushes the
  unread batch tail back into the ring.

Objectives
- Reduce cognitive overhead: ring-first vs batch-borrow is obvious and local.
- Borrow-first when safe; own when necessary (escapes, ring involvement,
  cross‑feed, raw mode).
- Keep event semantics and lifetimes: never borrow from ring; may borrow from
  batch; keys never fragment; numbers never fragment; string values may.
- Make finalize/drop effects explicit and testable.

Operating Modes
- A. Ring-first: ring contains leftover chars; consume only from ring until it
  drains. All content is owned.
- B. Batch-borrow: ring empty and batch `&str` provided; parse directly from
  batch; borrow slices when safe; upgrade to owned on escape or cross-batch.
- C. End-of-input: ring empty, no batch; drive to completion or EOI.

Design Overview

1) InputSession<'src>
- Purpose: for a single `Iterator::next()`, unify ring and batch access and
  location tracking, and then return updated state explicitly.
- Construction: `InputSession::new(ring: Buffer, batch: Option<&'src str>,
  pos, line, col, end_of_input)` where `ring` is obtained with
  `core::mem::take(&mut parser.source)`.
- Responsibilities:
  - Expose `peek/advance/advance_while` in priority order: ring → batch.
  - Maintain `batch_bytes` cursor for O(1) borrowed slicing.
  - Maintain `pos/line/col` (char-based) for diagnostics.
  - Never reach back into the parser directly.
- Finalize explicitly (no RAII backdoors):
  - `fn finish(self) -> FinishedSession` returns:
    - `ring: Buffer` (to write back to `parser.source`),
    - `bytes_used: usize` (consumed bytes from the batch),
    - `pos, line, col` (new location).
  - Caller pushes the unread batch tail (`&batch[bytes_used..]`) into the ring
    and stores `ring` back into the parser.

API sketch
  struct InputSession<'src> {
    ring: Buffer,
    batch: Option<&'src str>,
    batch_bytes: usize,
    pos: usize, line: usize, col: usize,
    end_of_input: bool,
  }

  enum Peeked { Empty, EndOfInput, Char(char) }

  impl<'src> InputSession<'src> {
    fn peek(&self) -> Peeked;
    fn advance(&mut self) -> Option<char>;                  // updates pos/line/col, batch_bytes
    fn advance_while<F: Fn(char)->bool>(&mut self, F) -> usize;
    fn in_ring(&self) -> bool;                              // next char comes from ring
    fn batch_byte(&self) -> usize;                          // current batch byte cursor
    fn slice_batch(&self, start_byte: usize, end_byte: usize) -> Option<&'src str>;
    fn finish(self) -> (Buffer, usize, usize, usize, usize); // (ring, bytes_used, pos, line, col)
  }

2) TokenCapture<'src>
- Purpose: per-token “borrow-first, own-on-demand” accumulator.
- Modes:
  - Borrowing { start_byte: usize }
  - OwnedUtf8 { buf: String }
  - OwnedRaw { bytes: Vec<u8> } // surrogate-preserving
- Metadata: `started_in_ring: bool`, `had_escape: bool`, `kind: { Key | String{raw} | Number }`.
- Never holds borrowed data beyond the current iterator call when persisted; if
  stored past the iterator, it must be converted to owned first (see InFlight).

API sketch
  impl<'src> TokenCapture<'src> {
    fn start_in_batch(kind, start_byte: usize) -> Self;      // Borrowing
    fn start_in_ring(kind) -> Self;                          // OwnedUtf8
    fn mark_escape(&mut self);                               // must own remainder
    fn ensure_raw(&mut self);                                // migrate utf8→raw exactly once
    fn push_char(&mut self, c: char);
    fn push_raw(&mut self, bs: &[u8]);
    // String values: emit borrowed prefix exactly once, then switch to owned
    fn borrow_prefix_and_switch_to_owned<'a>(&mut self, batch: &'src str, upto: usize) -> &'src str;
    // Keys: copy prefix into owned (no partials)
    fn upgrade_copy_prefix(&mut self, batch: &str, upto: usize);
    // At token end, decide borrow vs owned
    fn can_borrow_final(&self, end_byte: usize) -> bool;     // still borrowing and in same batch
    fn take_owned_utf8(self) -> String;
    fn take_owned_raw(self) -> Vec<u8>;
  }

Rules
- Must-own when: token started in ring; an escape occurred; raw mode active;
  token crosses feed boundary (ran out of batch or consumed ring at any point).
- Borrow allowed when: mode is Borrowing, no escape/raw, and the token ends in
  the same batch.
- Keys: never fragment. At close quote, borrow the whole slice or emit owned;
  if an escape occurs mid-key, use `upgrade_copy_prefix` and continue owned.
- Numbers: never fragment. Borrow whole slice when fully within batch; otherwise
  owned accumulation across feeds.
- String values: may fragment. Before first escape, emit borrowed prefixes via
  `borrow_prefix_and_switch_to_owned`; after upgrade, emit owned fragments.

3) InFlight state (parser-resident)
- Purpose: safely persist partially-read tokens across iterator drop.
- Shape (owned only):
  enum CaptureKind { Key, String, Number }
  enum TokenBuf { Utf8(String), Raw(Vec<u8>) }
  struct InFlight { kind: CaptureKind, buf: Option<TokenBuf>, started_in_ring: bool, had_escape: bool }
- Invariants:
  - No borrowed data is stored here; any in-flight batch prefix is copied.
  - For string values, if a borrowed prefix was already emitted, `buf` may be
    empty (the token will continue owned from now on).
- Iterator drop sequence: after finalizing session, call a small helper that
  examines parser state and, if mid-token, copies the current in-batch prefix
  (if any) into `InFlight` and stores it on the parser.

Event Emission
- LexToken/ParseEvent keep the same external shape with Borrowed/Buffered/Owned
  variants. Backends and options remain unchanged.

Lifetimes and Aliasing
- The iterator owns `InputSession` for the duration of `next()` and never holds
  a mutable reference into the parser at the same time.
- Borrowed `&'src str` slices always come from the batch captured by the
  iterator; `'src` is tied to the batch lifetime as today.
- `InputSession::finish` writes updated state back explicitly, avoiding `Drop`
  reliance or interior pointers.

Byte vs Char Accounting
- `pos/line/col` track chars for diagnostics; `batch_bytes` + `start_byte`
  suffice for borrowed slices without rescanning.
- Ensure we compute `upto` (byte position) before consuming a backslash/quote
  for prefix borrowing or final slicing.

Strings and Surrogates
- Decode escapes while pushing into owned buffers; before any unpaired
  surrogate emission in values, call `ensure_raw` and continue in raw mode.
- Keys degrade unpaired surrogates to U+FFFD per existing behavior.
- Pending surrogate bookkeeping (e.g., high/low tracking) remains on the parser
  and informs when `ensure_raw` is needed.

Numbers
- Start capture in owned if session is reading from ring; otherwise start in
  borrow mode and emit borrowed at delimiter when fully within batch.
- On EOI/Empty mid-number, convert to owned and record in `InFlight`.

Finalize and Drop Responsibilities
- Inside `Iterator::next()`:
  1) Build `InputSession` from the parser state (taking `source`).
  2) Drive lexing and produce at most one `LexToken`/`ParseEvent`.
  3) Call `finish()`; write back `source`, `pos/line/col`; push unread batch
     tail into the ring (`&batch[bytes_used..]`).
- In `StreamingParserIteratorWith::drop`:
  - If a token is in progress, copy any in-batch prefix into `InFlight` (owned),
    and ensure future continuation is owned (mark `started_in_ring` or
    `had_escape`). Do not attempt to borrow across drops.

Fast Paths
- Ring: `Buffer::copy_while(&mut String, pred)` for owned accumulation.
- Batch (borrowed): `session.advance_while(pred)` to skip over long runs without
  writes.
- Batch (owned): `advance_while` + periodic `push_char`, or add a
  `copy_while_to_owned` helper for tight loops.

Migration Plan (incremental)
1. Introduce `TokenBuf` and `ensure_raw` helper; shim existing buffers to it.
2. Add `InputSession` with `finish`; route number lexing through it first.
3. Update iterator `drop` to use a helper that persists in-flight owned prefix
   into `InFlight`; keep behavior identical for keys/numbers.
4. Port string value lexing to `TokenCapture`; implement
   `borrow_prefix_and_switch_to_owned` and confirm no duplicate fragments.
5. Remove legacy fields (`token_buffer`, `owned_batch_buffer`, `owned_batch_raw`,
   `token_is_owned`, `token_start_pos`, `string_had_escape`, `batch_cursor`).
6. Integrate surrogate-preserving mode with `ensure_raw`; keep surrogate flags
   on parser.
7. Remove char→byte rescans (e.g., `slice_chars`); rely on `start_byte` and
   `batch_bytes` exclusively.
8. Bench and iterate; consider a byte-ring as a separate optimization step.

Why This Design Holds Up
- No duplicate string fragments: value strings return a borrowed prefix once via
  `borrow_prefix_and_switch_to_owned` and then continue owned.
- Safe persistence: `InFlight` stores owned data only; no lifetime hazards.
- Borrow checker friendly: explicit `finish()` prevents `Drop` from reaching
  back into the parser; no interior mutability required.
- O(1) slicing for borrowed paths: start/end byte offsets are tracked, so no
  `char_indices()` rescans; diagnostics remain char-accurate.
- Ring→batch transitions: `started_in_ring` pins ownership for the token even
  after ring drains.
- External API unchanged: event types and lifetimes are preserved.

Open Questions / Options
- Always-own numbers for simplicity? Current design keeps borrow-first but can
  be simplified later if needed.
- Buffer reuse: pool `String`/`Vec<u8>` capacities in the parser and pass them
  into `TokenCapture` to reduce allocations.
- Ring backend: evaluate switching to a byte ring for performance; orthogonal
  to this design.

Appendix: Typical String Value Flow (no duplicates)
1) On opening quote, if `session.in_ring()` then `capture.start_in_ring(String{raw:false})`,
   else `start_in_batch(.., session.batch_byte())`.
2) Scan until backslash or quote via `advance_while`.
3) If backslash:
   - `let upto = session.batch_byte();`
   - `let prefix = capture.borrow_prefix_and_switch_to_owned(batch, upto);` // emit borrowed
   - Consume the backslash + decode and `push_char`/`push_raw` into capture.
   - Continue scanning; further fragments owned.
4) If quote:
   - `let end = byte before quote;`
   - If `capture.can_borrow_final(end)`, emit borrowed once; else emit owned
     (`take_owned_utf8` or `take_owned_raw`).

