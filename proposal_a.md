# Proposal A: Redesign of the Buffer System (LexInput/TokenCapture)

## Current Flow (What exists today)
- Ring-first: parse from `self.source: Buffer` until empty; fragments are owned.
- Batch-next: then parse from the current `&'src str` feed (aka BatchView), borrowing slices when fully contained in the batch and not escaped.
- Token scratch: uses two owned buffers (`token_buffer` for ring-built prefix, `owned_batch_buffer` for batch-built suffix) and `owned_batch_raw` for surrogate-preserving mode; plus flags like `token_is_owned`, `string_had_escape`.
- Drop handling: on iterator `Drop`, copy any in-flight batch prefix into `token_buffer` and flip to owned; push unread batch tail back into the ring.

This implements the intended A/B behavior:
- A) Ring non-empty → parse ring first → emit owned fragments → then parse batch.
- B) Ring empty + active batch → parse batch → prefer borrowed slices; fall back to owned on escapes or spanning.

## Design Goals
- Centralize ownership switching (borrow vs owned) and source switching (ring vs batch) behind one interface.
- Provide a simple “mark, advance, take” capture for tokens (numbers, strings), with one scratch buffer.
- Keep ring-first semantics and never hand out borrows into the ring; only borrow from batch.
- Preserve current drop semantics for in-flight tokens and unread input.
- Reduce duplicated logic and late-merge of multiple per-token buffers.

## Proposed Abstraction
Introduce a single input/cursor and a per-token capture object.

1) LexInput<'src>
- Owns the ring buffer taken via `core::mem::take(&mut parser.source)` and holds a view into the current batch `&'src str`.
- Presents a unified API to peek, bump, and scan regardless of source.
- Tracks global `pos/line/column`, and both char and byte offsets for the batch.
- On `Drop`, returns the ring to the parser, pushes unread batch tail into the ring, and preserves an in-flight token prefix when needed.

2) TokenCapture<'src>
- Bound to a single token. Starts in BorrowPossible mode when on batch; Owned mode when on ring.
- mark → commit_to_owned → push → take:
  - `mark()` remembers the batch byte offset and global char pos where the token began.
  - `commit_to_owned()` (one-way) copies the batch slice `[mark..cursor]` into an internal `String` and switches to Owned mode. If capturing started on ring, it is already Owned.
  - `push_char/push_str` append into the scratch; automatically implies Owned if not already.
  - `take_borrowed(&LexInput) -> Option<&'src str>` returns a borrowed slice when the entire token range lies within the active batch and we never committed.
  - `take_owned() -> String` returns the owned scratch otherwise.
- Variant for surrogate-preserving: `RawTokenCapture` mirroring the API but backed by `Vec<u8>`.

## Core API (sketch)
- LexInput<'src>:
  - `fn peek(&self) -> PeekedChar`
  - `fn bump(&mut self) -> Option<char>` (updates pos/line/column and ring/batch cursor)
  - `fn skip_while<F>(&mut self, pred: F) -> usize`
  - `fn copy_while_to_owned<F>(&mut self, cap: &mut TokenCapture, pred: F) -> usize`
  - `fn mark_token(&mut self) -> TokenCapture<'src>`
  - `fn eof(&self) -> bool` and `fn end_of_input(&self) -> bool`

- TokenCapture<'src>:
  - `fn mark_here(&mut self, input: &LexInput)`
  - `fn commit_to_owned(&mut self, input: &mut LexInput)`
  - `fn push_char(&mut self, ch: char)` / `fn push_str(&mut self, s: &str)`
  - `fn take_borrowed(&mut self, input: &LexInput) -> Option<&'src str>`
  - `fn take_owned(&mut self) -> String`
  - Internals: `mode: BorrowPossible | Owned`, `mark_bytes`, `cursor_bytes`, `scratch: String`

