JSONModem Parser Buffer Redesign (Draft D)

Status: proposal
Audience: parser maintainers and contributors

Goals
- Reduce cognitive overhead around “where are we reading from?” and “what do we own?”.
- Make borrow-first behavior explicit and local, with a narrow API.
- Preserve existing external behavior (events and options) with fewer moving parts.
- Integrate a clean Drop story for partially-read tokens and unread batch tail.

Summary
- Introduce a small, self-contained input abstraction that presents characters from two sources in a fixed priority order: ring first, then the current batch.
- Pair it with a token capture type that starts in borrow mode and escalates to owned on demand (escapes, cross-chunk, or ring involvement).
- Use RAII guards to manage: (a) temporary ownership of the ring via `core::mem::take`, and (b) automatic push-back of unread batch bytes on drop.
- Collapse the trio of scratch buffers (`token_buffer`, `owned_batch_buffer`, `owned_batch_raw`) into a single `TokenBuf` enum that clearly expresses the output representation.

Critical Review (Why this could fail)

- RAII Drop needs &mut parser: The sketch has `Input::drop` push unread batch tail to the ring and write positions back to the parser. In safe Rust, `Drop` can’t reach back into the parser unless `Input` holds a mutable reference to it for the entire iteration, which would alias with `&mut self` used elsewhere. That’s not viable without unsafe interior pointers.
  - Fix: make finalization explicit. Replace the Drop side‑effect with `fn finish(self) -> (Buffer, BatchTail)`, and in `next_event_with...` write `parser.source = ring` and push the tail explicitly. Positions (pos/line/col) should also be written back explicitly from fields carried by `Input`.

- In‑flight token preservation on iterator drop: The proposal suggests moving the “copy consumed prefix and flip to owned” responsibility into `Capture`. But `Capture` is ephemeral inside the lexing function; if the iterator drops mid‑token outside of `Capture`’s scope, nothing copies the prefix.
  - Fix (A): persist minimal “in‑flight capture” state in the parser (token_start_mark, token_mode, owned_prefix TokenBuf) so the iterator drop can always preserve progress even without a live `Capture` value.
  - Fix (B): or, keep the current parser fields (`token_start_pos`, `string_had_escape`, etc.) and expose a helper `preserve_in_flight(&mut parser, &Input)` that performs the copy at iterator drop. This keeps preservation localized and avoids storing `Capture` across calls.

- Missing batch boundary metadata: The `Input` sketch only tracked bytes/char consumed but not the batch’s global char span. To decide if a mark-to-now range is fully inside the batch, we need `batch_char_start` and `batch_char_end` (global char positions), same as today’s `BatchView`.
  - Fix: add `batch_char_start`/`batch_char_end` fields to `Input` and use them in `slice_from(mark)`.

- Char→byte mapping complexity: `slice_from` must translate char indices to byte offsets. Without additional indices, this requires scanning `char_indices()` to find start and end byte offsets. That’s O(n) in token length (same as today’s `slice_chars`) but contradicts the hand‑wavy “O(1)” comment. It’s acceptable, but the doc should not over‑promise.
  - Fix: remove the O(1) claim. Optionally cache a rolling ASCII fast‑path later if needed.

- Borrowed prefix on escape only when borrowable: The split‑prefix logic must not emit a borrowed fragment if any part of the token started in the ring. The sketch implies this via `Mode::Borrowable`, but we must explicitly ensure `Capture` only starts in `Borrowable` when the token began while consuming from the batch and before any ring consumption.
  - Fix: define `Capture::new(input)` to inspect `input.ring_is_active_at_mark` and start in `Mode::Owned` when necessary.

- Property name partials: The proposal says property names do not emit partial fragments, but the API examples don’t prevent accidentally calling `split_prefix_and_own()` in that state.
  - Fix: add specialized capture types (or a flag) so `split_prefix_and_own` is a no‑op for property names; enforce in the parser state machine.

- Raw vs UTF‑8 switching: Real code already has `ensure_raw_mode_and_move_buffers` to convert text accumulation to raw bytes when surrogate preservation is required. The proposal’s `TokenBuf` must preserve this behavior; otherwise, raw mode will drop already‑accumulated text.
  - Fix: keep a helper on `Capture` mirroring `ensure_raw_mode_and_move_buffers` that migrates `Text(String)` into `Raw(Vec<u8>)` in place.

