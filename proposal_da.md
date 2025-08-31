Unified Buffer Redesign (Proposals A + D)

Status: proposal
Audience: parser maintainers and contributors

Intent
- Merge the best ideas from Proposal A (LexInput/TokenCapture with RAII) and Proposal D (explicit finalize; parser‑resident scratch) into a design that:
  - Keeps ring‑first, borrow‑first semantics simple and explicit.
  - Removes duplicated owned buffers and scattered flags.
  - Has well‑scoped ownership/Drop responsibilities without borrow‑checker gymnastics.
  - Preserves public behavior and event shapes.

Summary
- Introduce a small InputGuard that unifies ring + batch reading and tracks positions, but uses explicit finalize() instead of Drop to write state back into the parser.
- Introduce a single TokenScratch enum for owned accumulation, shared between numbers and strings, including raw byte mode for surrogate preservation.
- Introduce a TokenCapture typed by token kind that starts borrowable only when legal, and flips to owned via ensure_owned() or ensure_raw(). The capture is a thin controller over parser‑resident scratch so partial tokens survive across feeds.
- Keep iterator.drop() responsible only for (a) preserving in‑flight progress using parser state, and (b) pushing unread batch tail into the ring. No back‑references from Drop to the parser through hidden RAII.

Core Principles
- Source priority: always read ring first; only when ring is empty read from batch.
- Borrowing scope: borrowed fragments are a sub‑slice of the active batch; nothing borrows from the ring.
- Token locality: borrow/own decision is local to a token via TokenCapture; once owned, it stays owned for that token.
- Partial policy: value strings may emit partial fragments; property names never do; numbers never fragment.
- Predictable Drop: iterator.drop() has explicit helpers; no hidden state changes in Drop of cursors.

Components

1) InputGuard<'src>
- Purpose: Provide a unified read API over ring + batch and track positions.
- Construction: `InputGuard::new(parser: &mut Parser, batch: Option<&'src str>, batch_char_start: usize, batch_char_end: usize)` moves the ring out of the parser via `core::mem::take(&mut parser.source)`.
- Read API:
  - `peek() -> Option<char>`
  - `next() -> Option<char>` updates `pos/line/col`; consumes from ring until empty, then from batch
  - `skip_while(pred) -> usize`
  - `copy_while_to_owned(pred, dst: &mut impl PushChar) -> usize`
- Borrow API:
  - `mark() -> Mark`  // captures current global char position and current batch byte/char offsets
  - `slice_from(mark: Mark) -> Option<&'src str>` // only if start..end lies wholly within batch
- Finalize (explicit):
  - `fn finish(self) -> Finished` where
    - `Finished { ring: Buffer, bytes_used: usize, pos: usize, line: usize, col: usize }`
    - Caller writes ring and positions back to the parser and pushes `&batch[bytes_used..]` into the ring if needed.

2) TokenScratch
- Purpose: Single owned scratch for all tokens.
- Shape:
  - `enum TokenScratch { Text(String), Raw(Vec<u8>) }`
  - Methods: `clear()`, `push_char(char)` (Text), `push_bytes(&[u8])` (Raw), `ensure_raw()` migrates Text→Raw by copying accumulated UTF‑8 bytes, idempotent.

3) TokenCapture<'src>
- Purpose: Token‑local controller for borrow/own decisions and operations.
- Shape:
  - `enum TokenKind { Number, StringValue, PropertyName }`
  - `enum Mode { Borrowable, Owned }`
  - Fields: `kind: TokenKind`, `mode: Mode`, `mark: Mark`, `string_had_escape: bool` (for strings), references to parser’s `TokenScratch`.
- Construction:
  - `fn new(input: &InputGuard<'src>, kind: TokenKind) -> Self`
    - Starts `Borrowable` only if ring is empty at mark time; otherwise `Owned`.