## Parser Usage Sketch
- Numbers (no fragments):
  - `let mut cap = input.mark_token();`
  - Use `skip_while` when in batch; `copy_while_to_owned` when in ring; if boundary/continuation forces it, call `cap.commit_to_owned()` and keep pushing.
  - End: `if let Some(s) = cap.take_borrowed(&input) { NumberBorrowed(s) } else { NumberOwned(cap.take_owned()) }`.

- Strings (values allow fragments; keys do not):
  - `let mut cap = input.mark_token();`
  - Fast-path: `skip_while(|c| c != '\\' && c != '"')`.
  - On backslash: if value, optionally emit a prefix fragment via `cap.take_borrowed(&input)` when non-empty; then `cap.commit_to_owned(&mut input)` and decode escapes into `cap.push_char(...)` (or into `RawTokenCapture`). For property names, don’t emit partial; just commit and continue.
  - On closing quote: `if let Some(s) = cap.take_borrowed(&input) { StringBorrowed(...) } else { StringOwned(...) }` with the right `is_initial/is_final` flags.

- Structural tokens, booleans, null: advance with `LexInput`; no capture unless needed.

## Drop Behavior
- `impl Drop for LexInput`:
  - If a token was marked and we consumed batch since the mark, copy `[mark..cursor]` into the parser’s scratch and mark the token as owned for continuation.
  - Push unread batch suffix into the ring.
  - Move the ring back into `parser.source`.

## What This Removes
- `BatchView` and `BatchCursor` exposure to parser logic.
- Dual scratch buffers with late-merge; replaced by a single per-token scratch (plus raw variant).
- Scattered checks for “am I in ring or batch?” within each lexing branch.
- Re-scanning `char_indices` to compute slice offsets; we track `mark_bytes`/`cursor_bytes` instead.

## Migration Plan
- Add `parser/input.rs` implementing `LexInput` and `TokenCapture`.
- Change `feed()` to construct an iterator that owns a `LexInput`, calling `core::mem::take(&mut self.source)`.
- Replace `BatchView/BatchCursor` args in lexing functions with `&mut LexInput`.
- Collapse `token_buffer` and `owned_batch_buffer` into a single `scratch: String` inside `TokenCapture`.
- Convert `produce_string/produce_number` to use `TokenCapture`:
  - Start capture, scan fast-path; first escape triggers `commit_to_owned`; finish via borrowed or owned.
- Keep `pos/line/column/parse_state` in the parser; `LexInput` updates positions.
- Keep `ClosedStreamingParser` unchanged; it just builds a `LexInput` with no batch.

## Critique
- Pros:
  - Single place for source and ownership switching; parser becomes a clear state machine over one “input”.
  - Borrow vs owned becomes a simple rule: borrow only from batch if never committed; otherwise owned.
  - Eliminates buffer merging and reduces per-branch complexity.
  - Consolidates tricky `Drop` behavior into `LexInput`.
- Cons:
  - `LexInput` is more complex: manages ring, batch, positions, and capture lifetimes.
  - Surrogate-preserving mode needs a `RawTokenCapture` or a mode bit in `TokenCapture` (additional branching).
  - Requires touching many call sites to remove `BatchView/BatchCursor` and `token_is_owned` logic.
  - Must be precise with char vs byte accounting when slicing borrowed prefixes (store `mark_bytes`/`cursor_bytes`).

## Implementation Notes
- Use byte indices for batch slicing; keep char-based `pos/line/column` for diagnostics.
- Retain the existing ring (`VecDeque<char>`) for now; consider `VecDeque<u8>` as a future improvement.
- Provide helpers for common string-fragment operations (e.g., `emit_borrowed_prefix_if_any`).
- Keep the `LexToken` and public `ParseEvent` shapes; the simplification is internal.

## Critical Review: Risks, Pitfalls, and Mitigations

This proposal meaningfully simplifies the mental model, but it also introduces new risks. Below is a candid critique with concrete mitigations.

