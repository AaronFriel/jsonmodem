# Proposal AB: Unified InputCursor + TokenCapture, with Persisted Scratch

A combined and refined design that merges Proposal A (LexInput/TokenCapture) and Proposal B (InputCursor/TokenCapture), clarifies lifetimes and drop semantics, and reduces parser complexity while preserving ring-first parsing and borrow-first emission.

## Objectives
- Separate concerns cleanly:
  - Where we read from: an `InputCursor` that drains the ring first, then reads the current batch.
  - How we capture a token: a `TokenCapture` that decides borrowed vs owned and accumulates content.
- Keep existing invariants: never borrow from the ring; borrow from the batch only when a token lies fully in it with no decoding; numbers never fragment.
- Centralize “partial token across feeds” handling with parser-owned persisted scratch.
- Simplify the parser state machine by removing ad‑hoc flags and buffer merging.

## Stream States at `next()`
- A: Ring non-empty → read ring first. Token is owned for its entire duration (even if later chars come from batch).
- B: Ring empty + batch non-empty → read from batch, borrow if token remains fully in batch and needs no decoding.
- C: Ring empty + batch empty → no data or end-of-input.

## Core Types

### InputCursor<'src>
A unified reader for ring + batch.
- Construction: created per `feed()`/`finish()` by taking the ring via `core::mem::take(&mut parser.source)` and holding an optional `&'src str` batch.
- Responsibilities:
  - Present a single peek/advance interface.
  - Track location: `pos`, `line`, `column` (char-based) internally while parsing; write back on drop.
  - Track batch offsets: `bytes_consumed` and `chars_consumed` for precise slicing and location.
  - Return the ring to the parser on drop and push unread batch tail into the ring.
- API (minimal):
  - `peek() -> Peeked` where `Peeked = Empty | EndOfInput | Char(char)`.
  - `advance() -> Option<char>` (updates position and ring/batch cursors).
  - `advance_while<F>(&mut self, pred: F) -> usize`.
  - `in_ring() -> bool` indicates the next char is from the ring.
  - `batch_offsets() -> (bytes_consumed, total_bytes)`.
  - `borrow_window() -> Option<(&'src str, usize /*start_byte*/, usize /*end_byte*/)>` exposing the current borrowable region only when in batch.

Notes:
- Cursor owns its own `Location` copy; the parser doesn’t get mutably borrowed during lexing. On drop, cursor writes the location back to the parser.
- All char/byte accounting is done once in the cursor to avoid repeated `char_indices()` scans.

### TokenCapture<'src>
A per-token capture that controls borrowed vs owned data flow.
- Modes:
  - `Borrowing { mark_bytes: usize }` when starting in batch and borrowing is still possible.
  - `Owning { buf: String }` when starting in ring, crossing ring→batch, or after we decide to own.
  - `OwningRaw { bytes: Vec<u8> }` when surrogate-preserving requires WTF‑8 bytes.
- API:
  - `start_in_batch(cursor: &InputCursor)` → `Borrowing { mark_bytes: cursor.bytes_consumed }`.
  - `start_owned()` → `Owning { buf: String::new() }`.
  - `push_char(ch)` → ensure owned; append to `buf`.
  - `push_raw(bs)` → ensure raw; append to `bytes` (for surrogate-preserving).
  - `upgrade_from_borrow(batch: &str, upto_bytes: usize)` → copy `[mark_bytes..upto_bytes]` into owned buffer and flip to owned. Use for property names.
  - `borrow_prefix_and_switch_to_owned(batch: &str, upto_bytes: usize) -> &'src str` → return a borrowed prefix and reset capture to empty owned. Use for string values on first escape to emit a fragment.
  - `borrow_slice(batch: &str, upto_bytes: usize) -> Option<&'src str>` → valid only in `Borrowing` mode.
  - `take_owned() -> String`, `take_raw() -> Vec<u8>`.
- Guarantees:
  - If a token starts in the ring, it remains owned for that token.
  - Borrowed slices only ever point into the current batch (never ring).
  - Switching to raw happens at most once per token; existing owned text is migrated to raw on demand.

### Persisted Scratch in Parser
To survive across `feed()` boundaries and iterator drops:
- `enum Scratch { Text(String), Raw(Vec<u8>) }`
- `struct InFlight<'src> { kind: CaptureKind, scratch: Scratch, /* extra per-kind fields if needed */ }`
- `enum CaptureKind { StringValue, PropertyName, Number }`
- Parser holds `in_flight: Option<InFlight<'src>>` plus existing decode-mode state (e.g., surrogate tracking) and string fragment flags.

On iterator drop:
- If a token is in progress and capture is `Borrowing`:
  - Keys/Numbers: copy `[mark..cursor]` into `scratch` and store `InFlight`.
  - String Values: if we already emitted a borrowed prefix, do nothing; else copy and store.
- Push unread batch tail into the ring; move cursor’s ring back into `parser.source`.
- Write back location.

## Event Emission Rules
- Numbers:
  - Never fragment. Prefer borrow if fully within the batch with no cross-feed; otherwise own.
  - If batch ends mid-number, return need-more-data (no event); partial number lives in `in_flight` until completed.
- Strings (values):
  - Borrow fast-path segments fully within batch with no escapes.
  - On first escape: emit borrowed prefix via `borrow_prefix_and_switch_to_owned`, then accumulate into owned/raw until next fragment or completion.
  - May emit multiple fragments with `is_initial`/`is_final` set appropriately.
