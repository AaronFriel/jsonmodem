# Profile JsonModem Flavors with Flameview (CPU + Allocation-Focused)

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must be kept up to date as work proceeds.

Refer to `PLANS.md` in the repository root for governing standards. This document is authored and maintained in accordance with those requirements.

## Purpose / Big Picture

This plan gives a reproducible workflow to profile all three JsonModem API flavors (`jsonmodem_events`, `jsonmodem_buffers`, `jsonmodem_values`) in release mode and inspect hotspots with flameview. After following it, a contributor can generate per-flavor CPU flamegraphs and allocation-focused flamegraphs, then open folded stacks in flameview to inspect heavy call paths interactively or via summaries.

The user-visible outcome is concrete files you can open immediately: `*.cpu.folded`/`*.cpu.flamegraph.svg` for time-based hotspots and `*.alloc.focus.folded`/`*.alloc.focus.flamegraph.svg` for allocation-path hotspots.

## Progress

- [x] (2026-02-22 00:16Z) Confirmed the three target flavors and benchmark coverage (`jsonmodem_events`, `jsonmodem_buffers`, `jsonmodem_values`) in `crates/jsonmodem/benches/`.
- [x] (2026-02-22 00:18Z) Validated tooling availability (`flameview`, `cargo-flamegraph`, `perf`, `inferno-collapse-perf`, `inferno-flamegraph`).
- [x] (2026-02-22 00:20Z) Validated heavy benchmark target selection and filters; selected `streaming_json_large` with `/5000` filter for all three flavors.
- [x] (2026-02-22 00:21Z) Generated release-mode CPU profiles for all three flavors using direct bench-binary perf recording.
- [x] (2026-02-22 00:23Z) Generated allocation-focused folded stacks and flamegraphs by isolating allocator-including stack lines.
- [x] (2026-02-22 00:24Z) Produced hotspot report and preserved profiling artifacts under `/tmp/jsonmodem_profiles_perf_direct_20260222_002044/`.
- [x] (2026-02-22 00:25Z) Added self-serve flameview inspection instructions in this ExecPlan.

## Surprises & Discoveries

- Observation: Running `cargo flamegraph --bench ...` on this Criterion bench produced stacks dominated by Cargo activity rather than the benchmark process.
  Evidence: flameview summaries showed `cargo::commands::metadata` and related frames at the top after `cargo flamegraph --package jsonmodem --bench streaming_json_large ...`.

- Observation: Direct bench execution (`cargo bench --no-run` then run the produced bench binary with perf) yielded clean JsonModem hotspots.
  Evidence: top exclusive symbols became parser/lexer functions such as `jsonmodem::parser::JsonModem<Ctx>::lex_state_step` and `jsonmodem::parser::scanner::Scanner::finish`.

- Observation: Tracefs permissions in this environment block dynamic probes and trace events needed for direct allocation event profiling.
  Evidence: `perf probe ...` failed with `No permission to write tracefs`; `perf record -e sdt_libc:...` failed with `No permissions to read /sys/kernel/tracing`.

- Observation: Allocation-focused slicing from CPU stacks still provides actionable allocation-path hotspots (especially in buffers/values assembly paths).
  Evidence: allocation-focused inclusive hotspots include `jsonmodem::backend::std::value_applicator::ValueApplicator::apply_string`, `jsonmodem::backend::std::value_zipper::ValueZipper::with_leaf_mut`, and `jsonmodem::jsonmodem_values::next_value_for_source`.

## Decision Log

- Decision: Profile `streaming_json_large` at filter `/5000` for each flavor instead of `single_chunk_json_large`.
  Rationale: it is heavier and reduces harness/noise effects while keeping parity across all three flavors.
  Date/Author: 2026-02-22 / Codex.

- Decision: Use direct perf on the compiled bench binary (`--no-run`) instead of `cargo flamegraph` for these Criterion targets.
  Rationale: this isolates benchmark execution and avoids Cargo process overhead dominating traces.
  Date/Author: 2026-02-22 / Codex.

- Decision: Use allocation-focused stack slicing as the default non-root allocation workflow.
  Rationale: tracefs restrictions prevented allocator uprobe/trace-event recording in this environment; slicing alloc-related frames preserves useful optimization guidance without privileged setup.
  Date/Author: 2026-02-22 / Codex.

## Outcomes & Retrospective

CPU and allocation-focused profiling for all three JsonModem flavors completed successfully in release mode, with artifacts saved and hotspot summaries generated. The workflow is stable and repeatable in an unprivileged shell. The main limitation is that direct allocator event profiling (malloc/free probe events) needs tracefs permissions; this plan includes a privileged optional path for environments where sudo access is available.

The resulting hotspot patterns suggest parser lexing/scanning dominates all flavors, while buffers/values add noticeable allocation-heavy work in value assembly and container handling.

