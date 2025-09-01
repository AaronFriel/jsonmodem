Scanner Refactor — TODO (Minimal, Test-Preserving)

Principles
- Keep current tests passing (use cargo nextest run).
- Make changes in small, reversible steps.
- Standardize on “emit, then advance”; remove delimiter/end_adjust footguns.

Phase 0 — API Prep (Scanner)
- [x] Add emit_final() to emit final fragment and clear anchor.
- [x] Add emit_partial() to emit non-empty partial and acknowledge borrowed prefixes.
- [x] Add yield_prefix() to split at transform boundaries for Allowed strings.
- [x] Add own_prefix() to force copy-once to owned mode (idempotent).
- [ ] Document “emit, then advance” discipline in scanner docs and comments.

Phase 1 — Parser Minimal Wiring
- [ ] Close-quote sites: use scanner.emit_final(); then advance the quote.
- [ ] Escape boundary (values): replace mark_escape + manual prefix copies with
      if let Some(p) = scanner.yield_prefix() { emit partial } else { scanner.own_prefix(); }.
- [ ] Feed-end partial (values): replace try_borrow_slice + acknowledge + emit(false, ..) with scanner.emit_partial().
- [ ] Keep keys/numbers behavior identical (Disallowed, no partials); do not refactor them yet.

Phase 2 — Cleanups (still test-preserving)
- [ ] Remove all end_adjust usages and delimiter-dependent emits.
- [ ] Delete acknowledge_partial_borrow call sites in parser.
- [ ] Collapse legacy manual copying paths now covered by yield_prefix/own_prefix.

Phase 3 — Keys and Numbers via Scanner
- [ ] Begin(Disallowed) for numbers and keys; emit only via scanner.emit_final() at completion.
- [ ] Ensure numbers never produce Raw; keys degrade/validate per backend rules.

Phase 4 — Prune Legacy Plumbing
- [ ] Remove BatchView/BatchCursor rescans at emit sites (use Scanner’s byte anchors).
- [ ] Remove duplicate per-token buffers in parser once string/number paths rely on Scanner scratch.

Phase 5 — Tests & Edge Cases
- [ ] Add tests for: partial emission acknowledgment; ring→batch split mid-string; reversed surrogate pairs; raw transitions.
- [ ] Ensure Disallowed tokens coalesce correctly on iterator drop (finish()).

Phase 6 — Perf & Docs
- [ ] Bench ASCII batch fast path and ring digit runs; verify no regressions.
- [ ] Update README/DESIGN to describe Scanner ownership and finish() semantics.
- [ ] Add a “Borrowing rules” doc snippet with borrow vs owned examples.

Quick Commands
- Run: cargo nextest run
- Focus a test: cargo nextest run -- package jsonmodem -- tests

