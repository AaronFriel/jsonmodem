# Benchmark and Recommendation Worktree

This plan is part of `plans/customer-incremental-json/plans.md`. It must stay current as benchmark fixtures, scripts, and reports change.

## Purpose

This branch should build the evidence used to choose between the implementation prototypes. After this work, a reviewer should be able to run pyperf benchmarks for selected string extraction, prefix inputs, live values, completed subtrees, byte payloads, errors, no-match filtering, feed overhead, memory scaling, and caller-owned batching. The final report should recommend which API and implementation to adopt.

## Worktree

- Path: `/home/friel/c/aaronfriel/jsonmodem-customer-benchmarks`
- Branch: `customer/requirements-benchmarks`
- Base commit: `47a542760f84dd402cecda6476b56dc92dae54e5`

## Current Status

This branch is a completed evidence branch. The benchmark scripts were ported
and extended in
`/home/friel/c/aaronfriel/jsonmodem-customer-production`; final production
artifacts are under `target/customer-incremental-json/` in that worktree and
summarized in `plans/customer-incremental-json/records.md`.

## Benchmark Scope

Implement fixture generation and benchmark groups corresponding to customer benchmarks A through K. Start with A, B, H, I, and K because phase 1 depends on them. Then add C for prefix adapters, D for live values, E/G/J for completed subtrees and memory, and F for byte payloads.

Competitors must be compared only when output semantics are comparable. Include current `jsonmodem`, implementation branches, `jiter` cumulative partial prefixes where appropriate, Python `json`, `orjson`, and at least one named partial JSON package. Do not present a partial parser that returns a full value as equivalent to selected streaming deltas.

Every benchmark must compute a checksum over event type, path, payload length, ownership/final state where relevant, and errors. Benchmarks that discard output without a checksum are invalid.

## Required Artifacts

- Fixture generators with deterministic seeds.
- `pyperf` benchmark scripts and a fast smoke mode.
- Memory measurement scripts that record RSS separately from `tracemalloc`.
- Raw JSON output files under `target/customer-incremental-json/`.
- A report in `plans/customer-incremental-json/records.md` or a linked branch-local report summarizing results by implementation branch.

## Progress

- [x] Worktree created from `origin/main`.
- [x] Record branch evidence in `plans/customer-incremental-json/records.md`.
- [x] Implement fixture generators for benchmarks A, B, H, I, and K.
- [x] Add current-main baseline benchmark results.
- [x] Add competitor setup and version recording.
- [x] Add benchmark commands for each implementation worktree.
- [x] Add memory measurement harness.
- [x] Write final recommendation.

## Validation

Run from the worktree root:

    .agent/check-py.sh
    .venv/bin/python crates/jsonmodem-py/benchmarks/<new-script>.py --fast --output target/customer-incremental-json/<name>.json
    .venv/bin/python -m pyperf check target/customer-incremental-json/<name>.json

The benchmark branch must record package versions and command lines for every published result.

## Decisions

- Decision: benchmark output semantics separately.
  Rationale: selected streaming deltas, cumulative-prefix partial values, live value snapshots, completed subtrees, and borrowed byte spans answer different application questions. Combining them would make the fastest result misleading.
  Date/Author: 2026-06-08 / Codex
