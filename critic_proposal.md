Critic-Proposed Refactor Plan (revised, v4) for crates/jsonmodem/src/parser

This proposal consolidates the strongest, implementable ideas into a concrete, low‑risk refactor plan. It resolves the RAII vs borrowing tension by making the iterator the sole owner of finalization and parser mutation, while giving lexing a clean, testable API.

Scope: internal parser architecture only. Public events, options, and lifetimes remain unchanged.

Goals
- Keep invariants: ring-first; never borrow from ring; batch-borrow when fully contained and undecoded; numbers/keys never fragment; values may fragment; preserve surrogate‑preserving behavior.
- Keys remain UTF‑8 only: property names never emit Raw; invalid sequences are handled per existing decode policy (e.g., replacement in non‑strict modes).
- Reduce complexity: centralize unread-input and token-scratch handling inside a single per-feed owner (Scanner). The parser no longer manipulates buffers directly.
- De-risk lifetimes/Drop: avoid RAII back-references that mutate the parser; centralize finalization in the iterator Drop via a single‑shot `finish(self) -> Tape` on the Scanner.

Architecture Overview
- Scanner<'src> (exclusive owner): The iterator constructs a per‑feed scanner from the parser’s persisted Tape via `Scanner::from_carryover(tape, batch)`. The Scanner owns the unread‑input ring and token scratch for the duration of the feed, plus the current batch `&'src str`, a byte cursor into that batch, and local position counters (`pos/line/col`).
- Unread ring storage: Implemented inside `scanner::Tape` as `VecDeque<u8>` (UTF‑8 only). A separate `ByteBuffer` type is no longer planned; the Tape’s ring is the canonical representation.
- Token scratch (scanner‑owned): Consolidate owned accumulation for the current token as either valid UTF‑8 text or raw bytes. The scanner provides a single typed view when emitting a fragment.
- Iterator finalization: The iterator owns the Scanner and is solely responsible for finalization. Dropping the iterator calls `finish(self) -> Tape`, which pushes unread batch tail to the ring, writes back ring + positions + token state into a new Tape, and leaves the parser ready for the next feed.

Unread Ring — finalized direction
- Invariant: the ring stores only valid UTF‑8 bytes originating from feeds and unread batch tails. Raw/WTF‑8 applies to decoded token output, not to unread input representation.
- Structure: the ring lives inside `scanner::Tape` as `VecDeque<u8>`; it is not exposed publicly. All reading/decoding/pushing happens through `Scanner`.
- Responsibilities in `Scanner`:
  - UTF‑8 decoding for `peek`/`advance` and fast‑path copying.
  - Pushing unread batch tails (`&str`) back into the ring.
  - No `raw` flag on the ring; rawness is a token‑local decision handled by the session’s scratch.
  - Performance: decode from the contiguous front slice of the deque; if a scalar spans the boundary, copy ≤4 bytes into a tiny stack buffer. Provide an ASCII fast path for the batch (contiguous) path.

Core Types and APIs (Scanner/Tape)

Data types
```rust
enum Source { Ring, Batch }
enum FragmentPolicy { Disallowed, Allowed }
enum TokenBuf<'src> { Borrowed(&'src str), OwnedText(String), Raw(Vec<u8>) }
enum TokenScratch { Text(String), Raw(Vec<u8>) }

struct TokenAnchor {
    source: Source,
    start_char: usize,                   // diagnostics
    start_byte_in_batch: Option<usize>,  // Some when source == Batch
    owned: bool, had_escape: bool, is_raw: bool,
}

pub(crate) struct Scanner<'src> { /* owns ring, batch, cursors, scratch, anchor */ }

