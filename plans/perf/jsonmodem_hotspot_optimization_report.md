# JsonModem Hotspot Optimization Report (2026-02-22)

## Scope

This report covers:

- New targeted benchmark coverage for parser/scanner, iterator drop/finalization, and values/buffers assembly paths.
- Baseline vs optimized timing in release bench mode.
- End-to-end timing for `streaming_json_medium` and `streaming_json_large` across all three JsonModem flavors.
- Post-change CPU and allocation-focused hotspot inspection with `perf` + `flameview`.

## Code Changes In Scope

- Added targeted bench binary: `crates/jsonmodem/benches/hotspot_targets.rs`.
- Registered bench target in `crates/jsonmodem/Cargo.toml` (`[[bench]] name = "hotspot_targets"`).
- Parser/scanner/drop path changes:
  - `crates/jsonmodem/src/parser/scanner/mod.rs`
  - `crates/jsonmodem/src/parser/mod.rs`
- Values/container assembly changes:
  - `crates/jsonmodem/src/jsonmodem_values.rs`
  - `crates/jsonmodem/src/backend/std/value_applicator.rs`

## Measurement Method

To reduce prior noise from fast-mode warmups, the authoritative run used:

- Release benches (`cargo bench`, optimized profile).
- `--measurement-time 5 --sample-size 50`.
- A clean baseline worktree at pre-change `HEAD` plus benchmark harness only.

Authoritative artifacts:

- Baseline: `/tmp/jsonmodem_execplan_authoritative_20260222_stable/baseline/`
- Optimized: `/tmp/jsonmodem_execplan_authoritative_20260222_stable/after/`
- Parsed comparison TSV: `/tmp/jsonmodem_execplan_authoritative_20260222_stable/comparison.tsv`

Commands used:

    cargo bench --package jsonmodem --bench hotspot_targets -- --measurement-time 5 --sample-size 50
    cargo bench --package jsonmodem --bench streaming_json_medium -- --measurement-time 5 --sample-size 50
    cargo bench --package jsonmodem --bench streaming_json_large -- --measurement-time 5 --sample-size 50

## Targeted Benchmarks (Hotspots)

Negative delta means faster (improved).

| Benchmark | Baseline | Optimized | Delta |
| --- | ---: | ---: | ---: |
| `hotspot_targets/parser_events_ascii_single_chunk_256k` | 62.280 us | 63.400 us | +1.798% |
| `hotspot_targets/parser_events_ascii_cross_chunk_256k_1024` | 164.51 us | 157.23 us | -4.425% |
| `hotspot_targets/iterator_drop_small_value_cycles/4096` | 465.83 us | 436.43 us | -6.311% |
| `hotspot_targets/buffers_object_assembly_1024x48` | 461.22 us | 453.23 us | -1.732% |
| `hotspot_targets/values_object_assembly_1024x48` | 467.96 us | 464.33 us | -0.776% |

Aggregate targeted delta across all 5 cases: **-2.289%**.

## End-to-End: streaming_json_medium

| Benchmark | Baseline | Optimized | Delta |
| --- | ---: | ---: | ---: |
| `streaming_json_medium/jsonmodem_events/100` | 28.559 us | 29.386 us | +2.896% |
| `streaming_json_medium/jsonmodem_events/1000` | 59.302 us | 59.304 us | +0.003% |
| `streaming_json_medium/jsonmodem_events/5000` | 126.95 us | 120.78 us | -4.860% |
| `streaming_json_medium/jsonmodem_buffers/100` | 39.877 us | 38.088 us | -4.486% |
| `streaming_json_medium/jsonmodem_buffers/1000` | 81.009 us | 77.720 us | -4.060% |
| `streaming_json_medium/jsonmodem_buffers/5000` | 159.16 us | 153.83 us | -3.349% |
| `streaming_json_medium/jsonmodem_values/100` | 40.239 us | 38.596 us | -4.083% |
| `streaming_json_medium/jsonmodem_values/1000` | 84.084 us | 81.200 us | -3.430% |
| `streaming_json_medium/jsonmodem_values/5000` | 171.99 us | 161.87 us | -5.884% |

Aggregate medium delta across all 9 cases: **-3.028%**.

## End-to-End: streaming_json_large

