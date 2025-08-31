# Proposal ABC: Unified InputCursor + TokenCapture with Persisted In‑Flight State

A self‑contained redesign of the streaming JSON parser’s input and buffering
model. It unifies ring‑first parsing with batch borrowing through two focused
abstractions and an explicit persistence mechanism for in‑flight tokens.

- InputCursor: one reader that drains the ring first, then reads the current
  batch, with precise byte/char accounting and well‑defined drop behavior.
- TokenCapture: one per token, borrow‑first with explicit upgrade points to
  owned utf‑8 or raw bytes, specialized operations for values vs keys.
- In‑Flight persistence: parser‑owned scratch that survives across feeds when an
  iterator ends mid‑token.

The design preserves current external semantics: ring is never borrowed; batch
is borrowed when safe; keys and numbers never fragment; string values may
fragment.

## Objectives
- Separate responsibilities: reading vs token materialization vs parser FSM.
- Prefer borrow: avoid copies when a token lies wholly in the batch without
  decoding; upgrade to owned only when needed.
- Make partial‑across‑feeds explicit via parser‑owned in‑flight state.
- Eliminate ad‑hoc buffer merging and scattered ownership flags.

## Stream States at `next()`
- Ring-first: ring has unread chars → read only from ring; the token is owned
  for its full duration (even if later bytes come from the batch).
- Batch: ring empty and a new `&'src str` is provided → read from batch; borrow
  if the token remains fully in this batch with no decoding; otherwise own.
- No data: ring empty and no batch → need more input or end‑of‑input.

## Core Types

### InputCursor<'src>
A unified reader over ring + batch for the lifetime of one iterator pass.

Responsibilities
- Expose a single peek/advance/scan interface that always prefers ring.
- Maintain both character and byte positions for the batch.
- Update global location (pos/line/col) during parsing without borrowing the
  parser; write back on drop.
- On drop, push unread batch tail into the ring and return the ring to the
  parser.

Sketch API
- `fn peek(&self) -> Peeked` where `Peeked = Empty | EndOfInput | Char(char)`.
- `fn advance(&mut self) -> Option<char>`; updates location and ring/batch cursors.
- `fn advance_while<F>(&mut self, pred: F) -> usize` for tight scans.
- `fn in_ring(&self) -> bool` indicating the next char source.
- `fn batch_offsets(&self) -> (bytes_consumed, total_bytes)`.
- `fn slice_batch(&self, start_byte: usize, end_byte: usize) -> Option<&'src str>`
  and/or `fn borrow_window(&self) -> Option<(&'src str, usize, usize)>`.

Lifecycle
- Constructed per `feed()`/`finish()` by taking the ring via
  `core::mem::take(&mut parser.source)` and holding an optional batch `&'src str`.
- Owns a private `Location { pos, line, col }` copy; writes it back on drop.
- Drop: push `batch[bytes_consumed..]` to the ring; move ring back into the
  parser; commit location.

### TokenCapture<'src>
A per‑token capture with borrow‑first semantics and explicit upgrade points.

Modes
- `Borrowing { start_byte }` – started in the batch, borrowing still possible.
- `OwnedUtf8 { buf: String }` – started in ring or upgraded to owned utf‑8.
- `OwnedRaw { bytes: Vec<u8> }` – surrogate‑preserving raw bytes.

Specialization and flags
- `kind: { Key, StringValue, Number }`
- `started_in_ring: bool` – pins ownership to owned for the token.
- `had_escape: bool` – once true, token must be owned.

Key Operations
- Constructors:
  - `start_in_batch(kind, start_byte)` → `Borrowing { start_byte }`.
  - `start_in_ring(kind)` → `OwnedUtf8 { String::new() }` with `started_in_ring=true`.
- Upgrades:
  - `upgrade_copy_prefix(batch, upto_byte)` – copy `[start_byte..upto]` into
    utf‑8 buffer and switch to owned; use for keys (never fragment).
  - `borrow_prefix_and_switch_to_owned(batch, upto_byte) -> &'src str` – return
    a borrowed prefix and reset the capture to empty owned; use for string
    values on first escape to emit a fragment without duplication.
  - `ensure_raw()` – once, migrate `OwnedUtf8` → `OwnedRaw` and continue.
