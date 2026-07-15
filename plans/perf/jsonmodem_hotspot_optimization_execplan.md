# Reduce JsonModem Hotspot Costs with Targeted Benchmarks and Verified Improvements

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must be kept up to date as work proceeds.

Refer to `PLANS.md` in the repository root for governing standards. This document is authored and maintained in accordance with those requirements.

## Purpose / Big Picture

This plan turns hotspot profiling findings into measurable improvements. It adds focused benchmarks that isolate parser lex/scanner work, iterator finalization overhead, and values/buffers assembly work with less end-to-end harness noise. It also preserves baseline end-to-end timing for the existing medium and large streaming benchmarks so we can prove overall impact, not only microbench gains.

After this plan, a contributor can:

1. Run targeted benchmarks and collect baseline metrics for each hotspot area.
2. Apply focused optimizations while maintaining parser correctness and adapter behavior.
3. Re-run the same benchmark set and produce a before/after report covering each hotspot and overall medium/large parsing throughput.

## Progress

- [x] (2026-02-22 00:31Z) Confirmed hotspot areas and profiling evidence from prior flameview/perf run.
- [x] (2026-02-22 00:34Z) Ran required `.agent/check.sh` baseline check; discovered pre-existing clippy failure in parser tests (`manual_flatten`), unrelated to this plan’s changes.
- [x] (2026-02-22 00:36Z) Started this optimization ExecPlan and locked implementation scope.
- [x] (2026-02-22 00:40Z) Added `hotspot_targets` benchmark and registered it in `crates/jsonmodem/Cargo.toml`.
- [x] (2026-02-22 00:52Z) Implemented parser/scanner/drop-path and values/container assembly optimizations in planned files.
- [x] (2026-02-22 01:08Z) Captured authoritative baseline vs optimized benchmark data with stabilized Criterion settings (`--measurement-time 5 --sample-size 50`) and archived outputs under `/tmp/jsonmodem_execplan_authoritative_20260222_stable/`.
- [x] (2026-02-22 01:10Z) Refreshed post-change CPU and allocation-focused flameview artifacts for all three flavors at `/5000` under `/tmp/jsonmodem_profiles_perf_after_20260222_010934/`.
- [x] (2026-02-22 01:12Z) Wrote before/after report with flameview self-serve workflow in `plans/perf/jsonmodem_hotspot_optimization_report.md`.
- [x] (2026-02-22 01:13Z) Re-ran `.agent/check.sh`; build/tests pass and clippy still fails only on pre-existing parser test lint (`manual_flatten`).
- [x] (2026-02-22 09:40Z) Completed deep-dive reprofiling of user-called hotspot frames (`consume_string_ascii_fast`, `push_ascii_to_scratch`, `push_key_from_str`, iterator drop/`finish`) and added findings to the report.

## Surprises & Discoveries

- Observation: CI-equivalent local check is not fully green before this effort due to a pre-existing clippy lint failure in parser tests.
  Evidence: `.agent/check.sh` failed at `crates/jsonmodem/src/parser/tests.rs:1327` with `clippy::manual_flatten`.

- Observation: In this environment, direct tracefs-based allocation probes are permission constrained, so non-root allocation analysis must rely on allocation-focused stack slicing from CPU profiles.
  Evidence: `perf probe` and `perf record -e sdt_libc:*` permission errors in prior profiling run.

- Observation: Benchmark noise was high when using `JSONMODEM_BENCH_FAST=1` (10 ms warmups), and sequential runs produced contradictory before/after outcomes.
  Evidence: repeated captures in `/tmp/jsonmodem_hotspot_baseline_20260222_after*` contained large swings, including implausible reversals between runs.

- Observation: Switching to longer Criterion sampling windows produced stable, plausible deltas aligned with profile-level behavior.
  Evidence: authoritative dataset in `/tmp/jsonmodem_execplan_authoritative_20260222_stable/comparison.tsv` shows coherent medium/large and targeted trends.

- Observation: In deep-dive runs, allocator cost remained concentrated under string scratch growth and iterator drop carryover work, and experimental mitigations were either regressive or inconclusive in this harness.
  Evidence: `/tmp/jsonmodem_deep_dive_profile_20260222_093319/` plus deep-dive benchmark captures under `/tmp/jsonmodem_deep_dive_20260222_*`.

## Decision Log

- Decision: Create a new ExecPlan file instead of modifying `plans/perf/jsonmodem_flameview_profile_execplan.md`.
  Rationale: the prior plan is operational profiling workflow documentation; this effort adds optimization implementation, targeted benchmarks, and comparative reporting.
  Date/Author: 2026-02-22 / Codex.

- Decision: Treat parser lex/scanner, iterator finalization, and values/buffers assembly as the three mandatory optimization buckets, with object-container handling folded into assembly optimizations.
  Rationale: this aligns with measured hotspot concentration while keeping implementation scope tractable and verifiable in one engineering pass.
  Date/Author: 2026-02-22 / Codex.

