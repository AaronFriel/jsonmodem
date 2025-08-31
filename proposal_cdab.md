Comprehensive Parser Buffer Redesign (Proposal CDAB)

Status: proposal
Audience: parser maintainers and contributors

Objective
- Synthesize and improve the ideas from proposals C, D, A, and B into one coherent plan that:
  - Keeps ring-first, borrow-first semantics explicit and correct.
  - Cleanly separates lexical FSM from input ownership and token materialization.
  - Preserves the current external API and event lifetimes, including iterator-drop behavior for partial tokens and unread tails.
  - Collapses scattered buffers/flags into a small, testable token state with clear, local rules.
  - Maintains performance via greedy fast paths and precise batch-slice boundaries.

Design Tenets
- Source priority: consume the ring first, then the current batch; never return borrows into the ring.
- Borrow-first: strings and numbers that are fully within the batch and unaffected by decoding are emitted as `&'src str`.
- Escalate-to-owned: switch to owned accumulation at the earliest sign that borrowing would be unsafe (escapes, surrogate-preserving/raw mode, ring participation, cross-batch tokens).
- Localized mechanics: a unified reader arbitrates ring vs batch; a token state/capture owns borrow/own decisions; the parser implements only the FSM.
- Iterator-drop remains the single place that preserves in-flight progress and pushes unread batch tail.

Architecture

1) SourceCursor (non-owning unified reader)
- Purpose: single, minimal API to read chars with ring→batch priority and update positions.
- Non-owning by design: borrows parser ring and position counters; holds local batch cursors. Avoids `mem::take` and RAII finalizers (a key improvement over RAII-heavy sketches) while keeping iterator-drop semantics intact.

Sketch
  struct SourceCursor<'p, 'src> {
    ring: &'p mut Buffer,
    batch: Option<&'src str>,
    batch_bytes: usize,   // bytes consumed within batch
    batch_chars: usize,   // chars consumed within batch (diagnostics)
    pos: &'p mut usize,
    line: &'p mut usize,
    col: &'p mut usize,
    end_of_input: bool,
  }

  impl<'p, 'src> SourceCursor<'p, 'src> {
    fn peek(&self) -> PeekedChar;                   // ring first, then batch, else Empty/EOI
    fn advance(&mut self) -> Option<char>;          // updates pos/line/col and increments ring/batch cursors
    fn advance_while<F: FnMut(char)->bool>(&mut self, pred: F) -> usize;             // greedy, borrowable path
    fn copy_batch_while_to<F: FnMut(char)->bool>(&mut self, dst: &mut String, pred: F) -> usize; // owned batch path
    fn batch_byte(&self) -> usize { self.batch_bytes }
  }

Why non-owning wins
- Eliminates aliasing/Drop complexity (no ring moves at iteration start). Positions are always current; iterator-drop can safely append the unread tail using `batch_bytes`.

2) TokenScratch (single owned buffer)
- Purpose: unify all owned accumulation into one enum, supporting both UTF-8 text and raw bytes (for surrogate-preserving mode).

  enum TokenScratch { Text(String), Raw(Vec<u8>) }
  impl TokenScratch {
    fn clear(&mut self);
    fn ensure_raw(&mut self);       // migrate Text→Raw once, preserving content
    fn push_char(&mut self, c: char);    // Text mode only
    fn push_bytes(&mut self, b: &[u8]);  // Raw mode only
  }

3) TokenState (parser-persistent per-token state)
- Purpose: replace scattered flags/buffers with a single, durable state that persists across multiple `next()` calls for partial strings.

  enum CaptureKind { String { raw: bool }, Number, PropertyName }

  struct TokenState {
    kind: CaptureKind,
    started_in_ring: bool,
    start_batch_byte: Option<usize>,
    had_escape: bool,
    scratch: TokenScratch,
  }

  impl TokenState {
    fn reset(&mut self);
    fn start_in_ring(&mut self, kind: CaptureKind) { self.reset(); self.kind = kind; self.started_in_ring = true; self.start_batch_byte = None; }
    fn start_in_batch(&mut self, kind: CaptureKind, start_byte: usize) { self.reset(); self.kind = kind; self.started_in_ring = false; self.start_batch_byte = Some(start_byte); }
    fn mark_escape(&mut self) { self.had_escape = true; }
    fn ensure_raw(&mut self) { self.scratch.ensure_raw(); if let CaptureKind::String { raw } = &mut self.kind { *raw = true; } }
    fn must_own(&self) -> bool { self.started_in_ring || self.had_escape || matches!(self.kind, CaptureKind::String { raw: true }) }
    fn can_borrow_slice(&self, end_batch_byte: usize) -> bool { self.start_batch_byte.is_some() && !self.must_own() && self.start_batch_byte.unwrap() <= end_batch_byte }
  }

