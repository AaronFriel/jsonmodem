# Parser Buffer Redesign (Proposal B)

This proposal simplifies the parser’s buffering and borrowing model by
introducing two focused abstractions that separate “where we read from” from
“how we capture a token”. It keeps the existing overall behavior (ring buffer
first; then borrow from the current batch) while reducing state flags and
ad‑hoc merge logic.

Goals
- Clarify ownership and lifetime boundaries between the ring buffer and the current batch.
- Make it obvious when we emit borrowed vs. owned fragments.
- Provide a small, cohesive API for lexing: peek, advance, copy_while, and a
  mark/take capture interface for strings and numbers.
- Preserve existing guarantees: ring is drained first and never borrowed from;
  batch can be borrowed; escapes or cross‑batch tokens become owned; numbers
  never emit fragments.

States At `next()`
- A: Non‑empty ring buffer. We must read from it first. All fragments become
  owned. When the ring drains mid‑token and we continue into the batch, we
  remain owned for that token.
- B: Empty ring + non‑empty batch. We read from the `&str` batch. Fragments are
  borrowed while the token remains entirely within the batch without escapes.
- C: Empty ring + empty batch. Either need more input or end‑of‑input.

Core Types
- `InputCursor<'src>`: Unifies reads from ring and batch.
  - Constructed per feed/finish iteration by taking temporary ownership of the
    ring (`let ring = core::mem::take(&mut self.source)`), plus an optional
    batch `&'src str`.
  - Implements `Drop` to return the ring to the parser and push any unread
    portion of the batch into the ring (preserving stream continuity).
  - API (minimal):
    - `peek() -> Peek` where `Peek = Empty | EndOfInput | Char(char)`.
    - `advance() -> Option<char>` (updates global `pos/line/column`).
    - `advance_while(pred) -> usize` (fast path scan).
    - `in_ring() -> bool` (true iff the next char comes from the ring).
    - `batch_offsets() -> (bytes_consumed, total_bytes)` (for capture slicing).
- `TokenCapture<'src>`: Own‑or‑borrow capture for strings, property names, and numbers.
  - Starts in one of two modes depending on the token’s first character:
    - `Borrowing { mark_bytes }` when starting in the batch.
    - `Owning { buf: String }` when starting in the ring or after we commit to own.
    - `OwningRaw { bytes: Vec<u8> }` when preserving surrogates (raw string mode).
  - API (mark/take):
    - `start_in_batch(cursor: &InputCursor)` sets `mark_bytes` to the current batch byte offset.
    - `start_owned()` creates an empty owned buffer (string/number/literal).
    - `push_char(ch)` appends to the owned buffer (switching implies owned).
    - `push_raw(bytes)` appends to the raw byte buffer (raw mode only).
    - `upgrade_from_borrow(batch_slice)` copies the batch slice from `mark_bytes`
      to the cursor’s current batch byte offset into the owned buffer and flips
      the mode to owned.
    - `borrow_slice(batch: &str) -> Option<&'src str>` returns a borrowed
      `&str` only if still in `Borrowing` mode and the slice is within the same
      batch.
    - `take_owned() -> String` or `take_raw() -> Vec<u8>` moves out owned data.
  - Behavior rules:
    - If we encounter an escape in a string value, call `upgrade_from_borrow` and
      keep writing into `push_char`/`push_raw`.
    - If a token starts in the ring (or crosses ring→batch), start/keep owned.
    - Numbers never emit partial events. If a number spans batches, either keep
      capturing owned until completion, or return `Eof` and let Drop promote the
      partial capture to owned for the next iteration.

Batch View Simplification
- Replace ad‑hoc `BatchView` + `BatchCursor` usage with byte‑based offsets only
  for slicing; keep separate char‑based global `pos/line/column` for error
  reporting. This avoids re‑scanning `char_indices` twice to compute start/end
  byte indices.
  - `InputCursor` already tracks `bytes_consumed`; `TokenCapture`’s
    `mark_bytes` gives O(1) slicing: `&batch[mark_bytes..cursor.bytes_consumed()]`.

