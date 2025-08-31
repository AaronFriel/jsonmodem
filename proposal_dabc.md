JSONModem Buffer/Ownership Redesign (DABC)

Status: proposal
Audience: parser maintainers and contributors
Scope: Internal buffer + borrowing model for StreamingParser; public events unchanged

Objective
- Unify and improve prior designs by delivering a self‑contained, implementable plan that:
  - Retains ring‑first, borrow‑first behavior with explicit, minimal APIs.
  - Removes duplicated buffers and scattered flags in the parser.
  - Avoids RAII back‑references in Drop; uses explicit finalize points.
  - Preserves in‑flight tokens across feeds deterministically.
  - Improves testability and performance predictability.

Core Invariants
- Source order: read ring first; when empty, read batch.
- Borrowing: may only borrow from the active batch; never borrow from ring.
- Token rules:
  - Numbers never fragment; emitted as one event (borrowed or owned).
  - Property names never fragment; emitted once on closing quote.
  - String values may fragment; first/last flags maintained.
  - Once a token upgrades to owned, it remains owned until completion.

Design Components

1) InputGuard<'src>
- Purpose: Unify ring+batch reads; track positions and batch byte indices; finalize explicitly.
- Construct: `InputGuard::new(parser: &mut Parser, batch: Option<&'src str>, batch_char_start: usize, batch_char_end: usize)`
  - Moves ring out of the parser via `core::mem::take(&mut parser.source)`.
  - Copies `pos/line/col` locally for updates.
- State:
  - `ring: Buffer`
  - `batch: Option<&'src str>`
  - `pos: usize, line: usize, col: usize` (char‑based)
  - `batch_char_start: usize, batch_char_end: usize` (global char positions)
  - `bytes_used: usize, chars_used: usize` (within batch)
- API:
  - `peek() -> Option<char>`: ring first, else batch.
  - `next() -> Option<char>`: consume one char; updates pos/line/col and batch cursors.
  - `advance_while<F: Fn(char)->bool>(&mut self, pred) -> usize`: greedy scan.
  - `in_ring() -> bool`: true if the next char comes from ring.
  - `mark() -> Mark`: capture current `bytes_used` and global char pos.
  - `slice_from(mark: Mark) -> Option<&'src str>`: O(1) batch slice if fully within current batch; None otherwise.
  - `finish(self) -> Finished`: returns `{ ring, bytes_used, pos, line, col }` to write back explicitly.
- Notes:
  - Slicing is pure byte‑offset based; diagnostics remain char‑based.
  - ASCII fast path: `advance_while` may scan `batch.as_bytes()` until a non‑ASCII or stop char, then fall back to `chars()`.

2) TokenScratch
- Purpose: Single owned scratch per token, persisted in the parser across feeds.
- Shape: `enum TokenScratch { Text(String), Raw(Vec<u8>) }`
- Methods:
  - `clear()`
  - `ensure_raw()` (Text→Raw, copy existing UTF‑8 bytes; idempotent)
  - `push_char(c: char)` (Text)
  - `push_bytes(bs: &[u8])` (Raw)
  - `take_text() -> String` / `take_raw() -> Vec<u8>`

3) TokenCapture<'src>
- Purpose: Token‑local controller for borrow/own decisions and operations.
- Types:
  - `enum TokenKind { Number, StringValue, PropertyName }`
  - `enum Mode { Borrowable, Owned }`
  - `struct TokenCapture<'src> { kind, mode, mark: Mark, string_had_escape: bool, scratch: &'src mut TokenScratch, size_hint: Option<usize> }`
- Construct:
  - `TokenCapture::new(input, kind, scratch, size_hint)`
    - `mode=Borrowable` iff `!input.in_ring()`; else `Owned`.
    - `mark = input.mark()`
- Ops:
  - `ensure_owned(input)`: if Borrowable, copy `input.slice_from(mark)` into Text scratch; `mode=Owned`.
  - `ensure_raw()`: `scratch.ensure_raw()`.
  - `push_char(c)`: implies Owned; write to Text scratch.
  - `push_bytes(bs)`: implies Owned; write to Raw scratch.
  - `borrow_prefix_and_own(input) -> Option<&'src str>`: StringValue only; if Borrowable and prefix non‑empty, return slice, then set `mark = input.mark()` and `mode=Owned`.
  - `finish_number(input) -> Payload<'src>`: Borrowed(&str) if Borrowable; else Owned(Text).
  - `finish_string(input) -> Payload<'src>`: Borrowed(&str) if Borrowable; else Owned(Text/Raw).
- Payload:
  - `enum Payload<'src> { Borrowed(&'src str), Owned(TokenScratch) }`

4) Minimal Parser‑Resident Token State
- `token_scratch: TokenScratch`
- `token_in_flight: Option<InFlight>` where `struct InFlight { kind: TokenKind, mode: Mode, mark_bytes: usize, start_pos: usize, string_had_escape: bool }`
- Purpose: Persist progress across feeds; enable iterator.drop() preservation without keeping a live TokenCapture.

