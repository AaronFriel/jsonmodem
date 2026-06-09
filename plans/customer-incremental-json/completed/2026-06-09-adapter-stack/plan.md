# Implementation C: Rust Adapter Stack

This plan is part of `plans/customer-incremental-json/plans.md`. It must stay current as the branch changes.

## Purpose

This branch should test whether the cleanest long-term implementation is a set of Rust adapters that run before Python conversion. After this work, selected string compaction, prefix suffix extraction, live value updates, completed subtree extraction, and byte payload ownership should be explicit Rust operations with thin Python wrappers.

## Worktree

- Path: `/home/friel/c/aaronfriel/jsonmodem-customer-adapters`
- Branch: `customer/adapter-stack`
- Base commit: `47a542760f84dd402cecda6476b56dc92dae54e5`

## Current Status

This branch is a completed evidence branch. Its Rust-first selected-string,
prefix, live-value, completed-subtree, and byte-payload ideas informed the
production branch, but the permanent public API now lives in
`/home/friel/c/aaronfriel/jsonmodem-customer-production`.

## Internal Design Hypothesis

Add native adapters around the existing `JsonModem` parser:

- `SelectedStringCompactor`: receives parser events, matches paths, and emits one decoded delta per lexical string per feed operation.
- `AppendOnlyPrefix`: accepts a current prefix and returns the suffix or an explicit mismatch result.
- `LiveValueUpdater`: updates the existing Rust value tree without creating Python notifications unless requested.
- `CompletedSubtreeExtractor`: emits selected complete subtrees and releases parser-owned memory where safe.
- `BytePayloadPolicy`: decides decoded text, decoded bytes, raw source spans, borrowed payloads, or owned fallback.

Python wrappers should expose enough of these adapters to benchmark, but the main value of this branch is proving whether the Rust layering keeps Python object creation low.

## Scope Through Phase 5

Phase 1 should implement `SelectedStringCompactor` with tests for escapes, Unicode, empty strings, duplicate keys, same-path lexical strings, and wildcard paths.

Phase 2 should implement append-only prefix handling with explicit `Append`, `Repeat`, `ValidatedAppend`, and `Mismatch` outcomes.

Phase 3 should add no-notification live value update and view reads after a feed operation.

Phase 4 should prototype completed selected subtree extraction with release after emission.

Phase 5 should add a byte payload policy that never claims zero-copy for escaped or cross-buffer decoded strings.

## Progress

- [x] Worktree created from `origin/main`.
- [x] Record branch evidence in `plans/customer-incremental-json/records.md`.
- [x] Implement the phase 1 Rust adapter and Python wrapper.
- [x] Add Rust and Python correctness tests.
- [x] Run `.agent/check-py.sh`.
- [x] Prototype phases 2 through 5 or record precise blockers.
- [x] Write branch recommendation and changed file list.

## Validation

Run from the worktree root:

    cargo test -p jsonmodem
    .agent/check-py.sh
    PATH="$HOME/.local/bin:$PATH" .agent/check.sh

The branch must include at least one Rust unit test proving that per-feed compaction happens before Python conversion is involved.

## Decisions

- Decision: make this branch Rust-first.
  Rationale: the customer requirement is about avoiding Python allocations before event emission. A Rust adapter can prove that behavior more directly than compacting already-created Python objects.
  Date/Author: 2026-06-08 / Codex