Drop Semantics
- `impl Drop for InputCursor<'_>`:
  - If a token is in flight and currently in `Borrowing` mode (started in the
    batch), force an upgrade by copying the batch slice (from `mark_bytes` to
    `bytes_consumed`) into the parser’s persisted `TokenCapture` (or a simple
    `String`), and mark the token as owned for the next `next()` call.
  - Push the unread tail of the batch into the ring.
  - Move the ring back into `self.source`.

How Parsing Uses The API
- Start of token:
  - If `cursor.in_ring()`: `capture.start_owned()`.
  - Else: `capture.start_in_batch(&cursor)`.
- While scanning:
  - Use `cursor.advance()` or `cursor.advance_while(pred)` to consume.
  - If string escape/Unicode handling is triggered: `capture.upgrade_from_borrow` and
    keep appending decoded chars or raw bytes via `push_char`/`push_raw`.
  - For numbers: scan to completion; if the cursor runs out while in batch and we
    haven’t switched to owned, either return `Eof` (partial) or proactively
    `upgrade_from_borrow` if code simplicity is preferred.
- Emit token:
  - Try `capture.borrow_slice(batch)`; on `Some(s)`, build borrowed event.
  - Otherwise use `take_owned()`/`take_raw()` to build owned event.

What This Replaces
- `token_is_owned`, `string_had_escape`, `token_is_raw_bytes`,
  `token_start_pos`, `owned_batch_buffer`, `owned_batch_raw`, and the bespoke
  merge helpers (`take_owned_from_buffers`, `ensure_raw_mode_and_move_buffers`).
  All become responsibilities of `TokenCapture`.
- `BatchView::slice_chars` can be replaced by byte‑offset slicing derived from
  `InputCursor` + `TokenCapture`.

Sketch: Types and Methods
```rust
enum Peeked { Empty, EndOfInput, Char(char) }

struct InputCursor<'src> {
    ring: Buffer,              // moved in via core::mem::take
    batch: Option<&'src str>,  // borrowed batch
    batch_bytes: usize,        // bytes consumed in batch
    end_of_input: bool,
    // references or copies of pos/line/column updated by advance()
}

impl<'src> InputCursor<'src> {
    fn peek(&self) -> Peeked { /* ring first, else batch, else end */ }
    fn advance(&mut self) -> Option<char> { /* updates pos/line/column */ }
    fn advance_while<F: Fn(char) -> bool>(&mut self, f: F) -> usize { /* scan */ }
    fn in_ring(&self) -> bool { /* next char is from ring? */ }
    fn batch_offsets(&self) -> (usize, usize) { (self.batch_bytes, self.batch.map_or(0, str::len)) }
}

enum TokenCapture<'src> {
    Borrowing { mark_bytes: usize },
    Owning { buf: String },
    OwningRaw { bytes: Vec<u8> },
}

impl<'src> TokenCapture<'src> {
    fn start_in_batch(cursor: &InputCursor<'src>) -> Self { Borrowing { mark_bytes: cursor.batch_bytes } }
    fn start_owned() -> Self { Owning { buf: String::new() } }
    fn push_char(&mut self, c: char) { /* ensure owned; push to buf */ }
    fn push_raw(&mut self, bs: &[u8]) { /* ensure raw; push bytes */ }
    fn upgrade_from_borrow(&mut self, batch: &str, upto_bytes: usize) { /* copy & switch */ }
    fn borrow_slice<'a>(&'a self, batch: &'src str, upto_bytes: usize) -> Option<&'src str> { /* O(1) slice */ }
    fn take_owned(self) -> String { /* move out */ }
    fn take_raw(self) -> Vec<u8> { /* move out */ }
}
```

Numbers vs Strings
- Numbers:
  - Start capture in owned if `cursor.in_ring() == true`.
  - Otherwise start in borrow and finish as borrowed iff fully within the batch.
  - On batch exhaustion, return `Eof` (no fragment) and rely on `Drop` to
    promote partial capture to owned for the next call.
- Strings:
  - Borrow while no escapes are seen and the token is contained within the batch.
  - On first escape, call `upgrade_from_borrow` and decode to owned (or raw) output.
  - Property names never emit partial fragments; apply the same capture rules
    but only emit once on closing quote.
- Surrogate preserving mode:
  - Switch `TokenCapture` to `OwningRaw` when an unpaired surrogate is decoded,
    continuing to write WTF‑8 bytes; property names degrade to replacement as
    today.