Iterator Integration

Event Loop (simplified)
1) Build `InputGuard` with the active batch and take the ring.
2) For a token start, create `TokenCapture` borrowing `token_scratch` and record `token_in_flight`.
3) Use `peek/next/advance_while` to drive lexing; on escapes or ring participation, call `borrow_prefix_and_own` (values only) and/or `ensure_owned`/`ensure_raw`.
4) Finish token via `finish_*` returning `Payload`; map to `LexToken` and then to `ParseEvent` with the backend.
5) After loop, finalize InputGuard and write ring/positions back; push unread tail `&batch[bytes_used..]` into ring if any.
6) Clear `token_in_flight` when a token finishes; otherwise it remains set.

Iterator Drop Helpers
- If `token_in_flight.is_some()` and mode was Borrowable with progress:
  - `preserve_in_flight(parser, &batch, bytes_used)`: copy `&batch[mark_bytes..bytes_used]` into `token_scratch` and set mode=Owned so the next call continues in owned mode.
- Push unread tail `&batch[bytes_used..]` into ring if finalize wasn’t reached.
- No other RAII side effects.

Lexing Recipes

Numbers
- Start: `let mut cap = TokenCapture::new(&input, Number, &mut token_scratch, size_hint)`; `size_hint` optional (e.g., for integer fast‑path pre‑allocation).
- Rule: any ring participation → Owned; else Borrowable.
- Consume digits/point/exp via `advance_while` and targeted steps.
- End: `finish_number(&input)`.
- Partial batch depletion: return Eof; drop helper preserves prefix and flips to Owned.

Strings (values)
- Start after opening quote.
- Fast path: `advance_while(|c| c != '"' && c != '\\')`.
- On backslash:
  - `if let Some(prefix) = cap.borrow_prefix_and_own(&input) { emit borrowed fragment }`.
  - Decode escape; if surrogate‑preserving, `cap.ensure_raw()` and write WTF‑8 bytes; else push decoded char.
- On closing quote: `finish_string(&input)` and emit final fragment with `is_final=true`.

Strings (property names)
- Same as values, but never emit partials; on first backslash `cap.ensure_owned(&input)` and continue; emit once on closing quote.

Whitespace/Literals/Punctuators
- Use InputGuard directly; literals may use a small temporary `ExpectedLiteralBuffer` and require no capture.

Lifetimes and Safety
- Borrowed slices are always from the iterator’s batch lifetime `'src` and only constructed by `InputGuard::slice_from`.
- TokenCapture never returns references that outlive the iterator; Owned payloads move via `TokenScratch`.
- InputGuard owns the ring temporarily; all write‑backs are explicit via `finish`.

Performance Notes
- Borrowed slices computed in O(1) via stored byte offsets.
- Maintain ASCII fast‑paths; benchmark impact of switching ring to bytes as a later optimization.
- Size hints: TokenCapture accepts an optional `size_hint` to reduce reallocations when switching to owned (e.g., long integers or strings with known partial length).

Error/EOF Behavior
- Invalid input handled via parser FSM using `peek/next` and `pos/line/col` from InputGuard.
- Eof token used to request more data; EndOfInput when parser is closed and sources exhausted.
- Surrogate handling unchanged; Raw mode is contained in TokenScratch.

Migration Plan
- Phase 1: Introduce `TokenScratch` and helpers (`ensure_raw`, `take_*`); refactor existing string path to use them internally behind a feature flag.
- Phase 2: Add `InputGuard` with `finish`; migrate number lexing first; keep BatchView temporarily for validation.
- Phase 3: Introduce `TokenCapture` for numbers and strings; delete `token_is_owned`, `owned_batch_buffer`, `owned_batch_raw` paths.
- Phase 4: Remove `BatchView`/`BatchCursor`; wire iterator finalize + drop helpers.
- Phase 5: Bench and add ASCII fast‑paths; evaluate byte‑ring.

Rationale: Addresses Prior Critiques
- No Drop aliasing: explicit `finish()` replaces RAII back‑references.
- Clear persistence: in‑flight token state lives in Parser; TokenCapture is a thin orchestrator.
- Single scratch: `TokenScratch` replaces three separate buffers and their merge logic.
- Local decisions: borrow vs owned belongs to the token, not global flags.
- O(1) slicing: byte offsets from mark → cursor; no repeated `char_indices`.
- Deterministic iterator Drop: only two small helpers; no hidden state changes.

Open Questions
- Do we preemptively own large tokens (e.g., size_hint over a threshold) to reduce partial emissions? Default: keep borrow‑first, own on demand.
- Should InputGuard expose a byte‑wise `advance_while_ascii` to avoid `chars()` overhead in common cases? Likely yes, as an optimization after parity.
- Should we pool TokenScratch capacity across tokens to reduce churn? Easy follow‑up with a small allocator.