- Operations:
  - `ensure_owned(input: &mut InputGuard)`; copies the batch slice `[mark..now]` into `TokenScratch::Text` and flips to Owned.
  - `ensure_raw()`; flips scratch to Raw and migrates accumulated text bytes.
  - `push_char(c)`; implies Owned if not already; appends to scratch (Text) or after ensure_raw.
  - `push_while_from_input(input, pred)`; greedy copy from source; borrowing path advances marks only.
  - `borrow_prefix_and_own(input)` for StringValue only; returns `Option<&'src str>` for the prefix before switching to Owned. For PropertyName, this is a no‑op (None).
  - `finish_number(input) -> Payload<'src>`; never fragments.
  - `finish_string(input) -> (Option<&'src str>, Option<TokenScratch>)`; returns borrowed if still Borrowable; else owned scratch. The Option pair accounts for partial emission flags controlled by the parser.

4) LexToken and Events (unchanged surface)
- LexToken remains a borrow‑or‑own payload chooser for strings and numbers; parser maps these to `ParseEvent` via `EventCtx`.

Resolved Concerns From A + D

- RAII Drop aliasing: Replaced with explicit finalize on InputGuard; all writes back to parser happen in the iterator after lexing.
- Persistence of partial tokens: Scratch lives in the parser; TokenCapture only orchestrates it, so iterator drop or additional feeds do not lose progress.
- Borrow slicing correctness: InputGuard holds batch and `[char_start, char_end]` and does the same O(n) char→byte slicing as today (no false O(1) claims). Fast‑pathing ASCII runs can be added later.
- Property name partials: TokenKind gates `borrow_prefix_and_own` to None for PropertyName.
- Raw vs UTF‑8 switching: TokenScratch::ensure_raw() mirrors ensure_raw_mode_and_move_buffers and is reusable for strings.
- Iterator drop responsibilities: Stay in iterator (as today), but call two tiny helpers: `preserve_in_flight(parser, &batch, consumed_chars)` and `push_unread_tail(parser, &batch, bytes_used)`.
- Ring semantics: Still ring‑first; borrowing never targets ring; only owned payloads are built when the ring participates.

API Sketches

InputGuard
```rust
struct InputGuard<'src> {
  ring: Buffer,
  batch: Option<&'src str>,
  // global position
  pos: usize, line: usize, col: usize,
  // batch boundaries
  batch_char_start: usize, batch_char_end: usize,
  // batch cursors
  bytes_used: usize, chars_used: usize,
}

impl<'src> InputGuard<'src> {
  fn new(p: &mut Parser, batch: Option<&'src str>, start: usize, end: usize) -> Self;
  fn peek(&self) -> Option<char>;
  fn next(&mut self) -> Option<char>;
  fn skip_while<F: Fn(char)->bool>(&mut self, pred: F) -> usize;
  fn copy_while_to_owned<F: Fn(char)->bool>(&mut self, dst: &mut impl PushChar, pred: F) -> usize;
  fn mark(&self) -> Mark;
  fn slice_from(&self, m: Mark) -> Option<&'src str>;
  fn finish(self) -> Finished; // { ring, bytes_used, pos, line, col }
}
```

TokenScratch
```rust
enum TokenScratch { Text(String), Raw(Vec<u8>) }
impl TokenScratch {
  fn clear(&mut self);
  fn ensure_raw(&mut self); // migrate Text→Raw if needed
  fn push_char(&mut self, c: char);    // Text mode
  fn push_bytes(&mut self, b: &[u8]);  // Raw mode
}
```

TokenCapture
```rust
enum TokenKind { Number, StringValue, PropertyName }
enum Mode { Borrowable, Owned }

struct TokenCapture<'src> {
  kind: TokenKind,
  mode: Mode,
  mark: Mark,
  string_had_escape: bool,
  scratch: &'src mut TokenScratch, // actually lives in parser, mut-borrowed for the token
}

impl<'src> TokenCapture<'src> {
  fn new(input: &InputGuard<'src>, kind: TokenKind, scratch: &'src mut TokenScratch) -> Self;
  fn ensure_owned(&mut self, input: &InputGuard<'src>);
  fn ensure_raw(&mut self);
  fn push_char(&mut self, c: char);
  fn push_while_from_input<F: Fn(char)->bool>(&mut self, input: &mut InputGuard<'src>, pred: F) -> usize;
  fn borrow_prefix_and_own(&mut self, input: &InputGuard<'src>) -> Option<&'src str>; // StringValue only
  fn finish_number(&mut self, input: &InputGuard<'src>) -> Payload<'src>;
  fn finish_string(&mut self, input: &InputGuard<'src>) -> Payload<'src>;
}

enum Payload<'src> { Borrowed(&'src str), Owned(TokenScratch) }
```