impl<'src> Scanner<'src> {
    // construction/finalization
    pub fn from_carryover(tape: Tape, batch: &'src str) -> Self;
    pub fn finish(self) -> Tape; // single-shot writeback

    // reading
    pub fn peek(&self) -> Option<Unit>;     // Unit { ch: char, ch_len: u8, source: Source }
    pub fn advance(&mut self) -> Option<Unit>;
    pub fn cur_source(&self) -> Source;

    // token lifecycle
    pub fn begin(&mut self, policy: FragmentPolicy);
    pub fn mark_escape(&mut self);
    pub fn ensure_raw(&mut self);
    pub fn switch_to_owned_prefix_if_needed(&mut self);

    // fast paths
    pub fn copy_while_ascii(&mut self, pred: impl Fn(u8)->bool) -> usize;  // batch-only
    pub fn copy_while_char(&mut self, pred: impl Fn(char)->bool) -> usize; // ring path

    // borrowing (O(1))
    pub fn try_borrow_slice(&self, end_adjust_bytes: usize) -> Option<&'src str>;

    // emits
    pub fn emit_fragment(&mut self, is_final: bool, end_adjust_bytes: usize) -> TokenBuf<'src>;
}
```

Borrow/own/raw rules (precise)
- Borrow only from the current batch and only when the full token lies in the batch, contains no escapes, and is not Raw.
- Keys: fragment_disallowed; Borrowed or OwnedText; under SurrogatePreserving may be Raw (consumer policy decides handling).
- Numbers: fragment_disallowed; Borrowed when fully in-batch, otherwise OwnedText. Never Raw.
- Values: fragment_allowed; Borrowed when fully in-batch and undecoded; may be OwnedText or Raw across feeds.

Iterator Integration and Finalization
- The parser/iterator hold a per‑feed `Scanner<'src>` alongside the legacy buffers during migration. Normal `next()` continues to drive the legacy path; the Scanner is driven in shadow (discard) until we switch individual read surfaces.
- On iterator Drop, call `finish()` exactly once by consuming the scanner and storing the returned Tape. `finish()` copies in‑flight prefixes for fragment‑disallowed tokens if needed, pushes unread batch tail into the ring, and writes back ring/positions/scratch.

Migration Plan (incremental, low risk)
1) Introduce `parser/scanner/mod.rs` with `ByteBuffer`, `TokenScratch`, `TokenBuf`, `TokenAnchor`, `Tape` (persisted state), and `Scanner` (per‑feed owner). Byte anchors are part of the contract.
2) Parser holds ring/scratch/pos as opaque state to be taken by the Scanner; do not mutate them outside the Scanner going forward.
3) Route string/number lexing gradually through `begin/mark_escape/ensure_raw/copy_while*/emit_fragment`. Iterator Drop uses single‑shot `self.tape = scanner.finish()`.
4) Remove `BatchView/BatchCursor` and char→byte rescans after parity; use `start_byte_in_batch..batch_bytes` for slicing.
5) Delete legacy fields (`token_buffer`, `owned_batch_buffer`, `owned_batch_raw`, `token_is_owned`, `token_start_pos`) only after full cutover.
6) Multiple‑values: ensure token state resets correctly when transitioning End→Start.
7) Validation: fuzz feed‑splitting at every byte; ensure numbers/keys never fragment and values fragment correctly; verify decode modes (StrictUnicode/ReplaceInvalid/SurrogatePreserving), including reversed surrogate order; benchmark batch ASCII fast path and ring path.

Risks and Mitigations
- Double finalization: `finish(self)` consumes `self`; iterator stores `Option<Scanner>` and calls finish only if present.
- Lifetime mishaps: Only `try_borrow_slice(end_adjust)` can produce a borrowed `&'src str`, and only from the batch; never store borrowed slices beyond the iterator.
- Fragment duplication: `switch_to_owned_prefix_if_needed()` is idempotent; call at first transform boundary. Keys/Numbers preserved across feeds by `finish`.
- Performance regressions: Batch path uses ASCII fast path and O(1) borrow slicing; ring path decodes from the deque front slice and remains small.

Notes for this codebase
- Map `Parser = StreamingParserImpl<B>`. Introduce `crates/jsonmodem/src/parser/scanner/mod.rs` defining `ByteBuffer`, `Scanner`, `Tape`, `TokenScratch`, `TokenBuf`, and `TokenAnchor`. Keep public `ParseEvent`, `LexToken`, and backends unchanged.
- The iterator (`StreamingParserIteratorWith`) already performs finalization on Drop. Replace the Drop logic with single‑shot `self.tape = scanner.finish()` and remove direct ring/scratch mutations from outside the Scanner.
- Property names remain UTF‑8 only. Preserve existing behavior in decode modes (e.g., replacement vs error) for keys.

