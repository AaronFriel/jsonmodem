# Production Integration

This plan owns `/home/friel/c/aaronfriel/jsonmodem-customer-production` on branch `codex/customer-incremental-json-production`.

The goal is to ship the customer incremental JSON requirements through phases 1 through 5 as production-ready Python bindings and Rust support. The older compaction, feed-result, adapter-stack, and benchmark branches provide evidence and code to inspect, but this branch must make the final API coherent and tested.

## Scope

- Public Python API for event streaming, caller-side prefix guidance, live values, completed subtrees, and byte payload modes.
- Rust and PyO3 implementation needed to avoid Python object creation on unselected values and compacted selected strings.
- Tests for correctness, lifecycle, errors, path behavior, Unicode, escaped strings, multiple roots where enabled, duplicate keys, and output ownership labels.
- Documentation in the Python README and type stubs.
- Benchmark integration with the requirements benchmark suite and final recommendation records.

## Non-Goals

- Optimizing `loads()` or positioning jsonmodem as a complete-document decoder.
- Adding parser-owned timers, async stream consumption, retry logic, or transport-specific helpers.
- Claiming borrowed or zero-copy output unless a returned Python object truthfully holds a defensible buffer owner.

## Milestones

1. Phase 1 selected strings: port the native compaction behavior behind `JsonModem(..., string_events="per_feed")`, preserve fragment mode, and add `feed_many(chunks)`.
2. Phase 2 cumulative-prefix guidance: document caller-side append validation and keep production input as normal delta feeds.
3. Phase 3 live values: expose a reused read-only view, explicit snapshot, and optional changed-path summary without per-mutation tuple allocation.
4. Phase 4 completed subtrees: emit selected completed values as owned Python values, preserve document order, handle truncation, and measure retained memory.
5. Phase 5 byte payloads: expose decoded text, decoded bytes, and raw source output with owned/borrowed labels and source-buffer lifetime documentation.
6. Benchmarks and validation: run targeted tests, `.agent/check-py.sh`, `.agent/check.sh`, pyperf checks, and benchmark groups A through K where applicable.
7. PR review: open the production PR, request Codex review, address relevant comments, and move this plan to `completed/` only when the review goal is done.

## Current Status

- [x] Worktree created from `origin/main` at `47a542760f84dd402cecda6476b56dc92dae54e5`.
- [x] Phase 1 code ported.
- [x] Phase 1 targeted tests pass.
- [x] Phase 2 code complete.
- [x] Phase 3 code complete.
- [x] Phase 4 code complete.
- [x] Phase 5 code complete.
- [x] Benchmark groups A through K run or explicitly marked inapplicable with evidence.
- [x] Full required validation passes.
- [x] Public `JsonModemPrefixes` and `PrefixResult` API removed.
- [x] A-K smoke evidence refreshed without public prefix rows.
- [x] PR #73 opened and Codex review requested.
- [x] First Codex review comment addressed.
- [x] Plan moved to `completed/` together with goal completion.

## Record Format

`record.md` stores dated entries with command, changed files, observation, artifact path, and consequence. Benchmark entries must include Python version, Rust version, package versions, exact command, fixture group, output semantics, checksum status, and artifact path.
