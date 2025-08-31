JSON Parser Buffer System Redesign (Combined Proposal B+C)

Summary
- Unite both proposals around two focused, orthogonal building blocks:
  1) an input “session” that owns where characters come from for one iterator pass, and
  2) a token “capture” that owns how bytes for one token are materialized (borrow vs own, utf8 vs raw).
- Make borrow-first, own-on-demand the default policy with explicit upgrade points.
- Preserve external semantics: ring is always drained first and never borrowed; batch yields borrowed slices when safe; keys never fragment; numbers never fragment; string fragments may fragment.
- Address critiques from B and C: avoid duplicate fragments on upgrade, persist in-flight captures across iterator drop, remove char→byte rescans, and isolate lifetime/aliasing concerns.

Principles
- Ring-first: while the ring has chars, parse exclusively from it; all content is owned.
- Batch-borrow: when the ring is empty, parse directly over the current `&str` batch; borrow slices unless escaping/ownership is required.
- No ring borrows: borrowed fragments only ever reference the batch.
- Localize ownership decisions: the capture decides owned vs borrowed; the parser’s FSM remains agnostic.

Core Abstractions

1) InputSession<'src>
- Purpose: a per-`next()` view over sources and location. It owns the ring for the duration (via `mem::take`) and has a borrowed view of the batch.
- Responsibilities:
  - Provide unified peek/advance/scan (ring first, then batch).
  - Maintain and expose batch byte offset for O(1) borrowed slicing.
  - Track `pos/line/column` locally and return them to the parser on drop.
  - On drop: append unread batch tail to the ring and return the ring to the parser.

Sketch
  struct InputSession<'src> {
    ring: Buffer,                // moved out of parser
    batch: Option<&'src str>,
    batch_bytes: usize,          // consumed bytes in batch
    pos: usize, line: usize, col: usize,
    end_of_input: bool,
  }

  enum Peeked { Empty, EndOfInput, Char(char) }

  impl<'src> InputSession<'src> {
    fn peek(&self) -> Peeked;                            // ring → batch → EOI/Empty
    fn advance(&mut self) -> Option<char>;               // updates location and batch_bytes
    fn advance_while<F: Fn(char)->bool>(&mut self, F) -> usize;
    fn in_ring(&self) -> bool;                           // next char comes from ring
    fn batch_byte(&self) -> usize;                       // current batch byte cursor
    fn slice_batch(&self, start_byte: usize, end_byte: usize) -> Option<&'src str>;
  }

Drop
- `impl Drop for InputSession<'_>`:
  - Push `batch[batch_bytes..]` into the ring.
  - Write back `pos/line/col` to the parser.
  - Move the ring back into `parser.source`.

2) TokenCapture<'src>
- Purpose: per-token accumulator with borrow-first semantics and explicit upgrade points.
- Modes:
  - Borrowing { start_byte }
  - OwnedUtf8 { buf: String }
  - OwnedRaw { bytes: Vec<u8> } // surrogate-preserving
- Metadata flags: `started_in_ring: bool`, `had_escape: bool`, `kind: {Key | String{raw} | Number}`

API
  impl<'src> TokenCapture<'src> {
    fn start_in_batch(kind, start_byte: usize) -> Self;   // Borrowing
    fn start_in_ring(kind) -> Self;                       // OwnedUtf8
    fn mark_escape(&mut self);                            // escape seen → must own
    fn ensure_raw(&mut self);                             // switch utf8→raw once
    fn push_char(&mut self, c: char);                     // appends in owned modes
    fn push_raw(&mut self, bs: &[u8]);                    // raw mode only
    fn borrow_prefix_and_switch_to_owned<'a>(
       &mut self,
       batch: &'src str,
       upto_byte: usize,
    ) -> &'src str;                                       // values: emit prefix, then own
    fn upgrade_copy_prefix(&mut self, batch: &str, upto_byte: usize); // keys: copy prefix, then own
    fn can_borrow_final(&self, end_byte: usize) -> bool;  // still borrowing and in same batch
    fn take_owned_utf8(self) -> String;
    fn take_owned_raw(self) -> Vec<u8>;
  }

Rules
- Must-own when: started in ring, saw escape (or decode differs), raw mode active, or token crosses feed boundary.
- Borrow allowed when: mode is Borrowing, start..end fully in current batch, no escape/raw, and token finishes in this batch.
- Keys never fragment: at closing quote, either borrow whole slice or emit owned; mid-key escape triggers `upgrade_copy_prefix` (no partials).
- Numbers never fragment: at delimiter, either borrow whole slice or emit owned; mid-number EOI/Empty upgrades to owned.
- String values may fragment: before first escape, emit borrowed prefixes using `borrow_prefix_and_switch_to_owned`; after upgrade, further fragments are owned.

