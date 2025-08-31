Critique of Proposals for crates/jsonmodem/src/parser/mod.rs

Context: current parser architecture (what exists)
- Ring-first, batch-borrow: The lexer drains `source: Buffer` (ring) first, then reads directly over the current `&'src str` batch. Borrowed string/number slices are taken only from the batch; ring-sourced content is always owned. This is already implemented.
- Per-token owned buffers: The parser carries multiple scratch buffers and flags: `token_buffer` (ring/owned), `owned_batch_buffer` (batch/owned), `owned_batch_raw` (raw WTF-8), plus coordination flags like `token_is_owned`, `token_is_raw_bytes`, `string_had_escape`, `token_start_pos` and counters (`BatchCursor` with char/byte counts). Iterator `Drop` preserves in-flight content and pushes unread batch tail into the ring. Borrowed slicing is obtained via char-index to byte scan (`BatchView::slice_chars`).
- Event surface and invariants: Numbers and property names never fragment; string values may fragment; decode modes include surrogate-preserving; capital `\U` is optionally allowed. LexToken already distinguishes owned vs borrowed payloads.

High-level takeaways
- Most proposals restate the existing behavior; the real opportunities are simplifying state/merging buffers, clarifying borrow/own transitions, replacing repeated char→byte rescans with byte marks, and tightening drop/finalize responsibilities to avoid borrow-checker pitfalls.
- Designs that depend on RAII mutating the parser in `Drop` without an explicit finalize step are fragile in safe Rust. Designs that keep a non-owning cursor and use iterator-drop for spill/preservation align better with the current implementation.

Proposal A (LexInput + TokenCapture with RAII Drop)
Unique/Good ideas
- Consolidates “where we read from” (LexInput) and “how we materialize a token” (TokenCapture). Clear mark/commit/take lifecycle per token.
- Pushes toward a single owned scratch concept per token and explicit upgrade points on escape/cross-batch.
- Calls out the need to track batch byte offsets (mark_bytes) to avoid rescans.

Critical issues
- RAII Drop that “returns the ring to the parser, pushes unread tail, and preserves in-flight” requires a mutable handle to the parser in Drop or interior mutability. Safe Rust Drop cannot write back to `parser.source` unless the object holds `&mut Parser` for the entire iteration, which conflicts with borrowing the parser elsewhere. The proposal acknowledges risks but still centers on RAII.
- Ephemeral `TokenCapture`: unless it borrows parser-owned scratch, progress is lost on iterator drop. The mitigation (“make TokenCapture a view over parser-owned scratch”) drifts back toward the current design and complicates lifetimes.
- Slice mapping claims “O(1)” but only if start byte is stored. The sketch also shows char-based positions; care is required to ensure both stay coherent. No concrete plan to remove `slice_chars` rescans.

Verdict
- Contains solid local APIs (mark/commit/take; borrow_prefix vs copy_prefix) and the byte-mark idea, but the RAII-based ownership/write-back story will fall apart at implementation time without reworking to an explicit finalize. As written, not worth committing; worth salvaging: the TokenCapture API shape and byte-marking.

Proposal B (InputCursor + TokenCapture; RAII-ish)
Unique/Good ideas
- Similar to A, but crisper API delineations and a stronger distinction between `borrow_prefix_and_switch_to_owned` (values) vs `upgrade_copy_prefix` (keys). This separation is practical and prevents duplicate fragment bugs.
- Explicitly tracks `bytes_consumed` for O(1) slice boundaries; pushes toward removing `slice_chars`.

Critical issues
- Still relies on Drop to “return the ring to the parser and push unread tail.” Since `InputCursor` owns the ring (via `mem::take`), Drop cannot restore it into the parser without also holding `&mut Parser`. The mitigation (“store location locally and assign back on drop”) does not address restoring the ring. This is a fundamental flaw unless replaced with explicit `finish()`.
- In-flight persistence is only outlined; it implicitly assumes a persisted state in the parser, which is fine, but that needs to be first-class.

Verdict
- Good local APIs and the key distinction for value vs key prefix handling. However, the RAII write-back assumption is incorrect; commit only if combined with an explicit finalize (not Drop) and parser-owned in-flight state. On its own, discard.

Proposal C (InputSession + Capture; RAII Drop)
Unique/Good ideas
- Names the two core concerns correctly (session vs capture) and restates the correct invariants. Emphasizes decoupling the FSM from ownership mechanics.

Critical issues
- Same Drop caveat as A/B. It handwaves over “on drop, push unread tail into the ring and return ring to parser,” which is not viable without an explicit finalize or borrowing the parser mutably for the entire iteration.
- “If `next()` exits while a Capture is active, the caller chooses…” conflates Drop-time behavior with explicit control flow; this is where bugs crop up. Either capture persists in the parser, or borrowed prefixes are lost.