- Append:
  - `push_char(c)` in owned utf‑8 mode; implicitly upgrades if needed.
  - `push_raw(bytes)` in owned raw mode.
- Finalization:
  - `borrow_slice(batch, end_byte) -> Option<&'src str>` – valid only in
    `Borrowing` mode.
  - `take_owned_utf8(self) -> String` / `take_owned_raw(self) -> Vec<u8>`.

Rules
- Must own when: started in ring, saw an escape (decoded differs), raw mode is
  active, or token crosses feed boundaries.
- May borrow when: still `Borrowing`, the entire token range lies in the same
  batch, and no escape/raw was required.
- Keys never fragment; numbers never fragment; string values may.

### In‑Flight Persistence (parser‑owned)
To survive across feeds and iterator drops.

Data
- `enum Scratch { Text(String), Raw(Vec<u8>) }`
- `enum CaptureKind { Key, StringValue, Number }`
- `struct InFlight<'src> { kind: CaptureKind, scratch: Scratch /*, optional hints */ }`
- `parser.in_flight: Option<InFlight<'src>>`

Iterator Drop contract
- If a token is in progress and capture is still Borrowing:
  - Keys/Numbers: copy current batch slice into `scratch` and store `InFlight`.
  - String values: if a borrowed prefix was already emitted, capture should be
    owned (empty or with suffix); store it. Otherwise, copy current borrowed
    prefix and store.
- Always push unread batch tail into the ring; restore ring and location.

## Event Emission
- Numbers
  - Never fragment. If fully in batch and no upgrade, emit borrowed; otherwise
    emit owned. If batch ends mid‑number, return need‑more‑data and persist
    in‑flight owned scratch.
- Strings (values)
  - Fast‑path borrow: scan until escape/quote; if quote, emit borrowed; if
    escape, emit borrowed prefix via `borrow_prefix_and_switch_to_owned`, then
    decode and append into owned (utf‑8 or raw) for subsequent fragments or
    finalization.
- Property names (keys)
  - Never fragment. Borrow only if the whole key lies within the batch with no
    escape; otherwise own by copying prefix on first escape and continue.
- Ring participation at any point forces the whole token to be owned.

## Byte/Char Accounting
- Cursor tracks `bytes_consumed` (for slicing) and `chars_consumed` (for
  location); parser’s `pos/line/col` reflect chars.
- Compute `upto_byte` before consuming delimiters (backslash/quote) when turning
  a borrowed prefix into an event or copying a prefix to own.
- Add debug asserts to ensure borrowed slices align to UTF‑8 boundaries.

## Surrogate‑Preserving Details
- Keep parser fields for surrogate bookkeeping (pending high surrogate,
  last‑was‑lone‑low) to drive decoding.
- On encountering unpaired/ordered surrogates in values, call `ensure_raw()` and
  write WTF‑8 bytes with `push_raw`; for keys, degrade to U+FFFD as today.

## Integration Plan (Incremental)
1) Introduce `InputCursor` and route lexing through it (keep old fields briefly
   to bridge; copy back location on drop).
2) Introduce `TokenCapture` built over the parser’s existing scratch to ease
   migration; port string value lexing first.
3) Add `borrow_prefix_and_switch_to_owned` for values; eliminate duplicate
   fragment hazards with tests.
4) Add `InFlight` and teach iterator drop to persist partially read tokens for
   keys/numbers/strings; remove legacy `token_start_pos` and similar flags.
5) Port number lexing to `TokenCapture`; delete dual scratch buffers and
   ownership flags (e.g., `token_buffer`, `owned_batch_buffer`, `owned_batch_raw`,
   `token_is_owned`, `string_had_escape`).
6) Integrate surrogate‑preserving via `ensure_raw` and raw appends; reuse
   existing parser decode‑mode options.
