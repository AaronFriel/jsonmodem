Unified Parser Buffer Design (DAB)

Status: proposal
Audience: parser maintainers and contributors
Scope: Replace internal buffer/borrowing mechanics while preserving public events and options

Intent
- Synthesize the strengths of prior proposals (A, B, D) into a single, self‑sufficient design that:
  - Keeps ring‑first then batch borrow‑first behavior.
  - Eliminates scattered flags and duplicated buffers.
  - Uses explicit finalization instead of RAII back‑references.
  - Cleanly preserves in‑flight tokens across feeds.
  - Provides clear, testable APIs for lexing and token capture.

Non‑Goals
- Changing `ParseEvent`, `EventCtx`, or `PathCtx` public contracts.
- Changing JSON grammar or escape semantics.

Terminology
- Parser: `StreamingParserImpl<B>`.
- Ring: `parser::buffer::Buffer` holding carry‑over chars from previous feeds.
- Batch: current `&'src str` provided by `feed_with(...)`.

Design Overview
- InputGuard: unifies ring+batch reads, tracks positions, returns state via finalize().
- TokenScratch: single owned buffer (text or raw bytes) that persists in the Parser.
- TokenCapture: per‑token controller that starts borrowable only when legal, and flips to owned via ensure_owned()/ensure_raw(). Capture borrows the Parser’s TokenScratch.
- Iterator.drop(): minimal, explicit helpers only (preservation and tail push). No hidden RAII mutations.

Invariant Rules
- Source priority: always consume the ring fully before reading from batch.
- Borrowed data: never from ring; only from the active batch.
- Tokens:
  - Numbers never fragment; emit once as borrowed or owned.
  - Strings (values) may fragment; strings (property names) never fragment.
  - Once owned for a token, stays owned until that token completes.

Components

1) InputGuard<'src>
Purpose: Provide unified access to chars across ring→batch and track positions and batch indices.

Construction
- `InputGuard::new(parser, batch: Option<&'src str>, batch_char_start: usize, batch_char_end: usize)`
  - Moves the ring out: `let ring = core::mem::take(&mut parser.source)`.
  - Captures `parser.pos/line/column`.

State
- `ring: Buffer`
- `batch: Option<&'src str>`
- Global position: `pos: usize`, `line: usize`, `col: usize` (char‑based)
- Batch boundaries: `batch_char_start: usize`, `batch_char_end: usize`
- Batch cursors: `bytes_used: usize`, `chars_used: usize`

API
- `peek() -> Option<char>`: ring first, else batch.
- `next() -> Option<char>`: consumes from ring until empty, then batch; updates `pos/line/col`, `bytes_used`, `chars_used`.
- `in_ring() -> bool`: true iff the next char comes from the ring.
- `advance_while<F: Fn(char)->bool>(&mut self, pred) -> usize`: greedily consume while predicate holds; updates counters.
- `mark() -> Mark`: save point for current token (captures `pos` for diagnostics and `bytes_used` for O(1) slicing).
- `slice_from(mark: Mark) -> Option<&'src str>`: return `&batch[mark.bytes_used .. bytes_used]` if the whole token is inside the batch; None otherwise.
- `finish(self) -> Finished` where `Finished { ring: Buffer, bytes_used, pos, line, col }`.

Note: `slice_from` uses byte offsets (O(1)). Global `pos/line/col` remain char‑based for errors.

2) TokenScratch
Purpose: Single owned accumulation buffer, shared across token kinds, persisted in Parser to survive across feeds.

Shape
- `enum TokenScratch { Text(String), Raw(Vec<u8>) }`

Helpers
- `clear()`
- `ensure_raw()` converts Text→Raw by copying existing UTF‑8 bytes; idempotent.
- `push_char(c: char)` (Text mode)
- `push_bytes(bs: &[u8])` (Raw mode)

3) TokenCapture<'src>
Purpose: Token‑local controller for borrow/own decisions.

Shape
- `enum TokenKind { Number, StringValue, PropertyName }`
- `enum Mode { Borrowable, Owned }`
- Fields: `kind`, `mode`, `mark: Mark`, `string_had_escape: bool`, `scratch: &mut TokenScratch` (borrowed from Parser), plus optional hints for backends.

Construction
- `TokenCapture::new(input: &InputGuard<'src>, kind: TokenKind, scratch: &mut TokenScratch)`:
  - `mode = Borrowable` iff `!input.in_ring()` (i.e., starts in batch).
  - Otherwise `mode = Owned`.
  - `mark = input.mark()`.

Operations
- `ensure_owned(input)`: if `mode == Borrowable`, copy `input.slice_from(mark)` into scratch (Text) and set `mode = Owned`.
- `ensure_raw()`: `scratch.ensure_raw()`.
- `push_char(c)`: implies owned; stores into Text.
- `push_bytes(bs)`: implies owned; stores into Raw.
- `borrow_prefix_and_own(input) -> Option<&'src str>`: StringValue only; if `mode == Borrowable` and prefix non‑empty, return borrowed prefix (`slice_from(mark)`), then set `mark = input.mark()` and `mode = Owned`.
- `finish_number(input) -> Payload<'src>`: Borrowed(&str) if still Borrowable; otherwise Owned(TokenScratch::Text(...)).
- `finish_string(input) -> Payload<'src>`: Borrowed(&str) if still Borrowable; otherwise Owned(Text or Raw depending on mode).

Payload
- `enum Payload<'src> { Borrowed(&'src str), Owned(TokenScratch) }`