- Lifetime plumbing: Borrowed fragments point into the batch `&'src str`. If we wrap input access behind `Input`, it must hand out slices that borrow the passed batch, not anything internal. That’s fine, but the iterator’s type must continue to bind `'src` to the `feed` batch lifetime, as it does today.
  - Fix: `Input::slice_from` returns `&'src str` derived from the batch argument captured at construction.

- Iterator/Drop division of labor: Today, `StreamingParserIteratorWith::drop` both preserves an in‑flight token prefix and pushes unread tail into the ring. Moving those into `Input::drop` would fight the borrow checker. We should keep the iterator `drop` and have it call small, testable helpers instead of burying logic inside RAII.
  - Fix: provide two helpers: `preserve_in_flight(parser, &input)` and `push_unread_tail(parser, &input)` and call them from iterator `drop`.

- Scope/size of refactor: This is a large refactor that touches string and number lexing, iterator lifetimes, and drop semantics. The migration plan must allow incremental adoption and back‑out.
  - Fix: keep old and new paths under a feature flag or a `cfg(test)` switch during migration; port numbers first.

Design Adjustments (incorporating fixes)

- Replace RAII drop with explicit finalize: `Input` exposes `finish(self) -> (Buffer, usize /*bytes_used*/)`; the caller writes the ring back and pushes `&batch[bytes_used..]` to the ring if not empty. Positions are also returned or readable from `Input` fields for writing back to the parser.

- Minimal in‑flight preservation, parser‑resident: Keep in parser: `token_start_pos: Option<usize>`, `token_mode: Borrowable|Owned`, `string_had_escape: bool`, and a single `TokenBuf` for any owned prefix. Iterator `drop` calls `preserve_in_flight(parser, &input)` if `token_start_pos.is_some()`.

- Input fields (complete):
  - `ring: Buffer`
  - `batch: Option<&'src str>`
  - `batch_char_start: usize`, `batch_char_end: usize` (global char positions)
  - `bytes_in_batch: usize`, `chars_in_batch: usize`
  - `pos: usize`, `line: usize`, `col: usize`

- Capture API guardrails:
  - `Capture::new(input, kind: TokenKind)` where `TokenKind` is String|Number|PropertyName; starts as `Borrowable` only if batch‑reading at start.
  - `split_prefix_and_own` returns `None` for PropertyName kind.
  - `ensure_raw_mode` migrates `Text`→`Raw` while preserving accumulated content.

- Borrow slicing wording: Clarify that `slice_from(mark)` performs `char_indices` scans to locate byte boundaries (O(n) in token length), same as the current implementation’s `slice_chars`.

Revised Pseudocode Touchpoints

- Iterator flow ends with explicit finalization rather than relying on Drop:

```rust
let mut inp = Input::from_parser(self, batch);
// ... lex loop using &mut self and &mut inp ...
let (ring, bytes_used, pos, line, col) = inp.finish();
self.source = ring;
self.pos = pos; self.line = line; self.column = col;
if let Some(b) = batch { if bytes_used < b.len() { self.source.push(&b[bytes_used..]); } }
```

- Iterator Drop calls a helper when mid‑token:

```rust
impl Drop for StreamingParserIteratorWith<'_, 'src, B> {
    fn drop(&mut self) {
        // After we’ve finished the event loop and finalized Input
        if let Some(start) = self.parser.token_start_pos { preserve_in_flight(self.parser, &self.batch, self.cursor) }
    }
}
```

Migration Plan (updated)
- Step 1: Introduce `TokenBuf` and migration helpers (`ensure_raw_mode`). Keep existing buffers; add shims to convert between them during development.
- Step 2: Introduce `Input` without RAII Drop; begin by routing only number lexing through `Input` and `Capture`.
- Step 3: Update iterator `drop` to call `preserve_in_flight` and `push_unread_tail` helpers that use `Input` metadata; delete the old ad‑hoc copying code.
- Step 4: Port string lexing; remove `owned_batch_buffer`/`owned_batch_raw`; switch to `TokenBuf` + `ensure_raw_mode`.
- Step 5: Remove `reading_from_source` branches; `Capture`/`Input` encapsulate the decision.
- Step 6: Regressions/benchmarks; add ASCII fast‑path improvements if needed.