- Property names:
  - Never fragment. Borrow only if the entire key lies within the batch with no escapes; otherwise own (copy prefix on first escape).
- Ring-only segments (A-state):
  - All tokens are owned for their whole duration.

## Byte/Char Accounting
- Cursor tracks both `chars_consumed` and `bytes_consumed` for batch; slices are by bytes, location by chars.
- When finalizing a borrowed slice, compute `upto_bytes` before consuming the closing quote to avoid off-by-one.
- Add debug assertions to ensure UTF‑8 boundary correctness for borrow slicing.

## Surrogate-Preserving Details
- Maintain parser fields (`pending_high_surrogate`, `last_was_lone_low`) as today.
- `TokenCapture` exposes `ensure_raw()` semantics under the hood; when a raw write is required, migrate text → raw once, then append WTF‑8 bytes.
- Property names degrade to replacement; values switch to raw.

## Integration Plan (Incremental)
1) Introduce `InputCursor` and redirect lexing to use it (preserve old parser fields; adapter layer maps old calls to cursor operations).
2) Add `TokenCapture` over existing parser scratch; convert string lexing to use capture (keep `BatchView` temporarily if needed).
3) Convert number lexing to `TokenCapture`; implement persisted `in_flight` for numbers and keys.
4) Remove `BatchView`, `BatchCursor`, `token_is_owned`, `string_had_escape`, `owned_batch_buffer`, `owned_batch_raw`, and merging helpers; keep only `Scratch` and `in_flight`.
5) Fuzz and benchmark (see Validation).

## Validation Strategy
- Boundary fuzzing: split inputs at every byte across feeds; compare event streams to monolithic parse.
- Borrow/own invariants: when ring has content, no borrowed events; when batch-only and no decoding needed, events can be borrowed.
- Decode modes: strict, replace-invalid, surrogate-preserving; include pairs split across feeds and reversed order.
- Numbers: exponents, signs, leading zeros; no partial fragments; correct need-more-data signaling.
- Keys vs values: property names never fragment; values may; correct `is_initial/is_final` flags.
- Performance: micro-benchmarks of fast-path strings and common mixes.

## Risks and Mitigations (Consolidated)
- Ownership switch misuse: encode switch points in APIs (`borrow_prefix_and_switch_to_owned` vs `upgrade_from_borrow`), provide specialized constructors for kind (number/key/value).
- Lifetime hazards: iterator holds the batch; cursor borrows it; events borrow from batch lifetime; cursor must be dropped after events are produced. Keep this sequence enforced by iterator design.
- Overgrown cursor: keep `InputCursor` focused on reading/position; persisted scratch and parser states remain in the parser; `TokenCapture` is per-token only.
- Drop-time copying on large batches: matches current semantics; document limits and encourage callers to drain iterators promptly.
- Accounting mistakes: keep both char and byte counters; assert on UTF‑8 boundaries; centralize slicing logic.

## Why This Improves A and B
- From A: keeps the strong “mark/commit/take” discipline, but narrows `LexInput` to a lean `InputCursor` with clear boundaries; formalizes persisted `in_flight` instead of overloading `Drop`.
- From B: adopts byte-offset slicing and the explicit `borrow_prefix_and_switch_to_owned` vs `upgrade_from_borrow` split, eliminating duplicate emission bugs and clarifying key vs value behavior.
- Removes duplicate per-token buffers and late merging entirely; one scratch per token (text or raw), persisted across feeds when needed.
- Tightens lifetime and ownership rules so they are enforced by API shape rather than global flags.

---

### Sketch: Typical String Flow (Value)
```text
on '"':
  cap = if cursor.in_ring() { start_owned() } else { start_in_batch(&cursor) }
  while let Char(c) = cursor.peek() {
    match c {
      '"' => { /* compute upto_bytes before consuming */ cursor.advance(); break; }
      '\\' => {
        // emit borrowed prefix and switch to owned
        if let Some(batch) = cursor.borrow_window().map(|(b,_,end)| b) {
          let upto = cursor.batch_offsets().0; // bytes_consumed before consuming escape
          let borrowed = cap.borrow_prefix_and_switch_to_owned(batch, upto);
          emit StringBorrowed(borrowed, partial=true);
        }
        cursor.advance(); // consume '\\'
        decode_escape_into(&mut cap, &mut cursor); // pushes chars/raw
      }
      _ if c < '\u{20}' => error
      _ => { cursor.advance_while(|d| d != '"' && d != '\\' && d >= ' '); }
    }
  }
  // finalize
  if let Some(batch) = cursor.borrow_window().map(|(b,_,_)| b) {
    if let Some(s) = cap.borrow_slice(batch, cursor.batch_offsets().0) { emit borrowed final }
    else { emit owned/raw final }
  } else { emit owned/raw final }
```

### Sketch: Number Flow
```text
cap = if cursor.in_ring() { start_owned() } else { start_in_batch(&cursor) }
scan integral/sign/fraction/exponent using advance/advance_while
if delimiter reached in batch and still borrowing -> emit borrowed number
if batch exhausted mid-number -> return need-more-data; persist cap into parser.in_flight
if ring involved at all -> cap is owned; emit owned at completion
```

This combined approach keeps the implementation intentional and testable, with clear data ownership transitions and minimal state spread across the parser.

