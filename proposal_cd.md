JSON Parser Buffer Redesign (Proposal CD)

Status: proposal
Audience: parser maintainers and contributors

Objective
- Merge the best of proposals C and D into a single, practical design that:
  - Makes borrow-first behavior explicit and reliable.
  - Separates parsing FSM from ownership mechanics.
  - Preserves the current iterator-drop semantics for partial tokens and unread batch tails.
  - Reduces duplicated buffers and scattered flags while keeping lifetimes sound and performance predictable.

Core Principles
- Ring-first: always consume from the ring before the current batch; never hand out borrows into the ring.
- Borrow-first: for strings and numbers that are wholly inside the current batch and unaffected by decoding, emit borrowed `&'src str`.
- Escalate-to-owned: switch to owned accumulation on escapes, cross-boundary tokens, or any ring participation.
- Localized mechanics: a single place decides borrowing vs owning; a single place performs source arbitration; the parser drives only lexical state.
- Iterator-drop remains the integration point for (a) preserving in-flight token prefix and (b) pushing unread batch tail into the ring.

Architecture

1) SourceCursor (non-owning, unified reader)
- A thin, non-owning view that reads chars from two sources in priority order: `Buffer` ring, then the current batch `&'src str`.
- Holds references to parser fields (ring, pos/line/column) and a local `BatchCursor` for the batch; does not use `mem::take` or RAII Drop.

Sketch
  struct SourceCursor<'p, 'src> {
    ring: &'p mut Buffer,
    batch: Option<&'src str>,
    batch_bytes: usize,   // byte offset into batch
    batch_chars: usize,   // char count consumed from batch
    pos: &'p mut usize,
    line: &'p mut usize,
    col: &'p mut usize,
    end_of_input: bool,
  }

  impl<'p, 'src> SourceCursor<'p, 'src> {
    fn peek(&self) -> PeekedChar;              // ring first, then batch, else Empty/EOI
    fn advance(&mut self) -> Option<char>;     // updates ring/batch and pos/line/col
    fn advance_while<F>(&mut self, F) -> usize where F: FnMut(char)->bool;  // fast path
    fn copy_batch_while_to(&mut self, dst: &mut String, F) -> usize;        // when owned
    fn batch_range_bytes(&self) -> usize { self.batch_bytes }
    // NB: slicing for borrow uses batch byte offsets recorded in TokenCapture (no char scan)
  }

Why non-owning?
- Avoids `mem::take` and RAII complications. The cursor simply reborrows parser fields within `next()`; iterator `drop` continues to push unread batch tail, preserving borrow-first across multiple events in one feed.

2) TokenCapture (parser-persistent per-token state)
- A single struct the parser owns while a token is in flight; it persists across `next()` calls (string partials) and cleanly encodes borrow vs owned decisions.

Sketch
  enum CaptureKind { String { raw: bool }, Number, PropertyName }

  struct TokenCapture {
    kind: CaptureKind,
    started_in_ring: bool,
    start_batch_byte: Option<usize>, // where token began in batch, if in batch
    had_escape: bool,
    utf8: String,                    // owned accumulation (UTF-8)
    raw: alloc::vec::Vec<u8>,        // owned raw bytes for surrogate-preserving mode
  }

  impl TokenCapture {
    fn start_in_ring(kind: CaptureKind) -> Self;
    fn start_in_batch(kind: CaptureKind, start_byte: usize) -> Self;
    fn mark_escape(&mut self);                           // forces owned mode for strings
    fn ensure_raw_mode(&mut self);                       // migrate utf8→raw exactly once
    fn must_own(&self) -> bool;                          // started_in_ring || had_escape || raw
    fn can_borrow_slice(&self, cur_batch_byte: usize) -> bool; // eligible for borrow
    fn push_char(&mut self, ch: char);                   // append into utf8/raw as needed
    fn push_raw_bytes(&mut self, bs: &[u8]);             // raw append
  }

Borrow vs Owned Rules
- Borrow requires: token started in batch (`start_batch_byte.is_some()`), no escape (`had_escape == false`), not in raw mode, and ended within the same batch. Numbers also require single-batch containment; property names require full completion (no partials).
- Owned triggers: token started in ring, any escape in strings, surrogate-preserving raw mode, or batch boundary crossing.

3) Iterator Drop Semantics (unchanged location, simplified helpers)
- Keep `StreamingParserIteratorWith::drop` responsible for:
  1) Preserving in-flight token prefix: copy any already-consumed batch chars into `TokenCapture.utf8` (or `.raw` if needed), then mark the token as owned.
  2) Pushing unread batch tail into the ring so the next feed starts from `self.source`.
- Provide two internal helpers that use `SourceCursor` metadata:
  - preserve_in_flight(parser: &mut Parser, cap: &mut TokenCapture, cursor: &SourceCursor)
  - push_unread_tail(parser: &mut Parser, cursor: &SourceCursor)