Why this plan
- Encodes fragment and decode semantics in one owner (Scanner) while leaving consumer policy (e.g., raw keys) to the backend.
- Removes char↔byte rescans via explicit byte anchors and O(1) borrow slicing.
- Guarantees single‑shot finalization and avoids RAII/lifetime hazards.
- Is incremental: land Scanner + adapters first; then route lexing and delete legacy BatchView/BatchCursor and parser-resident buffers.

Phased Adoption Plan (Dual Write → Dual Read → Cutover)
------------------------------------------------------

Intent
- Adopt Scanner/Tape side‑by‑side with the legacy `Buffer/BatchView` path. First we dual‑write (persist Tape in parallel), then we shadow‑read (drive Scanner in lockstep and discard), and finally we switch reads over in small, low‑risk surfaces.

Controls
- No public feature flags or runtime knobs. The Scanner/Tape code lives alongside the legacy path. During migration, Scanner runs in shadow mode (discard) and is validated via debug‑only assertions. We progressively switch specific read surfaces to Scanner within the same codebase.

Status Summary (as of 2025‑09‑01)
- [x] Phase 0: Scaffold (parser holds Tape; iterator constructs Scanner in debug builds; iterator Drop writes Tape back).
- [x] Phase 1: Dual‑write unread batch tail (Tape ring mirrors legacy ring; debug checks present).
- [x] Phase 2: Dual‑write in‑flight preservation for non‑fragmenting tokens (Tape scratch mirrors legacy prefix; debug checks present).
- [x] Phase 3: Shadow read with invariants (`shadow_*` helpers across numbers/strings/escapes; ASCII/char copy counters asserted).
- [x] Phase 4: Whitespace consumption via Scanner (no observable output; parity maintained).
- [ ] Phase 5: Switch strings without escapes (keys/values) to Scanner. (In progress: final emission via Scanner enabled where available.)
- [ ] Phase 6: Switch numbers and literals to Scanner.
- [ ] Phase 7: Escapes (UTF‑8 text) via Scanner.
- [ ] Phase 8: Surrogates and Raw (values) via Scanner.
- [ ] Phase 9: Broaden and compare with a debug‑only token stream comparator harness.
- [ ] Phase 10: Cutover and removal of legacy Buffer/BatchView and parser‑resident buffers.

TODO Checklist (next actionable items)
- [x] Implement Scanner‑driven whitespace: verified no‑output surface; parity holds in tests.
- [x] Route no‑escape strings (final) to Scanner when available; map `TokenBuf` to `LexToken` at closing‑quote.
- [x] Route value partial fragments without escapes to Scanner (shadow emit only, legacy result used). Keep `token_start_pos` semantics intact.
- [x] Shadow numbers and literals in parallel (begin/copy/emit via Scanner) while continuing to return legacy results.
- [x] Instantiate Scanner in release builds.
- [ ] Flip the no‑escape string path unconditionally in release builds (after short bake).
- [ ] Route numbers and `true/false/null` to Scanner; map `TokenBuf` to `LexToken`.
- [ ] Remove Option<Scanner> in ClosedStreamingParser by maintaining a persistent Scanner across closed iteration, then delete the remaining Option.
- [ ] Wire escape handling to Scanner’s `mark_escape` path for text mode.
- [ ] Implement surrogate‑preserving behavior via `ensure_raw` and WTF‑8 output for values.
- [ ] Add debug‑only stream comparator harness to diff legacy vs Scanner token streams for a corpus.
- [ ] After bake period, flip remaining read surfaces to Scanner; then remove legacy Buffer/BatchView and redundant buffers.

Phase 0 — Scaffold (no behavior change)
- [x] Parser struct: add `tape: scanner::Tape` initialized with `Tape::default()`.
- [x] Iterator struct: add `scanner: Option<scanner::Scanner<'src>>` (present in debug builds).
- [x] feed/feed_with: construct `Scanner::from_carryover(mem::take(&mut self.tape), text)` alongside existing `BatchView/Cursor`; scanner is not used for producing events yet.
- [x] Iterator Drop: if `scanner.is_some()`, call `finish()` and write the returned `Tape` back to `parser.tape` in addition to legacy behavior.
- [x] CI/tests: unchanged; behavior identical to legacy.