Verdict
- Conceptually aligned with A/B; implementation would still need explicit finalize and parser-resident in-flight state. As a standalone, discard; salvage capture/session API ideas only.

Proposal D (Non-RAII; explicit finalize; parser-resident minimal in-flight)
Unique/Good ideas
- Directly calls out why RAII fails: Drop can’t safely write back into the parser without awkward aliasing. Proposes `finish(self) -> (ring, bytes_used, pos, line, col)` and explicit write-back by the caller. This is realistic and implementable.
- Keeps iterator-drop responsible for (a) preserving in-flight progress (copy current batch prefix to owned) and (b) pushing unread tail; proposes small helpers for these.
- Tightens the spec around byte/char accounting (add batch-global span if needed) and removes the “O(1) without marks” handwave; acknowledges char→byte mapping costs unless byte marks are stored.

Critical notes
- Starts with RAII but self-corrects to explicit finalize. That self-critique is on point and improves feasibility.
- Still loose on the exact shapes of the persistent token state and scratch, but the direction (single TokenBuf + ensure_raw migration) is consistent with current needs.

Verdict
- The most grounded of the single proposals. Worth committing to the explicit finalize + small helpers pattern; borrow the `TokenBuf`/`ensure_raw` consolidation too.

Pairwise combinations (AB, BC, CD, DA)
- AB: Keeps A/B’s cursor+capture and adds explicit parser-owned in-flight scratch. It retains RAII Drop but with clearer lifetime considerations. The best parts are the explicit `borrow_prefix_and_switch_to_owned` vs `upgrade_from_borrow` split and the clear “A-state forces owned for the token” rule. Still weak where it assumes Drop can write back to the parser.
- BC: Refines the API split, emphasizes O(1) byte slicing, and clearly enumerates risks. It embraces persistent in-flight state and debug assertions for UTF‑8 boundaries. Again, Drop semantics still glossed over. Good API surface; needs finalize.
- CD: Moves to a non-owning SourceCursor and persistent TokenCapture on the parser; keeps iterator-drop as the place to spill unread batch tail. This aligns well with the current code and avoids `mem::take`/RAII tricks. Strong improvement over C/D without the Drop footgun.
- DA: Adopts explicit finalize (good), introduces a single TokenScratch (good), keeps capture a thin controller over parser-resident scratch (good), and limits Drop to preservation + tail spill. This is implementable and aligns with current structure.

Final combinations (ABCD, BCDA, CDAB, DABC)
- ABCD: Fully incorporates explicit `finish()` (no RAII) plus TokenCapture and in-flight persistence. This is a good unification of D’s realism with A/B’s useful APIs. It still leans on moving the ring out (`mem::take`) and handing it back via `finish()`. Implementable, but requires more reshaping than a non-owning cursor.
- BCDA: Similar to ABCD but clearer about parser-persistent `TokenState + TokenScratch` and an ephemeral `CaptureHandle`. Uses explicit `finish()`. A solid, implementable plan with good separation of concerns.
- CDAB: The most conservative refactor: non-owning `SourceCursor` that borrows parser ring and positions, `TokenState` persisted in the parser, and iterator-drop for preservation/spill. This maps closely onto the current implementation, but simplifies buffers/flags and introduces byte marks for O(1) slicing. Minimal risk and the easiest to land incrementally.
- DABC: Another strong variant: explicit finalize (`InputGuard::finish()`), single `TokenScratch`, and parser-persistent `TokenCapture`/state. Avoids RAII, keeps invariants crisp, and has a concrete migration plan (numbers first, then strings). Also implementable, a bit more invasive than CDAB.

Cross-cutting claims checked against reality
- RAII write-back to the parser: Not viable in safe Rust without keeping `&mut Parser` borrowed for the iterator’s lifetime, which collides with other borrows. Prefer explicit `finish()` or a non-owning cursor plus iterator-drop helpers. Designs relying on Drop alone are to be discarded or rewritten.
- O(1) batch slicing: Only if we store the batch start byte at token start and maintain a running batch byte cursor. Current code rescans via `char_indices`. Several proposals (B, AB, BC, CDAB, etc.) correctly push for storing `start_byte` and using `bytes_consumed` → this is worth adopting.
- In-flight persistence: Current code already preserves mid-token prefixes on iterator drop using `token_buffer` and sets `token_is_owned`. Proposals that rely on ephemeral capture values without parser-resident state will lose progress. Prefer a small `TokenState` persisted in the parser, with a single `TokenScratch` (text/raw) and flags (`started_in_ring`, `had_escape`, and `start_batch_byte`).
- Keys/numbers never fragment; strings may: Already true. The valuable addition is to differentiate operations: `borrow_prefix_and_switch_to_owned` (string values only) vs `upgrade_copy_prefix` (keys). This reduces duplication bugs and makes intent explicit.
- Surrogate-preserving/raw mode: Already present with `owned_batch_raw` and helpers. Proposals to unify this into a single `TokenScratch` with `ensure_raw()` that migrates once are spot-on, and map well onto the current code’s `ensure_raw_mode_and_move_buffers` behavior.
- Lifetimes of borrowed events: The batch must be held by the iterator. Designs that keep the batch in the iterator and pass a non-owning cursor/capture through are sound. Designs that try to encapsulate the batch inside a dropped guard must return borrowed slices before that guard is dropped or provide explicit finalize.