## Context and Orientation

This repository’s core parser crate is `crates/jsonmodem`. Benchmark binaries are Criterion benches in `crates/jsonmodem/benches/`. The three “flavors” are:

- `jsonmodem_events`: core event stream parser path.
- `jsonmodem_buffers`: event stream + buffer assembly path.
- `jsonmodem_values`: value-producing adapter path.

The selected benchmark binary target is `streaming_json_large` (from `crates/jsonmodem/benches/streaming_json_large.rs`) and each flavor is filtered at `/5000`.

Profiling artifacts are written to one output directory (example: `/tmp/jsonmodem_profiles_perf_direct_20260222_002044/`) and include:

- `*.cpu.perf.data`: raw perf capture.
- `*.cpu.folded`: collapsed stacks for flameview.
- `*.cpu.flamegraph.svg`: rendered CPU flamegraph.
- `*.alloc.focus.folded`: allocation-focused collapsed stacks.
- `*.alloc.focus.flamegraph.svg`: rendered allocation-focused flamegraph.
- `hotspots_report.txt`: aggregated hotspots for CPU and allocation-focused views.

## Plan of Work

Build the `streaming_json_large` bench binary in release mode without running it, then run it directly under perf for each flavor filter. Convert perf output to folded stacks with `inferno-collapse-perf`, render SVG flamegraphs with `inferno-flamegraph`, and inspect both summaries and interactive views with flameview.

For allocation-focused analysis in an unprivileged environment, derive a second folded file by keeping only stack lines that include allocator and reserve symbols (`__rust_alloc`, `__rdl_alloc`, `malloc`, `realloc`, `RawVec`, `reserve`, `alloc::alloc::`). This reveals where allocation-related cost concentrates.

If tracefs permissions are available, optionally add direct allocator event profiling using libc probes/trace events.

## Concrete Steps

Run from repository root (`/home/friel/c/aaronfriel/jsonmodem-perf`).

Build benchmark binary in release mode:

    JSONMODEM_BENCH_FAST=1 cargo bench --package jsonmodem --bench streaming_json_large --no-run
    BIN=$(find target/release/deps -maxdepth 1 -executable -name 'streaming_json_large-*' | head -n 1)
    OUT_DIR=/tmp/jsonmodem_profiles_perf_direct_$(date +%Y%m%d_%H%M%S)
    mkdir -p "$OUT_DIR"

Collect CPU profiles for all three flavors:

    for flavor in jsonmodem_events jsonmodem_buffers jsonmodem_values; do
      perf record -F 400 --call-graph dwarf,64000 -o "$OUT_DIR/${flavor}.5000.cpu.perf.data" -- \
        "$BIN" --bench "${flavor}/5000" --profile-time 5 >/dev/null 2>&1
      perf script -i "$OUT_DIR/${flavor}.5000.cpu.perf.data" | inferno-collapse-perf > "$OUT_DIR/${flavor}.5000.cpu.folded"
      inferno-flamegraph --title "${flavor}/5000 CPU" "$OUT_DIR/${flavor}.5000.cpu.folded" > "$OUT_DIR/${flavor}.5000.cpu.flamegraph.svg"
    done

Generate allocation-focused folded stacks and SVGs (non-root workflow):

    ALLOC_RE='(__rust_alloc|__rdl_alloc|malloc|realloc|alloc::alloc::|RawVec|reserve|with_capacity)'
    for flavor in jsonmodem_events jsonmodem_buffers jsonmodem_values; do
      rg -N "$ALLOC_RE" "$OUT_DIR/${flavor}.5000.cpu.folded" > "$OUT_DIR/${flavor}.5000.alloc.focus.folded" || true
      if [ -s "$OUT_DIR/${flavor}.5000.alloc.focus.folded" ]; then
        inferno-flamegraph --title "${flavor}/5000 Allocation-Focused (CPU)" \
          "$OUT_DIR/${flavor}.5000.alloc.focus.folded" > "$OUT_DIR/${flavor}.5000.alloc.focus.flamegraph.svg"
      fi
    done

Preview summaries in terminal:

    flameview --summarize --max-lines 30 "$OUT_DIR/jsonmodem_events.5000.cpu.folded"
    flameview --summarize --max-lines 30 "$OUT_DIR/jsonmodem_buffers.5000.cpu.folded"
    flameview --summarize --max-lines 30 "$OUT_DIR/jsonmodem_values.5000.cpu.folded"

    flameview --summarize --max-lines 30 "$OUT_DIR/jsonmodem_events.5000.alloc.focus.folded"
    flameview --summarize --max-lines 30 "$OUT_DIR/jsonmodem_buffers.5000.alloc.focus.folded"
    flameview --summarize --max-lines 30 "$OUT_DIR/jsonmodem_values.5000.alloc.focus.folded"