7) Remove char→byte rescans; rely on `start_byte + bytes_consumed` only for
   slices; keep char counters for diagnostics.
8) Trim remaining legacy helpers; keep only options, surrogate flags, and
   `initialized_string`.

## Validation Strategy
- Boundary fuzzing: split inputs at every byte across feeds; compare to
  monolithic parsing.
- Borrow/own invariants: no borrowed events when ring has content; borrowed
  when batch‑only and no decoding needed.
- Decode modes: strict, replace‑invalid, surrogate‑preserving; include split and
  reversed surrogate pairs.
- Numbers: signs, zeros, decimals, exponents; enforce no fragments; correct
  need‑more‑data signaling.
- Keys vs values: keys never fragment; values may; verify `is_initial/is_final`.
- Performance: micro‑bench fast strings and mixed tokens; ensure zero‑copy path
  remains hot and no extra allocations occur on happy paths.

## Risks and Mitigations
- API misuse at upgrade points
  - Use distinct operations for keys vs values (`upgrade_copy_prefix` vs
    `borrow_prefix_and_switch_to_owned`). Provide specialized constructors per
    kind to force ownership when starting in ring.
- Lifetime correctness
  - Iterator owns the batch; events borrow from it; cursor borrows the batch
    only during `next()`. Ensure cursor drops after event creation. Avoid any
    `&mut` borrows of parser while cursor is alive.
- Overgrown cursor
  - Keep `InputCursor` focused on reading/position; `TokenCapture` manages
    token materialization; parser holds only FSM and in‑flight.
- Drop‑time copying on large batches
  - Matches current semantics; document that consumers should drain iterators.
- Accounting mistakes
  - Maintain both char and byte counters; assert on UTF‑8 boundaries; centralize
    slicing paths.
- Performance regressions
  - Keep fast paths inlined (`advance_while` when borrowing; `copy_while` only
    when owning). Reuse buffer capacities; consider pooling scratch if needed.

## Appendices

A) Typical String Value Flow (no duplicates)
```
// On opening quote
cap = if cursor.in_ring() { start_in_ring(StringValue) } else { start_in_batch(StringValue, cursor.batch_offsets().0) }
// Scan
while let Char(c) = cursor.peek() {
  match c {
    '"' => { /* compute upto before consuming */ cursor.advance(); break; }
    '\\' => {
      let upto = cursor.batch_offsets().0; // bytes before consuming '\\'
      if let Some((batch, ..)) = cursor.borrow_window() {
        let prefix = cap.borrow_prefix_and_switch_to_owned(batch, upto);
        emit StringBorrowed(prefix, is_initial=first, is_final=false);
      }
      cursor.advance(); // consume '\\'
      decode_escape_into_owned(&mut cap, &mut cursor); // utf‑8 or raw
    }
    _ if c < '\u{20}' => error,
    _ => { cursor.advance_while(|d| d != '"' && d != '\\' && d >= ' '); }
  }
}
// Finalize
if let Some((batch, ..)) = cursor.borrow_window() {
  if let Some(s) = cap.borrow_slice(batch, cursor.batch_offsets().0) {
    emit StringBorrowed(s, is_initial=first, is_final=true);
  } else { emit StringOwnedOrRaw(cap); }
} else { emit StringOwnedOrRaw(cap); }
```

B) Typical Number Flow (never fragments)
```
cap = if cursor.in_ring() { start_in_ring(Number) } else { start_in_batch(Number, cursor.batch_offsets().0) }
scan integral/sign/fraction/exponent via advance/advance_while
if delimiter reached in batch and still borrowing → emit NumberBorrowed
if batch exhausted mid‑number → return need‑more‑data; persist owned scratch in parser.in_flight
if ring involved at all → emit NumberOwned at completion
```

This proposal is intentionally self‑contained and can be implemented in
stages. It consolidates ownership decisions, removes ad‑hoc buffer merging,
keeps the hot path zero‑copy, and makes partial‑across‑feeds behavior explicit
and testable.