Lexing Flows

Numbers
- Start capture with TokenKind::Number.
- If ring active at start → Owned; else Borrowable.
- Greedy scan digits/point/exponent using `skip_while` in batch; if ring participates or batch ends mid‑token, `ensure_owned` and use `copy_while_to_owned`.
- Finish: Borrowed if still Borrowable; otherwise Owned(TokenScratch::Text).

Strings (values)
- Start capture with TokenKind::StringValue at first char after `"`.
- Fast path: `skip_while(|c| c != '\\' && c != '"')`.
- On backslash:
  - `borrow_prefix_and_own` to emit borrowed fragment if any; flip to Owned.
  - Decode escapes; for surrogate‑preserving, call `ensure_raw` and write decoded bytes into Raw.
- On `"`:
  - Finish via `finish_string`: Borrowed if still Borrowable; else Owned.
- Emit `is_initial`/`is_final` as today; property names never emit partials.

Strings (property names)
- Same as values except: no partial emission; on first backslash, do not emit; just `ensure_owned` and continue.

Iterator Integration
- Before loop: `let mut inp = InputGuard::new(self, batch, batch_char_start, batch_char_end)`.
- Loop: `self.lex_step(&mut inp)` uses TokenCapture + TokenScratch.
- After loop: `let fin = inp.finish(); self.source = fin.ring; self.pos = fin.pos; self.line = fin.line; self.column = fin.col; if let Some(b) = batch { if fin.bytes_used < b.len() { self.source.push(&b[fin.bytes_used..]); } }`
- On iterator.drop():
  - `preserve_in_flight(self, batch, consumed_chars)` using parser fields `token_start_pos`, `mode`, and `&mut TokenScratch`.
  - No ring/batch juggling in Drop; finalize did that.

Error Handling and EOF
- `peek()` returns None when both ring and batch are drained; parser then returns Eof or waits for more data depending on `end_of_input`.
- Invalid chars and truncated escapes behave as today; positions come from InputGuard.

Migration Plan
- Phase 1: Introduce TokenScratch; keep current buffers but adapt them to TokenScratch internally; add `ensure_raw()` helper mirroring existing behavior.
- Phase 2: Introduce InputGuard with explicit finish; implement for numbers only; gate via internal `cfg`.
- Phase 3: Port strings; delete `owned_batch_buffer`/`owned_batch_raw`; unify on TokenScratch.
- Phase 4: Remove `reading_from_source` branches by using `TokenCapture` mode; simplify lex state code paths.
- Phase 5: Clean up: drop BatchView/BatchCursor; consolidate iterator.drop() to call preservation helper; add benches and ASCII fast‑paths if needed.

Risks and Mitigations
- Complexity concentration: InputGuard centralizes many responsibilities. Mitigate by small pure helpers and unit tests (slicing, position tracking, ASCII runs).
- Lifetime mistakes: Keep batch owned by iterator; only hand out `&'src str` from its lifetime; ensure InputGuard is finalized before events are consumed externally.
- Performance regressions: Stage work; benchmark at each phase; add ASCII fast‑paths only when numbers show regression.

Why This Is Better Than A or D Alone
- From A we keep the clear split of Input + Capture and a single per‑token scratch; from D we adopt explicit finalize (no back‑refs in Drop) and parser‑resident scratch for persistence.
- Ownership and borrowing rules are explicit, local, and testable.
- Iterator drop is minimal and deterministic; no hidden RAII mutations.
- The resulting parser code reads linearly: start capture, fast‑path scan, flip to owned on conditions, finalize borrowed/owned, emit.

