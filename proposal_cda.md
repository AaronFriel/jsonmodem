Unified Parser Buffer Redesign (Proposal CDA)

Status: proposal
Audience: parser maintainers and contributors

Objective
- Improve upon proposals CD (C+D) and DA (A+D) to produce a single, practical design that:
  - Keeps ring-first, borrow-first semantics simple and explicit.
  - Separates lexical FSM from ownership/borrowing mechanics.
  - Preserves current iterator-drop responsibilities (in-flight preservation, unread tail spill) without RAII back-references.
  - Collapses scattered buffers/flags into a small, coherent token state.
  - Maintains performance with straightforward fast paths and precise slice boundaries.

Guiding Principles
- Source priority: consume ring first, then batch; never borrow from ring.
- Borrow-first: produce borrowed `&'src str` only when a token is fully within the current batch and its decoded content equals the source slice.
- Escalate-to-owned: promote to owned on the first moment borrowing becomes unsafe: escapes, surrogate-preserving raw mode, cross-boundary, or ring participation.
- Localized concerns: one place arbitrates sources (reader), one place owns token state and borrowing decisions (capture/state); the parser drives only FSM.
- Iterator-drop stays the integration point for unfinished work and unread input.

Architecture Overview

1) SourceCursor (non-owning, unified reader)
- A thin facade over the two sources with fixed priority (ring → batch). It borrows the parser’s ring and position fields, and holds a local batch cursor.
- No `mem::take`, no RAII write-back. Iterator-drop continues to handle batch spill.

Sketch
  struct SourceCursor<'p, 'src> {
    ring: &'p mut Buffer,
    batch: Option<&'src str>,
    batch_bytes: usize,   // bytes consumed in batch
    batch_chars: usize,   // chars consumed in batch (diagnostics)
    pos: &'p mut usize,
    line: &'p mut usize,
    col: &'p mut usize,
    end_of_input: bool,
  }

  impl<'p, 'src> SourceCursor<'p, 'src> {
    fn peek(&self) -> PeekedChar;               // ring first, then batch, else Empty/EOI
    fn advance(&mut self) -> Option<char>;      // updates pos/line/col and cursors
    fn advance_while<F: FnMut(char)->bool>(&mut self, pred: F) -> usize;         // borrowable fast path
    fn copy_batch_while_to<F: FnMut(char)->bool>(&mut self, dst: &mut String, pred: F) -> usize; // owned path
    fn batch_byte(&self) -> usize { self.batch_bytes }
  }

Why non-owning vs DA’s InputGuard?
- It avoids `mem::take` and RAII finalization altogether, reducing aliasing risks. Positions are always up-to-date (mutably borrowed), and iterator-drop remains the one place that appends unread batch tail into the ring.

2) TokenScratch (shared owned accumulation)
- A single owned scratch for all tokens, replacing multiple per-path buffers.

  enum TokenScratch { Text(String), Raw(Vec<u8>) }
  impl TokenScratch {
    fn clear(&mut self);
    fn ensure_raw(&mut self);       // migrate Text→Raw exactly once
    fn as_text_mut(&mut self) -> &mut String;   // panics if Raw
    fn as_raw_mut(&mut self) -> &mut Vec<u8>;   // panics if Text
  }

3) TokenState (parser-persistent, per-token)
- A small struct that persists across `next()` calls until the token completes; it subsumes scattered flags (`token_is_owned`, `string_had_escape`, etc.) and buffers.

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
    fn can_borrow_slice(&self, cur_batch_byte: usize) -> bool { self.start_batch_byte.is_some() && !self.must_own() && self.start_batch_byte.unwrap() <= cur_batch_byte }
  }

4) CaptureHandle (ephemeral controller)
- A thin wrapper created inside lexing functions that holds `&mut TokenState` and `&mut SourceCursor` for convenience. It never owns buffers; it orchestrates pushes/borrows against the persistent state.

  struct CaptureHandle<'a, 'p, 'src> {
    st: &'a mut TokenState,
    cur: &'a mut SourceCursor<'p, 'src>,
  }

  impl<'a, 'p, 'src> CaptureHandle<'a, 'p, 'src> {
    fn borrow_prefix_and_flip(&mut self) -> Option<&'src str>;  // String only; returns borrowed prefix and makes state owned for remainder
    fn push_char(&mut self, ch: char);                          // appends into scratch (Text or Raw)
    fn push_raw(&mut self, bs: &[u8]);                          // appends into scratch Raw
    fn advance_while_borrowable<F: FnMut(char)->bool>(&mut self, pred: F) -> usize; // advance without copying when borrowable
    fn copy_batch_while_owned<F: FnMut(char)->bool>(&mut self, pred: F) -> usize;   // copy into Text when owned in batch
  }

Borrow vs Owned Rules (harmonized)
- Borrowed slices are produced only if: token started in batch; no escape; not in raw mode; token ends within the current batch.
- Once `must_own()` is true, all subsequent fragments for this token are owned. Strings may emit partials; property names and numbers do not.
- Starting in ring permanently disqualifies borrowing for the token.

Iterator Drop Semantics (preserved, simplified)
- Iterator.drop does two things, using SourceCursor metrics and TokenState:
  1) Preserve in-flight prefix: if a token is open and we were reading from batch, copy batch[start_batch_byte..cur_batch_byte] into `TokenState.scratch` (Text or Raw), and set `had_escape |= true` to guarantee owned continuation next time.
  2) Push unread batch tail: append `&batch[cur_batch_byte..]` into the ring.
- No ring movement or position write-back occurs in Drop; positions are always current because SourceCursor mutates parser fields directly.