3) InFlight persistence in the parser
- Purpose: if the iterator drops mid-token, preserve in-flight capture so the next call resumes seamlessly.
- Shape:
  enum CaptureKind { Key, String, Number }
  struct InFlight<'src> { kind: CaptureKind, cap: TokenCapture<'src> }
  struct Parser { in_flight: Option<InFlight<'src>>, /* … */ }
- Iterator Drop / parser handoff:
  - Keys and numbers: if capture is Borrowing, copy batch slice into owned and store as `in_flight`.
  - String values: if we already emitted a borrowed prefix, the active capture should already be owned (empty or with data); store it; if not yet emitted anything, copy current borrowed prefix and store.

Event Emission
- LexToken/ParseEvent remain unchanged externally; they gain nothing else beyond distinguishing Borrowed/Buffered/Owned variants.
- Backends remain the same; raw bytes are passed with `RawStrHint` as today.

Lifetimes and Aliasing
- The iterator owns `InputSession` for the duration of `next()` and produces at most one token at a time.
- Any borrowed `&'src str` in `LexToken` refers to the batch held by the iterator; the iterator’s lifetime parameter carries `'src`.
- `InputSession` does not hold references into the parser; it copies `pos/line/col` in/out, avoiding borrow-checker aliasing.

Byte vs Char Accounting
- Only `batch_bytes` are needed to produce slices; `pos/line/col` maintain correctness for diagnostics.
- To avoid rescans, `TokenCapture` records `start_byte` at token start. Final borrowed slices are `&batch[start_byte..session.batch_byte()]`.
- Quote/backslash handling must compute `upto_byte` before consuming the delimiter to avoid off-by-ones.

Fast Paths
- Ring: `Buffer::copy_while(&mut String, pred)` remains for owned accumulation.
- Batch borrowed: `session.advance_while(pred)` just advances counters (no writes) until a delimiter/escape/quote.
- Batch owned: `session.advance()` + `capture.push_char(c)` (or a `copy_while_to_owned` helper for tight loops).

Surrogate-Preserving
- Keep parser fields that coordinate surrogates (e.g., pending high surrogate). On first unpaired surrogate in a value, `capture.ensure_raw()` and start pushing WTF-8 bytes; keys degrade to U+FFFD per current behavior.

Error and EOI Paths
- All early returns (Err/EOI) go through iterator scope so `InputSession` Drop always runs, restoring ring and unread batch tail.
- Numbers at EOI/Empty: capture upgrades to owned and is stored in `in_flight` for continuation.

Migration Plan (incremental, testable)
1. Introduce `InputSession` minimal form (peek/advance, batch_bytes) and route lexing through it while keeping existing `BatchView` for compatibility.
2. Introduce `TokenCapture` with only utf8 owned path; port string value lexing to use it; keep current raw/escape bookkeeping in parser.
3. Add `borrow_prefix_and_switch_to_owned` and use it at the first escape for string values; confirm no duplicate fragments by tests.
4. Add `InFlight` to the parser and update iterator Drop to persist capture for keys/numbers/strings; remove `token_start_pos` and friends.
5. Port numbers to `TokenCapture`; delete `token_buffer`, `owned_batch_buffer`, `owned_batch_raw`, and `token_is_owned`.
6. Integrate surrogate-preserving by adding `ensure_raw` and raw push path; reuse existing parser flags as needed.
7. Remove `BatchView::slice_chars` and any char→byte rescans; rely on `start_byte` + `batch_bytes` only.
8. Trim remaining legacy fields and helpers; keep `initialized_string`, surrogate flags, and options intact.

Why This Resolves B + C Concerns
- No duplicate string fragments: value strings use `borrow_prefix_and_switch_to_owned` to return a borrowed prefix once and reset the owned buffer for the remainder.
- In-flight safety: `InFlight` explicitly persists partially consumed tokens across iterator drops.
- Borrow checker clarity: `InputSession` owns location; no shared `&mut` access to parser during lexing; state is copied back on drop.
- No char→byte rescan: capture uses byte marks; sessions track byte cursors; slices are O(1).
- Ring→batch transitions: `started_in_ring` pins ownership for the token even after ring drains.
- Same external API: `ParseEvent`/`LexToken` shapes stay compatible; backends unchanged.

Remaining Trade-offs / Open Questions
- Owned buffer reuse: we can pool `String`/`Vec<u8>` capacities across tokens to reduce allocations (e.g., store a scratch in parser and swap it into `TokenCapture`).
- Ring type: a `VecDeque<char>` is simple but not fastest for long runs; a byte-ring could be a later optimization without changing this design.
- Numbers: copy vs defer? We keep borrow-first when fully in batch; otherwise upgrade to owned. If simplification is desired, always own numbers—sacrificing zero-copy.
- Error reporting: if we later move to byte-based positions, we must preserve line/col accuracy; today we keep char counts for clarity.

Appendix: Typical String Value Flow (no duplicates)
1) On opening quote, if `session.in_ring()` then `capture.start_in_ring(String{raw:false})`, else `start_in_batch(.., session.batch_byte())`.
2) Scan until either backslash or quote using `advance_while`.
3) If backslash:
   - `let prefix = capture.borrow_prefix_and_switch_to_owned(batch, upto_byte)`; emit borrowed fragment event with `prefix`.
   - Consume escape, decode, push into capture (owned utf8/raw).
   - Continue scanning; further fragments are owned.
4) If quote:
   - If `capture.can_borrow_final(end_byte)`, emit `Borrowed` once; else emit `Owned`/`Raw` using capture’s buffer.