4) CaptureHandle (ephemeral controller)
- Purpose: ergonomics wrapper operating over `&mut TokenState` + `&mut SourceCursor`; provides precise borrow-prefix emission and owned accumulation.

  struct CaptureHandle<'a, 'p, 'src> { st: &'a mut TokenState, cur: &'a mut SourceCursor<'p, 'src> }
  impl<'a, 'p, 'src> CaptureHandle<'a, 'p, 'src> {
    // String values only: return borrowed prefix slice and flip state to owned with empty scratch.
    fn borrow_prefix_and_flip(&mut self, batch: &'src str) -> Option<&'src str>;
    // Property names: copy prefix into scratch and flip to owned (no partial emission).
    fn copy_prefix_and_flip(&mut self, batch: &'src str);
    fn push_char(&mut self, ch: char);           // into Text
    fn push_raw(&mut self, bs: &[u8]);           // into Raw
    fn advance_while_borrowable<F: FnMut(char)->bool>(&mut self, pred: F) -> usize;
    fn copy_batch_while_owned<F: FnMut(char)->bool>(&mut self, pred: F) -> usize;
  }

Borrow vs Owned Rules (unified)
- Borrowed emission requires: token started in batch; no escape; not in raw mode; token ends within the current batch. Numbers also require single-batch containment; property names require full completion (no partials).
- Owned emission triggers for: token started in ring; escape in strings; surrogate-preserving/raw mode; cross-batch tokens.
- Once owned, the token remains owned; only strings may emit partials. Property names and numbers do not fragment.

Iterator-drop Semantics (unchanged location; clearer helpers)
- On drop of the per-feed iterator:
  1) Preserve in-flight token prefix: if a token is in progress and was borrowing, copy `batch[start_batch_byte..cur_batch_byte]` into `TokenState.scratch` (Text or Raw as required) and set `had_escape = true` to force owned on resume.
  2) Push unread batch tail: append `&batch[cur_batch_byte..]` into the ring.
- No ring movement at iteration start; no RAII finalize. Positions are current because `SourceCursor` updates parser fields directly.

Lexing Flows

Numbers
- Start `TokenState` at first digit/sign: `start_in_ring` if ring active; else `start_in_batch` with `start_batch_byte = cur.batch_byte()`.
- Greedy consume via `advance_while` (borrowable batch) or copy into scratch (ring/owned path).
- End: emit borrowed only if `can_borrow_slice(cur.batch_byte())`; else emit owned from `scratch` (numbers never partial).

Strings (values)
- On opening `"`: initialize `TokenState` as for numbers with `CaptureKind::String { raw: false }`.
- Fast path: while borrowable, `advance_while(|c| c != '\\' && c != '"')`.
- On backslash:
  - If borrowable and prefix non-empty, `borrow_prefix_and_flip(batch)` to emit borrowed fragment and reset scratch; set `had_escape = true`.
  - Decode escape; for surrogate-preserving, call `ensure_raw()` and append decoded bytes with `push_raw`.
- On closing `"`: if still borrowable, emit borrowed from batch `[start..cur]`; else emit owned from scratch. Maintain `is_initial`/`is_final` flags.

Property Names
- Same scanning as value strings, but never emit partial. On backslash or boundary, `copy_prefix_and_flip(batch)` and continue owned until closing `"`.

Positions and Diagnostics
- `SourceCursor` updates `pos/line/col` through references; error reporting uses those updated parser fields.

Event Surface (unchanged)
- `LexToken` variants remain (borrowed and owned). Parser maps `TokenState` output to the existing `ParseEvent` via backend `EventCtx`.

Performance Considerations
- Precise slices: record `start_batch_byte` at token start; use `cur.batch_byte()` at emission; slice `&batch[start..end]` without rescans. `batch_chars` maintained for diagnostics and drop-copy bounds.
- Greedy paths: batch borrowable spans use `advance_while`; owned batch path uses `copy_batch_while_to`; ring path uses its existing `copy_while`.
- Allocation reuse: `TokenScratch` persists across tokens; reserve capacity based on prior usage to minimize reallocations.
- Future: byte-ring + UTF-8 decoder can replace `VecDeque<char>` without changing this API.

Why This Improves CD and DA (and incorporates B)
- Keeps CD’s non-owning reader and strict iterator-drop responsibilities; adds DA’s consolidated scratch and typed token state; incorporates B’s borrow-prefix emission API to avoid duplicate output.
- Prevents borrow/Drop aliasing by avoiding `mem::take` ownership tricks; simplifies lifetimes and restores a conventional lexer structure.
- Encodes all ownership decisions locally to the token, with clear invariants: “started in ring” and “escape/raw” permanently force owned.

Helper Signatures (summary)
- Reader: `peek`, `advance`, `advance_while`, `copy_batch_while_to`, `batch_byte`.
- Token state: `start_in_ring`, `start_in_batch`, `mark_escape`, `ensure_raw`, `must_own`, `can_borrow_slice`.
- Capture ops: `borrow_prefix_and_flip`, `copy_prefix_and_flip`, `push_char`, `push_raw`, `advance_while_borrowable`, `copy_batch_while_owned`.
- Drop helpers: `preserve_in_flight(&mut TokenState, batch: &str, end_byte: usize)`, `push_unread_tail(ring: &mut Buffer, batch: &str, end_byte: usize)`.

Migration Plan
- Phase 1: Add `TokenScratch` + `TokenState`; route existing owned writes through them and implement `ensure_raw` mirroring current behavior.
- Phase 2: Introduce `SourceCursor` in place of `BatchView/BatchCursor` inside lexing; keep iterator-drop logic, now using `batch_bytes`.
- Phase 3: Port numbers to `TokenState`/`SourceCursor` first; verify ring-start forces owned and batch-contained emits borrowed.
- Phase 4: Port strings, including escape/Unicode handling and surrogate-preserving; remove `owned_batch_buffer`/`owned_batch_raw`/`token_buffer`.
- Phase 5: Remove legacy flags (`token_is_owned`, `string_had_escape`, etc.); consolidate on `TokenState`.
- Phase 6: Benchmarks; add ASCII fast paths and capacity reuse in `TokenScratch`.

Risks and Mitigations
- Boundary mistakes (off-by-one around quotes/backslashes): compute end byte before consuming delimiter; add tests for boundary escapes.
- Lifetime errors: all borrowed slices originate from the iterator’s batch `&'src str`; `SourceCursor` remains non-owning.
- Performance regressions: maintain greedy scans; reuse buffers; add micro-benchmarks around numbers and unescaped strings.
- Surrogate edge cases: ensure `ensure_raw` migrates exactly once and that pending-high/low-surrogate parser fields remain honored.

