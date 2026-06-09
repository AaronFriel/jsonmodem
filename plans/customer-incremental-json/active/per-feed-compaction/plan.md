# Implementation A: Minimal Per-Feed String Compaction

This plan is part of `plans/customer-incremental-json/plans.md`. It must stay current as the branch changes.

## Purpose

This branch should answer whether the current Python `JsonModem` API can satisfy the mandatory selected-string requirements with the least public API churn. After this work, a caller should be able to select paths, pass several chunks as one application update, and receive at most one compacted decoded string delta per selected lexical string for that feed operation.

## Worktree

- Path: `/home/friel/c/aaronfriel/jsonmodem-customer-compaction`
- Branch: `customer/per-feed-compaction`
- Base commit: `47a542760f84dd402cecda6476b56dc92dae54e5`

## Current Status

This branch is a completed evidence branch. Its useful phase 1 API and native
compaction behavior were folded into
`/home/friel/c/aaronfriel/jsonmodem-customer-production`; production completion
through phases 1 through 5 is tracked in
`plans/customer-incremental-json/active/production-integration/plan.md`.

## API Hypothesis

Keep `JsonModem` as the main event parser and add explicit string emission behavior:

    parser = JsonModem(paths=["message", "items.*.content"], string_events="per_feed")
    events = parser.feed_many(chunks)

`feed(chunk)` should process exactly one scalar chunk. `feed_many(chunks)` should eagerly consume an iterable and use the whole method call as the output compaction boundary. The default remains fragment mode so existing behavior is preserved.

The prefix use case should not be part of `JsonModem.feed()`. If this branch measures prefixes, it should do so as caller-side append validation that passes only newly appended bytes to the normal parser.

## Scope Through Phase 5

Phase 1 is mandatory: selected path filtering and per-feed decoded string compaction before Python object creation.

Phase 2 should be a small adapter prototype only: trusted append-only prefixes parse only the new suffix; validated prefixes report comparison cost; prefix rewrites raise by default.

Phase 3 should add a no-notification update method for `JsonModemValues` or a nearby class if it can be done without destabilizing phase 1.

Phase 4 should prototype completed selected subtree emission for `items.*`, with explicit ownership of emitted Python values.

Phase 5 should reuse or extend `byte_views=True` so borrowed and owned payloads are truthfully labeled. Cross-buffer compaction may return owned output.

## Progress

- [x] Worktree created from `origin/main`.
- [ ] Add branch-local `record.md`.
- [ ] Implement phase 1 API and tests.
- [ ] Run `.agent/check-py.sh`.
- [ ] Add benchmark hooks compatible with the benchmark worktree.
- [ ] Prototype phases 2 through 5 or record precise blockers.
- [ ] Write branch recommendation and changed file list.

## Validation

At minimum, run from the worktree root:

    .agent/check-py.sh
    PATH="$HOME/.local/bin:$PATH" .agent/check.sh

Also run the shared selected-string pyperf benchmarks once the benchmark branch provides them.

## Decisions

- Decision: preserve fragment mode as the default.
  Rationale: the customer requires current fragment behavior to remain observable and comparable.
  Date/Author: 2026-06-08 / Codex