- Decision: Produce authoritative before/after metrics from a clean detached baseline worktree plus benchmark harness only, then run identical commands on optimized code.
  Rationale: this avoids relying on noisy prior runs and keeps baseline code truly pre-optimization while preserving identical benchmark entrypoints.
  Date/Author: 2026-02-22 / Codex.

- Decision: Use stable measurement settings (`--measurement-time 5 --sample-size 50`) for final reporting instead of fast mode.
  Rationale: longer warmup/measurement cycles materially reduced variance and yielded reproducible directional conclusions.
  Date/Author: 2026-02-22 / Codex.

- Decision: Keep deep-dive experiments out of the committed implementation unless they show consistent wins across hotspot and e2e benches.
  Rationale: interning and reserve-heuristic experiments were not consistently positive and risked regressions in object assembly and end-to-end cases.
  Date/Author: 2026-02-22 / Codex.

## Outcomes & Retrospective

Implementation completed with measurable improvements in targeted hotspots and `streaming_json_medium`, with mixed outcomes on `streaming_json_large` by flavor. Detailed numbers and reproducible commands are in `plans/perf/jsonmodem_hotspot_optimization_report.md`.

Headline outcomes from the authoritative dataset (`/tmp/jsonmodem_execplan_authoritative_20260222_stable/comparison.tsv`):

- Targeted hotspot aggregate: `-2.289%` over 5 cases.
- `streaming_json_medium` aggregate: `-3.028%` over 9 cases.
- `streaming_json_large` aggregate: `-0.228%` over 9 cases (near-flat overall).

By flavor on `streaming_json_large`:

- `jsonmodem_buffers`: improved (`-2.512%` aggregate).
- `jsonmodem_events`: essentially flat (`+0.122%` aggregate).
- `jsonmodem_values`: slight regression (`+1.705%` aggregate), still within a tight band but directionally behind medium gains.

Safety/conformance status:

- `.agent/check.sh` confirms formatting/build/tests pass.
- clippy remains red only on pre-existing `crates/jsonmodem/src/parser/tests.rs:1327` (`clippy::manual_flatten`), not introduced by this plan.

Retrospective:

- The added targeted bench gave actionable isolation for parser/drop/assembly improvements.
- The biggest remaining optimization opportunity is values-path large payload behavior (`next_value_for_source` plus value applicator/zipper allocation-heavy paths), corroborated by post-change flameview captures.
- Additional deep-dive profiling confirms user-highlighted frames are still allocation-heavy and should be addressed with focused structural changes (scratch/carryover storage strategy, property-name path handling), not ad-hoc reserve tweaks.

## Context and Orientation

The hotspot evidence comes from perf/flameview captures over `streaming_json_large/*/5000` and points to:

- Parser lex/scanner core:
  - `crates/jsonmodem/src/parser/mod.rs` (`next_event_with`, `lex_state_step`, `dispatch_parse_state`)
  - `crates/jsonmodem/src/parser/scanner/mod.rs` (`consume_string_ascii_fast`, `finish`)
- Iterator finalization/drop path:
  - `crates/jsonmodem/src/parser/mod.rs` (`Drop for JsonModemIterator` and `JsonModemClosed`)
- Values/buffers assembly:
  - `crates/jsonmodem/src/jsonmodem_values.rs`
  - `crates/jsonmodem/src/jsonmodem_buffers.rs`
  - `crates/jsonmodem/src/backend/std/value_applicator.rs`
  - `crates/jsonmodem/src/backend/std/value_zipper.rs`

Existing end-to-end benchmark binaries already live in `crates/jsonmodem/benches/`, especially `streaming_json_medium.rs` and `streaming_json_large.rs`.

## Plan of Work

First, add a new targeted Criterion benchmark binary under `crates/jsonmodem/benches/` that provides low-noise measurements for:

- parser string-lex/scanner-heavy workload using the events adapter;
- iterator/feed finalization overhead with tiny chunks and repeated feed/drop cycles;
- buffer/value assembly-heavy workload using object/string-rich payloads.

The benchmark should keep setup outside the inner iteration and avoid unrelated conversion work where possible.

Second, record baseline timings for:

- all targeted benchmark functions;
- existing end-to-end `streaming_json_medium` and `streaming_json_large` jsonmodem flavor cases.

Third, implement low-risk optimizations mapped to each hotspot bucket. Initial optimization candidates:

- scanner/parser path: reduce overhead in string fast-path scanning and session finalization fast paths;
- iterator finalization: reduce unnecessary work when no carryover needs to be persisted;
- assembly path: reduce redundant work in values classification and container initialization/mutation paths.

Fourth, re-run the exact same benchmark commands and compare before/after numbers. Produce a report in `plans/perf/` summarizing area-level and end-to-end changes with commands and outputs.