Integration Into The Iterator
- `feed_with(..)` creates the per‑batch iterator by constructing an
  `InputCursor` (moving the ring out of the parser) and passing references to
  `pos/line/column` so `advance()` can update location.
- `Iterator::next()` loops `lex()` using `InputCursor` and a scratch
  `TokenCapture`, producing a `LexToken` with either `&'src str` or owned data.
- On iterator drop:
  - If an in‑flight capture is borrowing, copy the already‑consumed part into
    the parser’s persisted capture buffer and mark the token as owned.
  - Push the unread batch tail into the ring, then move the ring back into the
    parser.

Why This Is Simpler
- Single place decides from where we read (`InputCursor`) and a single place
  holds token materialization (`TokenCapture`).
- No scattered state flags on the parser for ownership/escapes/raw mode.
- Byte‑offset slicing removes repeated `char_indices()` walks.
- The borrowed vs owned decision becomes a small set of rules tied to the
  capture instead of global parser flags.

Critique & Trade‑offs
- Extra struct plumbing: `InputCursor` and `TokenCapture` add types, but they
  shrink parser fields and centralize logic in cohesive modules.
- Minor overhead moving the ring via `core::mem::take()` per batch; this is a
  quick pointer/length swap on `VecDeque<char>` and should be cheap.
- Byte‑offset slicing assumes we always advance on char boundaries (we do);
  location metrics still count chars for `line/column`.
- Converting the current code paths to use `TokenCapture` requires touching
  string/number lexing and escape handling, but simplifies them (no more
  `take_owned_from_buffers`, `string_had_escape`, `token_is_owned`, etc.).
- Numbers still need special‑casing (no partial fragments); this proposal keeps
  that constraint explicit in `TokenCapture` use sites.

Incremental Migration Plan
- Step 1: Add `InputCursor` and use it inside the existing iterator scaffolding
  (keep `BatchView` temporarily to bridge lifetimes).
- Step 2: Introduce `TokenCapture` and convert string lexing to use it.
- Step 3: Convert number lexing; delete `token_is_owned`, `owned_batch_buffer`,
  `owned_batch_raw`, and related helpers.
- Step 4: Replace `BatchView::slice_chars` with byte slicing; remove
  `token_start_pos`, simplify drop path.
- Step 5: Trim now‑unused fields and helpers from `StreamingParserImpl`.

Appendix: Typical Control Flow (Strings)
```text
on '"':
  capture = if cursor.in_ring() { start_owned() } else { start_in_batch(&cursor) }
  while let Char(c) = cursor.peek() {
    match c {
      '"' => { cursor.advance(); break; }
      '\\' => { cursor.advance(); capture.upgrade_from_borrow(batch, cursor.batch_bytes); handle_escape(&mut capture, &mut cursor); }
      _ if c < 0x20 => error
      _ => { cursor.advance_while(|d| d != '"' && d != '\\' && d >= ' '); /* no writes if still borrowing */ }
    }
  }
  if let Some(s) = capture.borrow_slice(batch, cursor.batch_bytes) { emit borrowed }
  else if raw_mode { emit capture.take_raw() } else { emit capture.take_owned() }
```

Outcome
- Clear separation of concerns:
  - `InputCursor` decides from where we read and when to copy leftovers.
  - `TokenCapture` decides whether a token is borrowed or owned and provides
    a tiny mark/take interface.
- The public parser behavior stays the same, but the implementation becomes
  easier to reason about, test, and evolve.

---

Critical Review (What Can Go Wrong)
- Borrow→Owned switch for strings can duplicate output: The proposal’s
  `upgrade_from_borrow` copies the pre‑escape prefix into the owned buffer, but
  for string values we actually want to emit that prefix as a borrowed fragment
  and not re‑emit it later. Copying then emitting would duplicate bytes. Fix:
  provide two upgrade paths:
  - Values: `borrow_prefix_and_switch_to_owned(batch, upto_bytes)` which returns
    `&'src str` for emission and resets the capture to an empty owned buffer.
  - Property names: `upgrade_from_borrow(batch, upto_bytes)` which copies the
    prefix into owned (since keys never fragment) and continues owned.