What to commit vs discard

Commit (or adapt):
- Non-owning cursor + explicit finalize or existing iterator-drop: Adopt CDAB (preferred) or BCDA as the architectural baseline. CDAB meshes with the current code: keep iterator-drop handling, add a non-owning `SourceCursor` and parser-persistent `TokenState` + `TokenScratch`. If we prefer moving the ring out during `next()`, then ABCD/BCDA’s explicit `finish()` is workable; avoid RAII Drop.
- Single TokenScratch + ensure_raw(): Collapse `token_buffer`, `owned_batch_buffer`, and `owned_batch_raw` into one enum. Keep a single set of flags in `TokenState` (`started_in_ring`, `start_batch_byte`, `had_escape`, `kind`).
- Byte-marked borrow slicing: Record `start_batch_byte` at token start; use the running `batch_bytes` cursor to slice borrowed strings/numbers without rescans. Remove `slice_chars` from hot paths.
- Distinct prefix operations: Implement `borrow_prefix_and_switch_to_owned` (string values) and `upgrade_copy_prefix` (keys) to prevent duplicate fragment emission and to respect non-fragmenting keys.
- Iterator-drop helpers: Keep iterator-drop responsible for (a) preserving in-flight prefixes into `TokenScratch` and (b) pushing unread tail into the ring. Provide small, testable helpers instead of burying behavior in RAII.

Discard (or rewrite):
- RAII Drop that mutates parser state (A/B/C as written): Non-starter in safe Rust without complex aliasing. Replace with explicit `finish()` or non-owning cursor + iterator-drop.
- Ephemeral capture-only persistence: Any design that does not store in-flight state in the parser risks losing progress or duplicating fragments across feeds.
- O(1) slicing claims without storing byte marks: If `start_byte` isn’t recorded, we fall back to rescans; adopt byte marks or keep `slice_chars` as a fallback but do not oversell.

Recommended incremental plan (grounded in current code)
1) Introduce `TokenScratch` (Text/String vs Raw/Vec<u8>) and route all owned accumulation through it. Refactor `ensure_raw_mode_and_move_buffers` into `TokenScratch::ensure_raw()`.
2) Introduce a small `TokenState` on the parser: `{ kind, started_in_ring, start_batch_byte: Option<usize>, had_escape, scratch: TokenScratch }`. Replace scattered flags (`token_is_owned`, `string_had_escape`, `token_is_raw_bytes`) and dual owned buffers.
3) Add byte marks: record `start_batch_byte` when a token begins in the batch; use `BatchCursor.bytes_consumed` (already tracked) to slice borrowed fragments. Remove `BatchView::slice_chars` from hot paths.
4) Add explicit operations at the call sites: `borrow_prefix_and_switch_to_owned` for string values; `upgrade_copy_prefix` for keys. Keep numbers non-fragmenting.
5) Keep iterator-drop responsibilities but replace ad-hoc code with helpers that (a) copy any in-batch prefix into `TokenScratch` and (b) push unread tail into the ring.
6) Optional: If we prefer the “own the ring during next()” style, add an explicit `finish()` (ABCD/BCDA pattern). Otherwise, stick with a non-owning `SourceCursor` (CDAB pattern) that updates positions directly and preserves current iterator-drop workflow.
7) Later optimization: consider a byte ring with an UTF‑8 decoder; this is orthogonal and should not be part of the first refactor.

Closing
- The proposals converge on the same target: simplify ownership switching, centralize token capture, and make borrowed slicing cheap via byte marks. The variants that avoid RAII back-references (CDAB, BCDA, ABCD, DABC) are implementable; the RAII-heavy sketches (A, B, C) will run into borrow-checker walls unless rewritten to explicit finalize and parser-resident state. The lowest-risk path is CDAB (non-owning cursor + parser-persistent token state) with the concrete improvements listed above.