Observed States When Iterating
- A: Ring has data → must consume it first; resulting fragments are owned.
- B: Ring empty, batch non-empty → read directly from batch; prefer borrowed fragments.
- C: Ring empty, batch empty → need more data or end-of-input.

Problems With Current Shape
- Mode tracking is spread out: `token_is_owned`, `string_had_escape`, separate ring/batch accumulators, and implicit checks like `reading_from_source(batch)`.
- Owned accumulation is split between `token_buffer` (for ring) and `owned_batch_buffer`/`owned_batch_raw` (for batch), forcing branches on every push.
- Borrow decisions are reconstructed late from scattered state (`token_start_pos`, `batch_cursor`, ring emptiness), making edge cases (escape, split numbers) hard to reason about.
- Drop responsibilities (preserve in-flight, push unread tail) are implemented in the iterator, entangling parsing with ownership mechanics.

Design Overview
- Two orthogonal pieces:
  1) Dual source reader: the one place that knows how to yield chars from ring or batch.
  2) Token capture: the one place that knows whether the current token can be borrowed or must be owned, and how to produce the payload.

API Sketch

1) Dual source input

```rust
/// Drives consumption of input in this order: ring → batch.
/// Owns the ring for the iterator’s lifetime via take() and returns it on Drop.
struct Input<'src> {
    // Sources
    ring: Buffer,               // moved out of parser with mem::take
    batch: Option<&'src str>,   // view of the current feed

    // Cursors
    bytes_in_batch: usize,      // bytes consumed inside batch
    chars_in_batch: usize,      // chars consumed inside batch

    // Positions for diagnostics
    pos: usize,
    line: usize,
    col: usize,
}

impl<'src> Input<'src> {
    fn from_parser(parser: &mut StreamingParserImpl<..>, batch: Option<&'src str>) -> Self;
    fn peek(&self) -> Option<char>;           // ring first, then batch
    fn next(&mut self) -> Option<char>;       // advances pos/line/col, ring→batch

    // Borrow windows relative to this batch only
    fn mark(&self) -> Mark;                   // mark current global char position
    fn slice_from(&self, m: Mark) -> Option<&'src str>; // only if entirely inside batch

    // Owned copying helpers
    fn copy_while<F>(&mut self, out: &mut impl PushChar, pred: F) -> usize
    where F: Fn(char) -> bool;
}

struct Mark(usize); // global char index; stable across ring/batch

impl Drop for Input<'_> {
    fn drop(&mut self) {
        // Push unread batch remainder into ring
        if let Some(b) = self.batch {
            if self.bytes_in_batch < b.len() {
                let tail = &b[self.bytes_in_batch..];
                self.ring.push(tail);
            }
        }
        // Move ring back into parser
        // (parser.source = mem::replace(&mut self.ring, Buffer::new()))
    }
}
```

Notes
- `Input::from_parser` uses `core::mem::take(&mut parser.source)` to take temporary ownership of the ring.
- Positions (pos/line/col) live in `Input` while iterating; on guard drop, they’re written back to the parser.
- Borrowed slices always come from the active `batch` via `slice_from(mark)`; nothing borrowed from ring.

2) Token capture