Finally, run project checks to validate conformance and document any pre-existing failures separately from newly introduced regressions.

## Concrete Steps

Run from repository root (`/home/friel/c/aaronfriel/jsonmodem-perf`).

1. Add targeted benchmark source and register `[[bench]]` entry.

2. Collect baseline benchmark data:

    # Baseline from clean detached worktree at pre-change HEAD:
    cargo bench --package jsonmodem --bench hotspot_targets -- --measurement-time 5 --sample-size 50
    cargo bench --package jsonmodem --bench streaming_json_medium -- --measurement-time 5 --sample-size 50
    cargo bench --package jsonmodem --bench streaming_json_large -- --measurement-time 5 --sample-size 50

3. Apply hotspot-focused optimizations in parser/scanner and values/buffers code paths.

4. Re-run the same three commands and capture after metrics.

5. Optionally refresh CPU and allocation-focused folded stacks for the targeted benchmark cases:

    cargo bench --package jsonmodem --bench streaming_json_large --no-run
    BIN=$(find target/release/deps -maxdepth 1 -executable -name 'streaming_json_large-*' | head -n 1)
    perf record -F 400 --call-graph dwarf,64000 -o /tmp/profile.perf.data -- "$BIN" --bench jsonmodem_values/5000 --profile-time 5
    perf script -i /tmp/profile.perf.data | inferno-collapse-perf > /tmp/profile.folded
    flameview --summarize --max-lines 60 /tmp/profile.folded
    flameview /tmp/profile.folded

6. Write comparison report under `plans/perf/` with before/after numbers and interpretation.

7. Run checks:

    .agent/check.sh

## Validation and Acceptance

Acceptance requires all of the following:

- Targeted benchmark binary exists and runs successfully in release bench mode.
- Before and after timing data exists for each hotspot-targeted benchmark function.
- Before and after timing data exists for both end-to-end benchmarks (`streaming_json_medium`, `streaming_json_large`) for jsonmodem events/buffers/values cases.
- No correctness regressions in parser/buffers/values tests attributable to this work.
- Final report clearly states improvements or regressions per area and overall.

## Idempotence and Recovery

Benchmark commands are repeatable and safe; use `JSONMODEM_BENCH_FAST=1` for quicker cycles. If a specific optimization regresses correctness or performance, revert that change independently and re-run only the affected benchmark and test subset before full checks.

Because `.agent/check.sh` currently fails on a pre-existing clippy lint in tests, this plan records that failure explicitly and treats new regressions separately.

## Artifacts and Notes

Artifacts produced by this plan:

- New targeted benchmark source in `crates/jsonmodem/benches/`.
- Updated `crates/jsonmodem/Cargo.toml` bench registration.
- Before/after metrics report in `plans/perf/jsonmodem_hotspot_optimization_report.md`.
- Authoritative benchmark captures and parsed comparisons:
  - `/tmp/jsonmodem_execplan_authoritative_20260222_stable/baseline/`
  - `/tmp/jsonmodem_execplan_authoritative_20260222_stable/after/`
  - `/tmp/jsonmodem_execplan_authoritative_20260222_stable/comparison.tsv`
- Post-change perf/flameview artifacts:
  - `/tmp/jsonmodem_profiles_perf_after_20260222_010934/`
- Deep-dive hotspot artifacts:
  - `/tmp/jsonmodem_deep_dive_profile_20260222_093319/`
  - `/tmp/jsonmodem_deep_dive_20260222_015100/`
  - `/tmp/jsonmodem_deep_dive_20260222_015100_repeat/`
  - `/tmp/jsonmodem_deep_dive_20260222_015100_revert_finish_fastpath/`
  - `/tmp/jsonmodem_deep_dive_20260222_015100_revert_all_experiments/`

## Interfaces and Dependencies

Benchmark interfaces:

- Existing: `streaming_json_medium`, `streaming_json_large`.
- New: `hotspot_targets` (to be added by this plan).

Primary dependencies:

- `criterion` for timing measurements.
- `perf`, `inferno-collapse-perf`, `inferno-flamegraph`, `flameview` for hotspot verification when needed.

Code interfaces likely touched:

- `crate::parser::scanner::Scanner`
- `crate::parser::JsonModemIterator` / `JsonModemClosed` drop/finalization paths
- `crate::jsonmodem_values` emit/classification helpers
- `crate::backend::std::value_applicator` and possibly `value_zipper` container/value mutation paths

Change Note (2026-02-22): Created this ExecPlan to convert hotspot findings into targeted benchmark coverage, measurable optimizations, and a before/after performance report as requested by the user.
Change Note (2026-02-22): Updated this ExecPlan to reflect completed implementation, stabilized authoritative benchmark methodology, final before/after outcomes, artifact locations, and flameview self-inspection workflow.
Change Note (2026-02-22): Added deep-dive hotspot profiling outcomes and documented that exploratory mitigations were measured but not retained due inconsistent performance impact.