Phase 1 — Dual write: unread batch tail
- [x] Keep legacy Drop intact.
- [x] Additionally, push the unread tail of the active batch into `tape.ring` in iterator Drop (same slice as the legacy ring push). This ensures future `Scanner` reads see identical unread input.
- [x] Debug assert (debug builds): verify mirrored unread tails against `tape.ring`.

Phase 2 — Dual write: in‑flight preservation for non‑fragmenting tokens
- [x] When dropping the iterator mid‑token for property names and numbers (which never fragment), mirror the legacy “copy already‑read batch prefix into owned buffer” into `tape.scratch`.
- [x] Debug assert (debug builds): when legacy copied into `token_buffer`, check `tape.scratch` matches byte‑for‑byte.

Phase 3 — Shadow read (discard) with invariants
- [x] Drive `Scanner` in lockstep but discard its outputs; legacy path remains the source of truth. Helpers present in `parser/mod.rs`.
- [x] `shadow_begin`, `shadow_peek_eq`, `shadow_advance`, `shadow_copy_while_*`, `shadow_mark_escape`, `shadow_ensure_raw`, `shadow_emit` implemented.
- [x] Positions kept in sync through step‑wise invariants.

Phase 4 — Switch first reads: whitespace
- [ ] Switch only whitespace skipping to use `Scanner`; keep legacy for everything else.
- [ ] Tests: enable a small set that exercises ASCII and optional Unicode whitespace and confirms identical events.

Phase 5 — Strings without escapes
- [ ] For keys and value strings that contain no escapes:
  - Use `scanner.begin(..)`, fast‑path copies, and `scanner.emit_fragment(true, 1)`.
  - Map `TokenBuf::{Borrowed,OwnedText}` to the existing `LexToken` variants; values may fragment across feeds as today.
- [ ] Tests: enable non‑escape string scenarios (single chunk, cross‑feed, mid‑drop) for `Scanner`.

Phase 6 — Numbers and literals
- [ ] Route number DFA and `true/false/null` through `Scanner` for these surfaces.
- [ ] Map `TokenBuf` to number/literal `LexToken`s.
- [ ] Tests: enable number and literal suites for `Scanner`.

Phase 7 — Escapes (UTF‑8 text)
- [ ] On backslash in keys and values: `scanner.mark_escape()`; decode simple escapes and `\uXXXX` scalars and push as text (`push_char`).
- [ ] Keys remain UTF‑8 only; `Scanner` must not emit Raw here unless the backend policy explicitly allows it.
- [ ] Tests: enable escape tests that do not involve surrogate pairs.

Phase 8 — Surrogates and Raw (values)
- [ ] Implement SurrogatePreserving: call `scanner.ensure_raw()` at the first need and append WTF‑8 bytes for lone/reversed surrogates; keep StrictUnicode/ReplaceInvalid semantics identical to legacy.
- [ ] Tests: enable raw backend tests and cross‑feed surrogate pairing (including reversed‑order cases) for `Scanner`.

Phase 9 — Broaden and compare
- [ ] Parameterize a subset of the suite to ensure `Scanner`‑backed surfaces match legacy. Add a debug‑only comparator that records token streams from both paths for a corpus and fails on mismatch with clear diffs.

Phase 10 — Cutover (optional later)
- [ ] Switch remaining reads to the `Scanner` across the board after a bake period. Keep the legacy code path available for one cycle in case of rollback.
- [ ] Remove legacy `Buffer`/`BatchView` and parser‑resident buffer fields.

Acceptance checks per phase
- Build and tests remain green; no public API or behavior changes until the relevant phase switches a read surface.
- New `Scanner` tests are added per phase for the surfaces switched.
- Debug assertions (dev/test builds) catch divergences early without impacting release builds.

Appendix — Out‑of‑Date Items Reconciled
- The earlier idea of a separate `ByteBuffer` type has been superseded by `scanner::Tape`’s internal `VecDeque<u8>` ring; no standalone `ByteBuffer` will be implemented.