| Benchmark | Baseline | Optimized | Delta |
| --- | ---: | ---: | ---: |
| `streaming_json_large/jsonmodem_events/100` | 54.765 us | 53.377 us | -2.534% |
| `streaming_json_large/jsonmodem_events/1000` | 95.603 us | 97.696 us | +2.189% |
| `streaming_json_large/jsonmodem_events/5000` | 230.26 us | 231.90 us | +0.712% |
| `streaming_json_large/jsonmodem_buffers/100` | 65.708 us | 65.289 us | -0.638% |
| `streaming_json_large/jsonmodem_buffers/1000` | 111.59 us | 107.99 us | -3.226% |
| `streaming_json_large/jsonmodem_buffers/5000` | 259.28 us | 249.76 us | -3.672% |
| `streaming_json_large/jsonmodem_values/100` | 66.800 us | 67.561 us | +1.139% |
| `streaming_json_large/jsonmodem_values/1000` | 115.81 us | 118.52 us | +2.340% |
| `streaming_json_large/jsonmodem_values/5000` | 271.18 us | 275.62 us | +1.637% |

Aggregate large delta across all 9 cases: **-0.228%**.

Flavor-level aggregate deltas on `streaming_json_large`:

- events: **+0.122%** (flat/slight regression)
- buffers: **-2.512%** (improved)
- values: **+1.705%** (slight regression)

## Post-Change Hotspots (CPU + Allocation-Focused)

Profile artifacts:

- `/tmp/jsonmodem_profiles_perf_after_20260222_010934/`

Generated files include per-flavor:

- `*.5000.cpu.folded`
- `*.5000.cpu.flamegraph.svg`
- `*.5000.alloc.focus.folded`
- `*.5000.alloc.focus.flamegraph.svg`
- `*.5000.cpu.summary.txt`
- `*.5000.alloc.summary.txt`

Top inclusive CPU hotspots (post-change):

- Events flavor: parser loop dominates (`JsonModem::next_event_with`, `next_event_step`, `lex`, `lex_state_step`), with `Scanner::consume_string_ascii_fast` still material.
- Buffers flavor: parser loop still dominates plus assembly path (`ValueApplicator::push`, `ValueZipper::with_leaf_mut`).
- Values flavor: `jsonmodem_values::next_value_for_source`/`next_emit_kind` on top of parser and buffers adapter layers.

Top allocation-focused hotspots (post-change):

- `jsonmodem::backend::std::value_applicator::ValueApplicator::apply_string`
- `jsonmodem::backend::std::value_zipper::ValueZipper::with_leaf_mut`
- `jsonmodem::parser::scanner::Scanner::push_ascii_to_scratch`
- drop/finalization paths involving `JsonModemIterator` and `ScannerState`

## Clear Next Optimization Targets

Based on the latest profiles and deltas, the clearest remaining targets are:

1. Parser single-chunk fast path (`Scanner::consume_string_ascii_fast` and nearby lex dispatch), since targeted single-chunk parser benchmark regressed while cross-chunk improved.
2. Values flavor on large payloads (`jsonmodem_values::next_value_for_source` + container/path mutation) where `streaming_json_large/jsonmodem_values/*` is still mildly slower.
3. Allocation pressure in string/container assembly (`ValueApplicator::apply_string`, `ValueZipper::with_leaf_mut`) and scratch growth (`push_ascii_to_scratch`) visible in allocation-focused stacks.

## Deep Dive: Requested Hotspot Paths (2026-02-22)

User-requested focus paths were re-profiled directly with `perf` + collapsed stacks:

- Artifacts: `/tmp/jsonmodem_deep_dive_profile_20260222_093319/`
- Sample collection warning: one perf chunk lost (`Processed 2772 events and lost 1 chunks`), but hotspot ordering remained stable.

Inclusive CPU percentages for requested symbols (at `streaming_json_large/*/5000`):

- `jsonmodem_events`:
  - `Scanner::consume_string_ascii_fast`: `25.34%`
  - `Scanner::push_ascii_to_scratch`: `8.29%`
  - `StdBackend::push_key_from_str`: `0.43%`
  - `JsonModemIterator::drop`: `17.39%`
  - `Scanner::finish`: `4.89%`
- `jsonmodem_buffers`:
  - `Scanner::consume_string_ascii_fast`: `23.52%`
  - `Scanner::push_ascii_to_scratch`: `7.77%`
  - `StdBackend::push_key_from_str`: `0.47%`
  - `JsonModemIterator::drop`: `16.08%`
  - `Scanner::finish`: `3.51%`
- `jsonmodem_values`:
  - `Scanner::consume_string_ascii_fast`: `21.89%`
  - `Scanner::push_ascii_to_scratch`: `7.92%`
  - `StdBackend::push_key_from_str`: `0.76%`
  - `JsonModemIterator::drop`: `13.53%`
  - `Scanner::finish`: `3.77%`