Open interactive flameview (your request):

    flameview "$OUT_DIR/jsonmodem_events.5000.cpu.folded"
    flameview "$OUT_DIR/jsonmodem_buffers.5000.cpu.folded"
    flameview "$OUT_DIR/jsonmodem_values.5000.cpu.folded"

    flameview "$OUT_DIR/jsonmodem_events.5000.alloc.focus.folded"
    flameview "$OUT_DIR/jsonmodem_buffers.5000.alloc.focus.folded"
    flameview "$OUT_DIR/jsonmodem_values.5000.alloc.focus.folded"

Optional privileged allocator-event workflow (requires tracefs/sudo):

    # Example only; run if your environment permits tracefs access.
    sudo perf probe -x /lib/x86_64-linux-gnu/libc.so.6 'malloc size=%di'
    sudo perf record -e probe_libc:malloc -g --call-graph dwarf,64000 -o "$OUT_DIR/malloc.perf.data" -- \
      "$BIN" --bench jsonmodem_values/5000 --profile-time 5
    sudo chown "$(id -u):$(id -g)" "$OUT_DIR/malloc.perf.data"
    perf script -i "$OUT_DIR/malloc.perf.data" | inferno-collapse-perf > "$OUT_DIR/malloc.folded"
    inferno-flamegraph --title "malloc call stacks" "$OUT_DIR/malloc.folded" > "$OUT_DIR/malloc.flamegraph.svg"
    flameview "$OUT_DIR/malloc.folded"

## Validation and Acceptance

The workflow is accepted when all six folded files exist (three CPU and three allocation-focused) and flameview can open each one without errors. At minimum, verify:

- `ls -1 "$OUT_DIR" | rg '\.cpu\.folded$'` returns three files.
- `ls -1 "$OUT_DIR" | rg '\.alloc\.focus\.folded$'` returns three files.
- `flameview --summarize "$OUT_DIR/jsonmodem_values.5000.cpu.folded"` prints a symbol summary.
- `flameview --summarize "$OUT_DIR/jsonmodem_values.5000.alloc.focus.folded"` prints an allocation-focused symbol summary.

Behavioral acceptance for hotspot quality:

- CPU profiles show parser scanner/lexer hotspots (for example `JsonModem::lex_state_step`, `Scanner::consume_string_ascii_fast`, `Scanner::finish`).
- Allocation-focused profiles show value/buffer assembly and container paths more prominently in buffers/values flavors.

## Idempotence and Recovery

All commands are idempotent when writing to a fresh `OUT_DIR`. Re-running in the same directory safely overwrites matching files.

If perf capture is interrupted, rerun only the affected flavor command; conversion and rendering are independent per file. If symbol quality degrades, ensure release bench was built with debuginfo (`bench` profile already includes debuginfo in this repository’s current setup) and keep `--call-graph dwarf,64000`.

If interactive flameview is unavailable in your terminal session, use `flameview --summarize` as a non-interactive fallback.

## Artifacts and Notes

Current run artifacts:

    /tmp/jsonmodem_profiles_perf_direct_20260222_002044/
      jsonmodem_events.5000.cpu.folded
      jsonmodem_buffers.5000.cpu.folded
      jsonmodem_values.5000.cpu.folded
      jsonmodem_events.5000.alloc.focus.folded
      jsonmodem_buffers.5000.alloc.focus.folded
      jsonmodem_values.5000.alloc.focus.folded
      hotspots_report.txt

Example hotspot snippets from `hotspots_report.txt`:

    CPU (events): jsonmodem::parser::JsonModem<Ctx>::lex_state_step
    CPU (buffers): jsonmodem::parser::scanner::Scanner::consume_string_ascii_fast
    CPU (values): jsonmodem::jsonmodem_values::next_value_for_source
    Alloc-focus (buffers): jsonmodem::backend::std::value_applicator::ValueApplicator::apply_string
    Alloc-focus (values): jsonmodem::backend::std::value_zipper::ValueZipper::with_leaf_mut

## Interfaces and Dependencies

Key CLI dependencies used by this plan:

- `cargo` (bench build and target discovery).
- `perf` (sampling and call graph capture).
- `inferno-collapse-perf` (collapse perf script output to folded stacks).
- `inferno-flamegraph` (render SVG from folded stacks).
- `flameview` (interactive and summary inspection).
- `rg` (allocation-focused stack filtering).

Important benchmark interface:

- Bench binary target: `streaming_json_large` (package `jsonmodem`).
- Flavor filters: `jsonmodem_events/5000`, `jsonmodem_buffers/5000`, `jsonmodem_values/5000`.
- Profiling mode: Criterion `--profile-time 5` for profiler-friendly steady execution.

Change Note (2026-02-22): Created this ExecPlan and recorded the validated end-to-end workflow plus explicit flameview self-inspection instructions, because the user requested executable profiling instructions after workflow validation.