```rust
/// Result payloads the lexer can produce without caring about Buffer internals.
enum Payload<'src> {
    Borrowed(&'src str),
    Owned(TokenBuf),
}

/// Single scratch buffer for owned payloads.
enum TokenBuf {
    Text(String),        // decoded UTF‑8 string / number lexeme
    Raw(Vec<u8>),        // raw bytes for WTF‑8 / surrogate-preserving mode
}

trait PushChar { fn push_char(&mut self, c: char); }
impl PushChar for String { fn push_char(&mut self, c: char) { self.push(c) } }

/// Borrow‑first accumulator with explicit “owning” transition.
struct Capture<'src> {
    input_mark: Mark,        // where this token starts
    mode: Mode<'src>,
}

enum Mode<'src> { Borrowable, Owned(TokenBuf), }

impl<'src> Capture<'src> {
    fn new(input: &Input<'src>) -> Self;            // record start mark
    fn ensure_owned(&mut self) -> &mut TokenBuf;    // flip Borrowable→Owned on first call
    fn push_char(&mut self, input: &mut Input<'src>, c: char); // helper for lexers
    fn push_from_input_while<F>(&mut self, input: &mut Input<'src>, pred: F) -> usize
        where F: Fn(char) -> bool;

    // Finalize: return best representation available.
    fn finish_text(self, input: &Input<'src>) -> Payload<'src>;
    fn finish_number(self, input: &Input<'src>) -> Payload<'src>; // number: no fragments

    // String-specific: split into prefix fragment and continue owning.
    fn split_prefix_and_own(&mut self, input: &Input<'src>) -> Option<&'src str>;
}
```

Rules enforced by Capture
- Start in `Borrowable` if the token begins while reading from the batch. If the ring had any unread data at token start, start directly in `Owned`.
- Transition to `Owned` when:
  - string escape is encountered (decoded content differs from source),
  - the token crosses ring→batch boundary, or
  - lexing requires buffering (numbers spanning feeds, property names with escapes).
- For strings, `split_prefix_and_own()` emits a borrowed prefix (if any) and switches to `Owned` for the remainder.
- For numbers, `finish_number` never emits fragments; if token is not fully in batch, commit to `Owned`.

Iterator Flow (simplified)

```rust
fn next(&mut self) -> Option<Result<ParseEvent<'src, B>, ParserError<B>>> {
    // 1) Hold ring+batch via guard
    let mut inp = Input::from_parser(self, self.active_batch);

    loop {
        match self.lex_step(&mut inp)? { // produces tokens using Capture
            Some(evt) => return Some(Ok(evt)),
            None if self.partial_lex || self.end_of_input || inp.peek().is_none() => break,
            None => continue,
        }
    }

    None
} // Drop of `inp` pushes unread batch bytes to ring and restores positions
```

String Handling
- Start `Capture` at the first codepoint after `"`.
- Fast-path: copy consecutive non-escape, non-quote chars via `push_from_input_while`.
- On `\` escape:
  - Call `split_prefix_and_own()`; if it returns Some(prefix), emit a partial borrowed fragment immediately.
  - `ensure_owned()` and decode into `TokenBuf::Text` or `TokenBuf::Raw` depending on `DecodeMode`/surrogate-preserving.
- On closing `"`: finalize with `finish_text`, which returns a borrowed slice if still `Borrowable`, else `Owned(TokenBuf)`.

Number Handling
- Begin `Capture` at first digit or sign.
- Consume digits/points/exponent via `push_from_input_while`.
- On token end:
  - If still `Borrowable`, return `Payload::Borrowed(&str)` from the batch.
  - Else return `Payload::Owned(TokenBuf::Text)`.

Drop and Ownership Semantics
- `Input` guard owns the ring while iterating and returns it to the parser on Drop.
- If a token is in flight when the iterator is dropped, borrowed data already consumed from the batch is preserved by flipping that token’s `Capture` to `Owned` and copying the prefix into its `TokenBuf` (the parser already does this today; the responsibility moves from the iterator drop to `Capture`/`Input` helpers).
- The unread batch tail is pushed into the ring in `Input::drop` so the next `feed` continues from `self.source`.

Event Surface (unchanged)
- Borrowed tokens yield `&'src str` for strings and numbers.
- Owned tokens are built from `TokenBuf` via existing `EventCtx` methods (`new_str_owned`, `new_number_owned`).
- Property names do not emit partial fragments.

What This Removes/Simplifies
- No `reading_from_source(batch)` branches and duplicated write paths.
- No `owned_batch_buffer` and `owned_batch_raw` separate from `token_buffer`; a single `TokenBuf` represents the owned state.
- No ad hoc `token_is_owned` flag; ownership state lives in `Capture::mode` and is localized to the token.
- `BatchView`/`BatchCursor` collapse into the simpler `Input { batch, bytes_in_batch, chars_in_batch }` with `mark()`/`slice_from()` as the only borrowing API.