4) Parser State (minimal, persistent)
- `token_scratch: TokenScratch`
- `token_in_flight: Option<InFlight>` where:
  - `struct InFlight { kind: TokenKind, mode: Mode, mark_bytes: usize, start_pos: usize, string_had_escape: bool }`
- Used to preserve progress across feeds and enable iterator.drop() helpers.

Iterator & Drop Responsibilities

Flow
1) Build `InputGuard` at iterator start.
2) Lex with `&mut InputGuard` and `TokenCapture` borrowing `token_scratch`.
3) On each token start, write `token_in_flight = Some(...)` from `TokenCapture` state; on token completion, set `token_in_flight = None`.
4) After lex loop, finalize InputGuard and write back ring/positions; push unread batch tail `&batch[bytes_used..]` into ring if any.

Drop
- `impl Drop for Iterator` calls two helpers if `token_in_flight.is_some()`:
  - `preserve_in_flight(parser, &batch, bytes_consumed_chars)`: if mode was `Borrowable` and chars consumed since `mark`, copy `&batch[mark_bytes..current_bytes]` into `token_scratch` and set `mode = Owned`.
  - `push_unread_tail(parser, &batch, bytes_used)`; No ring writes happen here if already done at finalize (call site must avoid double‑pushing).

Lexing Usage Patterns

Numbers
- `let mut cap = TokenCapture::new(&input, Number, &mut parser.token_scratch);`
- If `input.in_ring()`, cap starts Owned; else Borrowable.
- Consume digits/points/exponent via `advance_while` and explicit steps.
- On token end: `match cap.finish_number(&input)` → Borrowed or Owned(Text) → map to `LexToken::NumberBorrowed` or `NumberOwned`.
- If batch ends mid‑number while Borrowable: return Eof; iterator.drop() preserves prefix and marks Owned for continuation.

Strings (values)
- `let mut cap = TokenCapture::new(&input, StringValue, &mut parser.token_scratch);`
- Fast path: `advance_while(|c| c != '"' && c != '\\')`.
- On backslash:
  - If any prefix exists and `mode == Borrowable`, `if let Some(prefix) = cap.borrow_prefix_and_own(&input) { emit partial borrowed fragment }`.
  - Decode escape; if surrogate‑preserving requires raw, call `cap.ensure_raw()` and push bytes; else push decoded char.
- On closing quote: `match cap.finish_string(&input)` → Borrowed or Owned(Text/Raw); emit final fragment with `is_final = true`.

Strings (property names)
- Same as values except: do not emit partial fragments. On first backslash, `cap.ensure_owned(&input)` and continue decoding. Emit once on closing quote.

Whitespace/Literals/Structures
- Handled with InputGuard `peek()`/`next()` alone; no capture needed.

Lifetimes and Safety
- Borrowed slices are always `&'src str` from the iterator’s batch; InputGuard never owns the batch.
- TokenCapture never yields references that outlive the iterator; Owned payloads move out via TokenScratch.
- No RAII back‑references: all writes back to Parser happen explicitly on finalize or in iterator.drop() helpers.

Error Handling and EOF
- Invalid chars: detected via InputGuard `peek()` and parser state; report using `pos/line/col` maintained by InputGuard.
- EOF handling matches current behavior (Eof sentinel from lex; final EndOfInput when parser.end_of_input is true and no more input remains).

Performance Considerations
- O(1) borrowed slice slicing via `mark.bytes_used .. input.bytes_used`.
- Maintain an ASCII fast path in `advance_while` by scanning with `as_bytes()` until a non‑ASCII or stop‑char boundary, then fall back to `chars()`.
- Ring remains `VecDeque<char>` initially; future improvement: `VecDeque<u8>` with UTF‑8 decoding for potentially better cache behavior.

Test Plan (behavioral)
- Borrowed vs. owned for numbers and strings fully in batch vs. spanning ring→batch boundaries.
- String partial fragments (values) across multiple feeds; verify no partials for property names.
- Surrogate‑preserving raw mode transitions (switching mid‑string) and integration with backend `new_str_raw_owned`.
- Iterator drop mid‑token preserves prefix and marks token owned for continuation.
- EOF: numbers/strings split across final feed and `finish()`.

Migration Plan
- Phase 1: Implement TokenScratch in Parser; add `ensure_raw()` and refactor existing string path to use it behind a feature flag.
- Phase 2: Add InputGuard (with explicit `finish`); wire numeric lexing to it first; keep current BatchView temporarily for reference tests if necessary.
- Phase 3: Introduce TokenCapture for numbers; delete `token_is_owned` for numbers; validate borrowed vs owned decisions.
- Phase 4: Port strings to TokenCapture; remove `owned_batch_buffer` and `owned_batch_raw`; use TokenScratch only.
- Phase 5: Remove BatchView/BatchCursor and related helpers; centralize iterator finalize + drop helpers.
- Phase 6: Bench; iterate on ASCII fast paths or byte ring if needed.

Why DAB Is Better
- From A/B: keeps unified input + capture, and O(1) slice computation via stored byte offsets.
- From D: removes RAII back‑references, uses explicit finalize, and persists scratch in Parser for robust cross‑feed preservation.
- Consolidates raw/UTF‑8 buffering with a single TokenScratch and explicit ensure_raw().
- Minimal, testable iterator.drop(); no hidden side effects.

Open Questions
- Should numbers proactively flip to owned on any ring participation to simplify partial EOF behavior? (Current: yes.)
- Should TokenCapture track size hints for pre‑allocation when switching to owned? (Probably, as an optimization.)
- Is a byte‑based ring worthwhile for typical workloads? Measure after functional parity.