Allocation-focused percentages for the same symbols:

- `jsonmodem_events`: `consume_string_ascii_fast/push_ascii_to_scratch` each `51.44%`, `push_key_from_str` `6.09%`, `drop` `18.18%`, `finish` `9.07%`.
- `jsonmodem_buffers`: `consume_string_ascii_fast/push_ascii_to_scratch` each `30.32%`, `push_key_from_str` `5.44%`, `drop` `14.56%`, `finish` `3.23%`.
- `jsonmodem_values`: `consume_string_ascii_fast/push_ascii_to_scratch` each `14.66%`, `push_key_from_str` `13.17%`, `drop` `16.16%`, `finish` `10.28%`.

Line-level call-tree evidence (`perf report -F+srcline --stdio`) showed allocator time concentrated in:

- `String::reserve`/`RawVec::reserve` under `Scanner::push_ascii_to_scratch`.
- `Vec::reserve`/`RawVec::reserve` in path growth work under `StdBackend::push_key_from_str`.
- `VecDeque` growth work under `Scanner::finish` from iterator drop carryover writes.

Experiment notes during this deep dive:

- Tried key interning in `StdBackend::push_key_from_str`: reduced allocations in principle but regressed object assembly benches in this harness, so not kept.
- Tried broader `finish` no-op fast path and aggressive reserve heuristics: mixed/noisy and not consistently positive, so not kept.

## How To Inspect Flamegraphs Yourself With flameview

From repo root:

    cargo bench --package jsonmodem --bench streaming_json_large --no-run
    BIN=$(find target/release/deps -maxdepth 1 -executable -name 'streaming_json_large-*' | head -n 1)
    OUT=/tmp/jsonmodem_profiles_manual_$(date +%Y%m%d_%H%M%S)
    mkdir -p "$OUT"

Record CPU profile for each flavor (release benchmark binary):

    for flavor in jsonmodem_events jsonmodem_buffers jsonmodem_values; do
      perf record -F 400 --call-graph dwarf,64000 -o "$OUT/${flavor}.5000.cpu.perf.data" -- \
        "$BIN" --bench "${flavor}/5000" --profile-time 5 >/dev/null 2>&1
      perf script -i "$OUT/${flavor}.5000.cpu.perf.data" | inferno-collapse-perf > "$OUT/${flavor}.5000.cpu.folded"
      inferno-flamegraph --title "${flavor}/5000 CPU" "$OUT/${flavor}.5000.cpu.folded" > "$OUT/${flavor}.5000.cpu.flamegraph.svg"
    done

Optional allocation-focused view by slicing allocation-related stacks from CPU data:

    ALLOC_RE='(__rust_alloc|__rdl_alloc|malloc|realloc|alloc::alloc::|RawVec|reserve|with_capacity)'
    for flavor in jsonmodem_events jsonmodem_buffers jsonmodem_values; do
      rg -N "$ALLOC_RE" "$OUT/${flavor}.5000.cpu.folded" > "$OUT/${flavor}.5000.alloc.focus.folded" || true
      if [ -s "$OUT/${flavor}.5000.alloc.focus.folded" ]; then
        inferno-flamegraph --title "${flavor}/5000 Allocation-Focused (CPU)" \
          "$OUT/${flavor}.5000.alloc.focus.folded" > "$OUT/${flavor}.5000.alloc.focus.flamegraph.svg"
      fi
    done

Inspect summaries:

    flameview --summarize --max-lines 60 "$OUT/jsonmodem_events.5000.cpu.folded"
    flameview --summarize --max-lines 60 "$OUT/jsonmodem_buffers.5000.cpu.folded"
    flameview --summarize --max-lines 60 "$OUT/jsonmodem_values.5000.cpu.folded"

Open interactive flameview session:

    flameview "$OUT/jsonmodem_events.5000.cpu.folded"
    flameview "$OUT/jsonmodem_buffers.5000.cpu.folded"
    flameview "$OUT/jsonmodem_values.5000.cpu.folded"

    flameview "$OUT/jsonmodem_events.5000.alloc.focus.folded"
    flameview "$OUT/jsonmodem_buffers.5000.alloc.focus.folded"
    flameview "$OUT/jsonmodem_values.5000.alloc.focus.folded"

## Conformance / Safety Validation

` .agent/check.sh ` run after changes:

- `rustfmt` passed.
- build passed.
- tests passed.
- clippy failed due pre-existing unrelated lint in `crates/jsonmodem/src/parser/tests.rs:1327` (`clippy::manual_flatten`).