- Persisting in‑flight captures across iterator drop: The proposal relies on
  promoting partial tokens at `Drop` time, but did not specify storage. We need
  a parser field, e.g. `in_flight: Option<CaptureState>`, to hold
  `(kind, TokenCapture, decode_mode flags)` so the next `next()` can resume. For
  numbers, this replaces the current `token_buffer` usage. For keys, it avoids
  losing pre‑escape content when a feed ends mid‑token.
- Borrowing and lifetimes: `InputCursor` wants to own the ring and also mutate
  `pos/line/column`. Holding `&mut` references into the parser while also
  borrowing `&mut self` elsewhere risks borrow checker conflicts. Mitigation:
  - Let `InputCursor` store its own `Location { pos, line, column }` copy and
    return it to the parser on drop (assign back). All lexing goes through the
    cursor so there is a single mutable owner during parsing.
  - Alternatively, keep `InputCursor` methods free of parser references and
    route all state changes through it while the iterator holds no other borrows
    of the parser.
- Byte vs char offsets: The design slices the batch by byte indices derived from
  `bytes_consumed`. This assumes we only advance on character boundaries (true)
  and that we exclude delimiters correctly (e.g., closing quote). We must be
  careful to compute `upto_bytes` before consuming the closing quote to avoid an
  off‑by‑one in `borrow_slice`.
- Cross‑source transitions: Starting a token with the ring non‑empty must force
  owned for the entire token, even after the ring drains. The API needs a
  simple guard (e.g., `capture.is_owned()` set once at token start) to prevent
  accidental borrow when the cursor later points into the batch.
- Drop behavior and partial tokens:
  - Numbers: The proposal says to return `Eof` and rely on `Drop` to promote the
    partial number to owned; we must ensure the `in_flight` storage captures the
    already consumed chars to avoid rescanning.
  - Values mid‑escape: We will already be owned (or raw) and fine, but the drop
    path must not attempt to borrow; it should only push the unread batch tail
    to the ring.
  - Values after emitting a borrowed prefix: There is no “in‑flight” prefix to
    preserve; the capture should be reset to owned and only the unread tail is
    pushed to the ring.
- Extra copying in Drop: Pushing the unread tail of the batch into the ring may
  create temporary memory overhead if the caller feeds very large batches and
  drops iterators early. This matches current behavior but is worth noting.
- Refactor scope: Moving to `InputCursor` + `TokenCapture` touches many code
  paths (string/escape, numbers, literals) and increases near‑term risk even if
  the end state is simpler. The migration plan should be followed strictly with
  tests green after each step.
- Surrogate‑preserving edge cases: The current implementation manages
  high/low‑surrogate bookkeeping (`pending_high_surrogate`, `last_was_lone_low`).
  `TokenCapture::OwningRaw` must integrate with that logic; the proposal keeps
  those fields on the parser, but care is needed so transitions between borrow →
  raw do not lose pending state.

Adjustments To The Proposal
- Add `CaptureState` to the parser:
  - `enum CaptureKind { String, PropertyName, Number }`
  - `struct CaptureState<'src> { kind: CaptureKind, cap: TokenCapture<'src> }`
  - On iterator drop, if a token is in progress and `cap` is Borrowing:
    - For keys/numbers: copy current batch slice into `cap` (owned) and store.
    - For string values: do nothing if we already emitted a borrowed prefix;
      otherwise copy the in‑progress fragment and store.
- API update for string values:
  - Replace `upgrade_from_borrow` with
    `borrow_prefix_and_switch_to_owned(batch, upto_bytes) -> &'src str` that
    both returns the borrowed prefix and flips internal mode to owned with an
    empty buffer. Keys keep `upgrade_from_borrow` to copy instead of borrowing.
- `InputCursor` location handling:
  - Store `Location { pos, line, column }` inside the cursor and write it back
    to the parser on drop. The parser should not be accessed directly while the
    cursor is alive to avoid aliasing.
- Emit boundaries:
  - Ensure we compute `upto_bytes` before consuming the backslash/quote when
    emitting borrowed prefixes or final borrowed strings.

Open Questions
- Should numbers proactively copy into owned when they start in the batch to
  avoid `Drop` work, or keep the borrow‑first approach and accept the Drop path?
- Do we want `TokenCapture` to carry an optional char/byte length hint so backends
  can pre‑allocate when switching to owned?
- Is it worth preserving the old `BatchView` temporarily to ease migration, or
  switch directly to byte offsets in a single change?
