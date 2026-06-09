# Implementation B: Explicit FeedResult API

This plan is part of `plans/customer-incremental-json/plans.md`. It must stay current as the branch changes.

## Purpose

This branch should test whether a result-returning API is clearer and faster than returning iterators of events directly. After this work, a caller should be able to call `feed_many(chunks)` and receive a `FeedResult` object that contains compacted events, changed paths, completed root information, and error state for that feed operation.

## Worktree

- Path: `/home/friel/c/aaronfriel/jsonmodem-customer-feedresult`
- Branch: `customer/feed-result-api`
- Base commit: `47a542760f84dd402cecda6476b56dc92dae54e5`

## Current Status

This branch is a completed evidence branch. The explicit result-object API was
not selected for production because the simpler `JsonModem.feed()` and
`JsonModem.feed_many()` API now covers the required event-stream use cases in
`/home/friel/c/aaronfriel/jsonmodem-customer-production`.

## API Hypothesis

Prototype a separate class or constructor mode rather than overloading the current event iterator too heavily:

    parser = JsonModemFeed(paths=["message"], string_events="per_feed")
    result = parser.feed_many(chunks)
    for event in result.events:
        ...

`feed()` and `feed_many()` must advance the parser eagerly. The caller should not have to iterate a returned object to make parsing happen. This branch should make error behavior explicit: either events before the error are observable in `FeedResult`, or failed feeds are atomic. The initial preference is to make preceding events observable because it matches streaming parser behavior.

## Scope Through Phase 5

Phase 1 should implement per-feed compacted selected string events in `FeedResult.events`.

Phase 2 should add a `PrefixFeedResult` or adapter result that reports `appended`, `ignored`, `validated`, `reset`, or `rejected`.

Phase 3 should add `result.view` and optional `result.changed_paths` for live values without one Python tuple per mutation.

Phase 4 should add `result.completed_subtrees` for selected complete subtrees.

Phase 5 should make payload ownership explicit with fields such as `payload.mode`, `payload.is_view`, and `payload.kind` where `kind` distinguishes decoded text, decoded bytes, and raw JSON source bytes.

## Progress

- [x] Worktree created from `origin/main`.
- [ ] Add branch-local `record.md`.
- [ ] Define `FeedResult` and minimal selected-string phase 1 behavior.
- [ ] Add tests for eager feed execution and error state.
- [ ] Run `.agent/check-py.sh`.
- [ ] Prototype phases 2 through 5 or record precise blockers.
- [ ] Write branch recommendation and changed file list.

## Validation

Run from the worktree root:

    .agent/check-py.sh
    PATH="$HOME/.local/bin:$PATH" .agent/check.sh

The `FeedResult` branch must include tests proving that parser advancement occurs during `feed_many(chunks)`, before the caller iterates over result events.

## Decisions

- Decision: prefer explicit result objects in this prototype.
  Rationale: the requirements repeatedly distinguish parser advancement from Python emission, and a result object can make that boundary visible.
  Date/Author: 2026-06-08 / Codex