Event Surface (unchanged)
- `LexToken` variants remain. Parser converts:
  - Borrowed: `&batch[start..end]` with `'src` lifetime.
  - Owned: `TokenScratch::Text` → `String` and `TokenScratch::Raw` → `Vec<u8>`.
  - For compatibility, ring-sourced owned fragments may map to existing `...Buffered` variants if desired.

Lexing Flows

Numbers
- Start TokenState with `Number`, in ring or batch depending on `ring.peek()` at start.
- Greedy consume with `advance_while` on borrowable path (batch), or `copy_batch_while_to` / ring copy for owned path.
- Finish: borrowed if still eligible; otherwise owned from `scratch`.

Strings (values)
- Start TokenState at char after `"` as in Numbers.
- Fast path: `advance_while(|c| c != '\\' && c != '"')` when borrowable.
- On `\`:
  - If borrowable and there is a non-empty prefix in batch, `borrow_prefix_and_flip()` to emit that partial as borrowed; set `had_escape = true`.
  - Decode escape; for surrogate-preserving, call `ensure_raw()` and push raw bytes.
- On closing `"`: if still borrowable, emit borrowed; else emit owned from `scratch` (Text/Raw). Maintain `is_initial`/`is_final` semantics.

Property Names
- Same as value strings except: never emit partial; on first `\` or boundary, flip to owned and continue accumulating silently.

Positions and Diagnostics
- `SourceCursor` updates `pos/line/col` directly via references; lexing and error reporting use those fields, so there’s no separate finalize step.

Performance Considerations
- Slice boundaries: record `start_batch_byte` at token start and use `cur.batch_byte()` at emission, slicing directly without `char_indices` rescans. Keep `batch_chars` only for diagnostics and iterator-drop copy bounds.
- Fast paths: numeric runs and unescaped string spans use `advance_while` (borrowable) or `copy_batch_while_to` (owned). `TokenScratch` reuses allocation across tokens to minimize churn.
- Ring remains `VecDeque<char>` for correctness; optional future: byte-ring with UTF‑8 decode.

What This Removes/Simplifies
- Eliminates multiple per-path buffers and cross-cutting flags in favor of `TokenState { scratch, started_in_ring, start_batch_byte, had_escape, kind }`.
- Removes `mem::take` and RAII finalizers; keeps iterator-drop for batch spill and in-flight preservation.
- Centralizes borrow/own transitions and copying logic in `TokenState`/`CaptureHandle` methods.

Why This Improves on CD and DA
- From CD: retains non-owning reader and iterator-drop spill, plus persistent token state; adds DA’s clearer TokenScratch and typed capture semantics.
- From DA: preserves explicit, parser-resident scratch and typed capture operations; removes the need to move the ring out of the parser; finalization is implicit because positions are live-updated and tail spill stays in drop.
- Across both: locks down partial rules (strings only), enforces “started in ring” disables borrowing, and encodes raw/UTF‑8 switching as a single idempotent operation.

Minimal Helper Signatures
- Reader
  - `fn peek(&self) -> PeekedChar`
  - `fn advance(&mut self) -> Option<char>`
  - `fn advance_while<F>(&mut self, F) -> usize`
  - `fn copy_batch_while_to<F>(&mut self, &mut String, F) -> usize`
- Token state
  - `fn start_in_ring(&mut self, kind: CaptureKind)`
  - `fn start_in_batch(&mut self, kind: CaptureKind, start_byte: usize)`
  - `fn mark_escape(&mut self)` / `fn ensure_raw(&mut self)`
  - `fn must_own(&self) -> bool` / `fn can_borrow_slice(&self, end_byte: usize) -> bool`
- Iterator drop helpers
  - `fn preserve_in_flight(state: &mut TokenState, batch: &str, end_byte: usize)`
  - `fn push_unread_tail(ring: &mut Buffer, batch: &str, end_byte: usize)`

Migration Plan
- Phase 1: Introduce `TokenScratch` and `TokenState` in the parser; keep existing logic but route owned writes through `TokenScratch`; add `ensure_raw`.
- Phase 2: Replace `BatchView/BatchCursor` with `SourceCursor` in lexing functions; keep iterator-drop logic, updated to use `batch_bytes`.
- Phase 3: Port number lexing to `TokenState`/`SourceCursor` (easier surface); validate borrowed vs owned transitions and EOI handling.
- Phase 4: Port string lexing with escapes and surrogate handling; remove `owned_batch_buffer`/`owned_batch_raw` and `token_buffer`.
- Phase 5: Remove legacy flags (`token_is_owned`, `string_had_escape`, etc.); keep only `TokenState` fields.
- Phase 6: Bench, add ASCII fast-path tuning, and consider pooling capacities in `TokenScratch`.

Risks and Mitigations
- Wrong slice boundaries: rigorous tests for start/end-of-batch escapes, ring→batch boundaries, and long runs; property-based tests.
- Lifetime errors: slices originate from the iterator’s batch `&'src str`; `SourceCursor` never owns, so lifetimes map directly.
- Performance regressions: retain greedy paths; reuse `TokenScratch` allocations; add short-circuit ASCII scans.
- Surrogate-preserving pitfalls: ensure `ensure_raw` migrates a string’s prior content exactly once and all subsequent writes go to Raw.

Open Questions
- Should we prohibit `as_text_mut`/`as_raw_mut` at type level via specialized capture types (StrCapture/NumCapture) to avoid mode misuse? Possible refinement.
- Do we want a byte-ring in-tree prototype now or later? This design does not preclude it.
- Is there value in tracking both batch byte and char marks for precise error columns across splits? Currently we keep `batch_chars` for diagnostics only.