Critique / Trade-offs
- Borrow slicing still needs char→byte mapping. `slice_from(mark)` uses the already-tracked `bytes_in_batch` and `chars_in_batch` to derive byte ranges in O(1) for common cases; worst case requires a short `char_indices()` walk when mark falls outside cached spans. This matches today’s cost profile but is centralized.
- Keeping the ring as `VecDeque<char>` maintains current correctness and makes char iteration easy, but it’s not optimal for throughput. A follow-up could switch the ring to `VecDeque<u8>` with an internal UTF‑8 decoder for `peek/next`. That change is orthogonal to this proposal.
- RAII moves more work into guard types. This simplifies parser code but requires careful lifetimes so we don’t outlive `&'src str` borrows. The guard only ever returns slices of the passed batch, so lifetimes are straightforward.
- Strings with surrogate-preserving raw output still need a raw sink. `TokenBuf::Raw(Vec<u8>)` keeps this isolated; decoding logic chooses between `Text` and `Raw` at the moment we encounter an unpaired surrogate.
- Minor refactor touch-points: error reporting uses positions that now live in `Input` during iteration; we must plumb them back into `StreamingParserImpl` at guard drop.

Migration Plan
- Step 1: Add `TokenBuf`, `Payload`, `Capture`, and `Input` behind a module flag, with unit tests for their behavior (mark/slice/ensure_owned/split_prefix).
- Step 2: Port number lexing to `Capture` + `Input` (small surface, no string escapes).
- Step 3: Port string lexing, including escape handling and surrogate modes; remove `owned_batch_buffer` and `owned_batch_raw`.
- Step 4: Replace `BatchView`/`BatchCursor` with `Input` in the iterator; move `Drop` preservation logic out of `StreamingParserIteratorWith` into `Input`/`Capture`.
- Step 5: Remove `token_is_owned`, `string_had_escape`-driven write-path branching where possible.
- Step 6: Bench and re-tune hot paths (`copy_while`, escape decode) with the simpler API.

Appendix: Minimal Pseudocode for String Path

```rust
fn lex_string(&mut self, inp: &mut Input<'src>) -> Result<Option<LexTok<'src>>, Err> {
    let mut cap = Capture::new(inp);
    loop {
        match inp.peek() {
            Some('\\') => {
                if let Some(prefix) = cap.split_prefix_and_own(inp) {
                    // emit borrowed prefix fragment now
                    return Ok(Some(LexTok::StringBorrowed(prefix).partial()));
                }
                inp.next();
                decode_escape_into(cap.ensure_owned());
            }
            Some('"') => {
                inp.next();
                return Ok(Some(match cap.finish_text(inp) {
                    Payload::Borrowed(s) => LexTok::StringBorrowed(s),
                    Payload::Owned(TokenBuf::Text(s)) => LexTok::StringOwned(s),
                    Payload::Owned(TokenBuf::Raw(b)) => LexTok::StringRawOwned(b),
                }));
            }
            Some(c) if is_fast_path_char(c) => {
                // Greedy copy borrowed or owned depending on cap.mode
                cap.push_from_input_while(inp, is_fast_path_char);
            }
            Some(c) => { cap.push_char(inp, c); inp.next(); }
            None => return Ok(Some(LexTok::Eof.partial())),
        }
    }
}
```

Why This Helps
- Parser code reads top-to-bottom like a conventional lexer again: read chars, capture tokens, decide borrowed vs owned in one place.
- Drop responsibilities are confined to one RAII guard.
- Eliminates cross-cutting flags and duplicated buffers, reducing mental overhead and bugs around “did we already commit to owned?”

Open Questions
- Should `Input` expose byte-wise APIs to speed up ASCII runs? We can add specialized helpers later without changing the owning/borrowing contract.
- Do we want `Capture` specialized as `StrCapture`/`NumCapture` to make illegal ops (like splitting numbers) a type error? Nice-to-have; can be added incrementally.
- If we later switch the ring to bytes, does `Mark(usize)` track chars or bytes? Today the parser tracks chars for diagnostics; we can keep `Mark` as chars and maintain a small converter for batch slicing.