Event Surface and LexToken
- Keep the existing `LexToken` variants and ParseEvent emission. Map borrowed slices from the batch to `...Borrowed(&'src str)` and owned fragments from the capture buffers to `...Owned(String)` or `StringRawOwned(Vec<u8>)`. Ring-sourced owned fragments may continue to use the `...Buffered` variants for back-compat if desired.

String Handling Flow
- At `"`: initialize `TokenCapture`:
  - If ring is active at start, `start_in_ring(String { raw: false })`.
  - Else `start_in_batch(String { raw: false }, start_batch_byte = cursor.batch_range_bytes())`.
- Greedy loop:
  - Fast path: advance_while over consecutive non-escape, non-quote chars.
  - On `\`: if the capture can_borrow_slice for the prefix, emit a borrowed partial using `&batch[start_byte..cursor_byte]`, then `mark_escape()` and continue owned; always `ensure_raw_mode()` when decode mode requires raw.
  - On `"`: finalize: if `can_borrow_slice(cursor_byte)`, emit borrowed; else emit owned from capture buffers (utf8/raw). Property names never partial.
- Unpaired surrogate logic mirrors today’s behavior using `ensure_raw_mode` when needed.

Number Handling Flow
- At first digit/sign: capture starts in ring or batch accordingly.
- Advance greedily through digits/point/exponent.
- On token end: borrowed only if capture began in batch and current byte lies within the same batch; otherwise owned (no partials for numbers).

Property Names
- Same rules as strings for borrow vs own, but never emit partials mid-key. On encountering `\` or batch depletion, immediately flip to owned and continue accumulating silently.

Positions and Diagnostics
- `SourceCursor` updates `pos/line/col` directly through references; no finalize step needed. Parser fields always hold up-to-date diagnostics.

Performance Considerations
- Batch slicing: record `start_batch_byte` at token start; use current `batch_bytes` for end; slice with `&batch[start..end]` (no `char_indices` rescans). Maintain `batch_chars` only for diagnostics and iterator-drop copy bounds.
- Fast paths: keep `advance_while` on ring (copy into utf8) and on batch (either just advance for borrowable or copy into utf8 for owned).
- Ring remains `VecDeque<char>` for now (correctness-first); a later optimization can move to a byte ring with decoding at the edges without changing this API.

What This Removes/Simplifies
- Eliminates `token_is_owned`, `string_had_escape`, scattered owned buffers (`token_buffer`, `owned_batch_buffer`, `owned_batch_raw`) in favor of a single `TokenCapture` struct with explicit methods.
- Avoids `mem::take` and RAII finalize of the ring; keeps the current, proven iterator-drop for tail spill.
- Centralizes borrow/own decisions and spill preservation into two small helpers and the capture.

Addressing C and D Concerns Explicitly
- From D: “Drop needs &mut parser” – avoided by not RAII-owning the ring. Iterator `drop` continues to handle batch spill using safe references.
- From D: “In-flight token preservation” – handled by persistent `TokenCapture` on the parser, not ephemeral values; iterator `drop` can always access it.
- From D: “Batch boundary metadata” – we use precise byte offsets (`start_batch_byte` + `batch_bytes`) to slice without rescans.
- From C: “Session must not push unread tail” – adhered to; only iterator `drop` pushes the tail once.
- From C: “Ephemeral capture loses state” – capture persists and accumulates across events until token close.
- From both: “Raw vs UTF-8 switching” – `ensure_raw_mode` migrates once, preserving prior content.
- From both: “Partial rules clarity” – codified: strings may partial; keys and numbers do not.

Minimal Integration Steps
- Add `TokenCapture` and refactor string/number paths to use it for accumulation and borrowing decisions.
- Introduce `SourceCursor` methods (peek/advance/advance_while/copy_batch_while_to), implemented over existing ring + current batch with `BatchCursor` replaced internally.
- Replace scattered buffer/flag checks with capture methods; translate capture output to `LexToken` as today.
- Keep `StreamingParserIteratorWith::drop` and reimplement its body using the two helpers and capture metadata.

Risks and Mitigations
- Risk: misuse of `start_batch_byte` could produce wrong slices.
  - Mitigation: unit tests for cross-boundary tokens, escapes at boundaries, and property-name no-partial behavior.
- Risk: performance regressions on long ASCII runs.
  - Mitigation: benchmark and add ASCII fast paths in `advance_while`; avoid reallocations by reserving in `TokenCapture` based on observed run lengths.
- Risk: lifetime errors when returning borrowed slices.
  - Mitigation: `SourceCursor` never owns data; borrowed slices are taken directly from the batch `&'src str` captured in the iterator; signatures retain the existing `'src` lifetime coupling.

Optional Future Enhancements
- Byte-based ring with UTF‑8 decoder for improved throughput.
- Split `TokenCapture` into specialized `StrCapture`/`NumCapture` types to make illegal operations a type error.
- Pool reusable `String`/`Vec<u8>` capacities inside `TokenCapture` to avoid frequent allocations.

