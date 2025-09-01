Scanner Refactor — Critic TODO

Purpose
- Track concrete, testable tasks to complete the Scanner refactor described in critic_proposal.md. Check off items as they land.

Milestone 0 — Consistency & Wiring
- [ ] Rename “InputSession” wording to “Scanner” in `crates/jsonmodem/src/parser/scanner/mod.rs` top docs.
- [ ] Cross‑link: add a short pointer from `DESIGN.md` to `critic_proposal.md` and `critic_todo.md`.
- [x] Iterators own `Scanner<'src>` and finalize via `finish()` on Drop.
- [x] Persist `Tape` on parser between feeds.

Milestone 1 — Shadow Parity Scaffolding
- [ ] Ensure debug‑only parity checks compare: source (ring/batch), positions `(pos,line,col)`, and emitted payloads at each emit site.
- [ ] Add a targeted parity test that forces many ring↔batch transitions (short feeds, mixed ASCII/UTF‑8).

Milestone 2 — Flip Surfaces (incremental)
- [x] Keys without escapes: emit via Scanner (borrow vs own), no Raw.
- [ ] Value strings without escapes: borrow when fully in batch; otherwise own; allow fragments across feeds.
- [ ] Numbers: never fragment; borrow when fully in batch; otherwise own. No Raw.
- [ ] Literals (`true/false/null`): route through Scanner for position/advance; ensure parity asserts stay green.

Milestone 3 — Remove Rescans & Legacy Plumbing
- [ ] Delete `BatchView` and `BatchCursor` once string/number paths use byte anchors.
- [ ] Replace all `slice_chars` usages with batch byte spans from `Scanner`.
- [ ] Remove legacy per‑token fields: `token_buffer`, `owned_batch_buffer`, `owned_batch_raw`, `token_is_owned`, `token_start_pos`.

Milestone 4 — Decode Modes Correctness
- [ ] Tests: StrictUnicode — invalid escapes and unpaired surrogates are errors for both keys and values.
- [ ] Tests: ReplaceInvalid — invalid escapes and unpaired surrogates become U+FFFD in UTF‑8 output.
- [ ] Tests: SurrogatePreserving — applies to keys and values; valid pairs joined; lone surrogates preserved as Raw; backend behavior: raw‑capable backends accept Raw, Rust backend errors if required to produce a surrogate‑preserving string.
- [ ] Edge cases: reversed pair order; boundary splits across feeds; ring boundary split of multi‑byte scalars.

Milestone 5 — Fuzzing & Properties
- [ ] Feed‑split fuzz: split every JSON bytestream at every byte (including inside scalars) and assert: (a) no panics/UB, (b) same event sequence as unsplit input.
- [ ] Property tests: “numbers/keys never fragment”; “values may fragment but concatenate to the original”.
- [ ] Add pathological UTF‑8 corpora (short invalid prefixes; mixed valid/invalid; astral plane).

Milestone 6 — Performance Guardrails
- [ ] Bench: ASCII‑heavy batch fast path vs main; ensure within 3% of baseline.
- [ ] Bench: ring path with frequent batch tails; ensure no regression from deque decode.
- [ ] Remove char→byte rescans and confirm bench wins hold after legacy deletion.

Milestone 7 — Cleanups & Docs
- [ ] Remove debug‑only parity assertions once the bake period is complete.
- [ ] Update `README.md`/`DESIGN.md` wording to reflect Scanner ownership/finalization.
- [ ] Add a small “How borrowing works” note to docs.rs with examples.

Risk Watchlist (manual checks during review)
- [ ] Double finalization: `finish(self)` called exactly once per iterator pass.
- [ ] Borrow lifetime: `try_borrow_slice(..)` results never escape the iterator frame.
- [ ] Off‑by‑one on `end_adjust_bytes` when excluding closing quotes.
- [ ] Raw/UTF‑8 transitions: `switch_to_owned_prefix_if_needed()` called before `ensure_raw()`.
- [ ] Ring decode at deque boundary: ≤4‑byte lookahead path is correct.
- [ ] Position accounting: `(pos,line,col)` match legacy after every `advance`.

Definition of Done (must all be checked)
- [ ] All string/number/literal paths use Scanner; legacy buffers and `BatchView/BatchCursor` removed.
- [ ] Fuzz/property tests pass with arbitrary feed boundaries; keys/numbers never fragment; values fragment correctly.
- [ ] All decode‑mode tests pass; keys remain UTF‑8 in all modes.
- [ ] Benches show no meaningful regression; ASCII batch fast path verified.
- [ ] Docs and comments reflect final names and behavior.

Quick Commands
- `cargo nextest run`