1) Persistence Across Feeds (Partial Tokens)
- Problem: `TokenCapture` as sketched is ephemeral. If iteration ends mid-token (end-of-batch), its scratch must persist into the next `feed()`. Today this persistence is implemented with parser-level fields (`token_buffer`, `owned_batch_buffer`, `owned_batch_raw`).
- Risk: If `TokenCapture` owns its scratch and is dropped with the iterator, partial content is lost, or we duplicate responsibility with a parser-level scratch.
- Mitigation: Make `TokenCapture` a lightweight view over parser-owned scratch state: `parser.token_scratch: Option<Scratch>`, where `Scratch` = `Text(String)` or `Raw(Vec<u8>)` plus metadata (`started_on_ring`, `mark_bytes`, `started_pos`). `TokenCapture` borrows mutable access to this scratch during the token’s lifetime; on iterator `Drop`, scratch remains in the parser. This preserves the “single scratch” goal per token while ensuring persistence.

2) Surrogate-Preserving Transitions Mid-String
- Problem: Strings may start as borrow-possible UTF-8 and later require raw bytes (surrogate-preserving) when encountering escapes. The capture must switch modes and migrate buffered data.
- Risk: Mode switching is error-prone if the capture owns buffers and has to merge multiple representations.
- Mitigation: Use a single `Scratch` enum in parser state with `ensure_raw()` that migrates the current text into raw bytes exactly once (like the current `ensure_raw_mode_and_move_buffers`). `TokenCapture` calls this on transition and continues accumulating raw bytes. Borrowed prefixes can still be emitted before the transition.

3) Lifetime and Borrowing Correctness
- Problem: Borrowed string fragments must outlive the returned `ParseEvent`. The batch slice must be held by the iterator, not by `LexInput` or the parser.
- Risk: If `LexInput` owns or drops the batch before the event is consumed, references dangle.
- Mitigation: Keep the batch owned by the iterator struct (as today). `LexInput` only references it. Ensure `LexInput` is dropped before or at the same time as the iterator’s drop, never earlier, and never owns the batch. Events borrow from the iterator’s batch lifetime.

4) Property Name Edge Cases and Drop Semantics
- Problem: Property names never emit partial fragments. If `Drop` happens mid-key, we must preserve the already-read prefix and never have emitted a fragment.
- Risk: If `LexInput` tries to be responsible for “preserve in-flight token”, it needs awareness of parser state (are we inside a key? did we emit a fragment?). That reintroduces coupling.
- Mitigation: Keep parser as the source of truth for state. On `LexInput` drop, just push unread batch remainder to ring. Let the parser decide whether/how to preserve in-flight scratch. This keeps `LexInput` focused on I/O and position tracking.

5) API Misuse Risk (Forgetting to Commit)
- Problem: The correctness of borrow vs owned depends on calling `commit_to_owned()` at the right times (e.g., on first escape or when starting on ring).
- Risk: A missed call can lead to illegal borrowing that spans ring→batch or escapes.
- Mitigation: Provide dedicated constructors/guards:
  - `TokenCapture::for_number(started_on_ring: bool)` forces owned at start if needed.
  - `TokenCapture::for_string_value()` returns a type that exposes `emit_borrowed_prefix_if_any()` and internally flips to owned on first escape.
  - `TokenCapture::for_property_name()` that disables partial emission and always preserves scratch on batch end.
  Use type-level phantom flags or distinct types to reduce misuse.

6) Need-More-Data Signaling and Partial Numbers
- Problem: Numbers cannot be fragmented. We need a clear way to signal “partial token, need more data”.
- Risk: If `LexInput` doesn’t clearly distinguish `Empty` (batch ended) vs `EndOfInput`, we might emit an incomplete token or incorrectly treat a boundary as final.
- Mitigation: Keep the tri-state peek (`Empty`, `Char`, `EndOfInput`) and plumb it through. `TokenCapture` returns no fragment for numbers until a delimiter is seen; parser retains capture state and returns a partial (Eof-with-partial) to the iterator.

7) Position and Byte/Char Accounting
- Problem: We track global position in chars for diagnostics, but slice in bytes for borrowing.
- Risk: Off-by-one errors and mismatched counters lead to panics on slicing.
- Mitigation: Store both `cursor_chars` and `cursor_bytes` in `LexInput`. On each bump, increment both (`+1 char`, `+len_utf8 bytes`). Maintain `mark_bytes` in capture. Add debug assertions that slice boundaries always fall on UTF-8 boundaries.

8) “God Object” Concerns
- Problem: `LexInput` risks becoming a large, hard-to-test object that does too much (ring, batch, positions, drop behavior, marker APIs).
- Mitigation: Split responsibilities:
  - `InputCursor` (peek/bump/skip, pos/line/column, ring+batch cursors)
  - `TokenCapture` (view into parser scratch)
  - Iterator owns the batch and orchestrates drop; parser owns scratch and state. `LexInput` can be a thin facade around `InputCursor` plus references, not a stateful owner.

9) Performance Regressions
- Problem: Additional indirections (capture, scratch enum) and mode checks might add overhead.
- Risk: Throughput regressions on hot paths (fast string copy/scan).
- Mitigation: Keep fast-paths inlined:
  - `skip_while` for batch scanning; no writes when borrowing is viable.
  - `copy_while_to_owned` used only for ring or when committed.
  - Single scratch buffer per token, no merging.
  Benchmark and compare: large unescaped strings fully in one batch; many small values; numbers with and without exponents; decode modes.

10) Integration Churn and Invariants
- Problem: Changing many call sites can introduce subtle bugs.
- Risk: Incorrect handling of `is_initial/is_final`, key/value boundaries, or multi-value mode.
- Mitigation: Stage changes:
  1. Introduce `TokenCapture` over existing buffers; keep `BatchView` and `token_is_owned` for a short period.
  2. Migrate string lexing only; keep number parsing as-is.
  3. Remove `BatchView` exposure and finalize `LexInput`.
  Add property tests and fuzzing at each stage.

## Alternatives Worth Considering

- A) TokenSpan + Scratch
  - Keep a lightweight `TokenSpan { batch_start_byte, end_byte }` for borrowable ranges and a parser-level `Scratch` for owned. Strings: emit borrowed spans until an escape, then switch to owned and continue. This is close to the proposed model but may not need a full `LexInput` abstraction; it layers over existing `BatchView`/`Buffer` until we retire them.

- B) Trait-Based Readers
  - Implement `Reader` for `RingReader` and `BatchReader` with a common interface (`peek`, `bump`, `copy_while`). Use an `EitherReader` while the ring is non-empty, then switch to the batch reader. Add `Capture` independent of the reader type. This keeps responsibilities separated but introduces dynamic dispatch or enum dispatch at hot sites.

- C) Keep BatchView, Simplify Capture Only
  - Leave source switching as-is. Introduce `TokenCapture` over parser scratch and use it to eliminate dual buffers and merging. Smaller change; fewer moving parts; less risk; most of the win.

- D) Cow-Based Event Payloads Internally
  - Use `Cow<'src, str>` for string/number tokens internally (still surface the same `ParseEvent`). Borrow from batch on success; to-owned on escape/boundary. This is conceptually similar but pushes ownership logic into one place. Might still require drop/persistence care.

## Validation Strategy
- Boundary fuzzing: split inputs at every byte boundary across feeds; verify identical event sequences to monolithic parse.
- Borrow/own invariants: assert that when ring has content, no borrowed fragments are produced; when batch-only and no escape, borrowed is produced.
- Decode modes: cover strict Unicode, replace-invalid, surrogate-preserving; include surrogate pairs split across feeds.
- Numbers: exponents, signs, leading zeros; verify “no partial fragments” invariant; ensure need-more-data is signaled correctly.
- Keys vs values: property names never emit partial; values may; verify path transitions and `is_initial/is_final` flags.
- Performance: micro-benchmarks for common cases; ensure zero-copy path is still hot.
