# Python Incremental Values API

This ExecPlan follows the repository's `AGENTS.md` guidance for substantial feature work. Keep `Progress`, `Surprises & Discoveries`, and `Validation` current while implementing.

## Purpose

Expose Python APIs for incremental JSON values so callers can feed a stream of JSON fragments and receive useful value updates. The comparison target is other partial JSON parsers that reparse the cumulative prefix after every fragment and return the current best value.

This is not a full-document `loads()` API. The API is for incremental fragment streams.

## Progress

- [x] (2026-06-04) Confirmed Rust already has `JsonModemValues` with `ValuesOptions::with_partial(true)` and `view_root()`.
- [x] (2026-06-04) Confirmed the Python event APIs were initially split across `JsonModem`, `JsonModemByteViews`, and `JsonModemPathFilter`, and did not yet expose `JsonModemValues`.
- [x] (2026-06-04) Exposed Python `JsonModemValues` with `feed(chunk_or_chunks)`, `finish()`, `view()`, and `is_finished`.
- [x] (2026-06-04) Added Python tests covering partial updates, final updates, multiple roots, `view()`, and finish state errors.
- [x] (2026-06-04) Added typing and README guidance for `JsonModemValues`.
- [x] (2026-06-04) Added focused benchmark comparisons against `jiter` cumulative-prefix parsing and existing partial JSON packages.
- [x] (2026-06-04) Added Python tests covering partial updates, final updates, multiple roots, `view()`, and finish state errors.
- [x] (2026-06-04) Added typing and README guidance.
- [x] (2026-06-04) Added focused benchmark comparisons against `jiter` cumulative-prefix parsing and existing partial JSON packages.
- [x] (2026-06-04) Ran `.agent/check-py.sh`: extension build, 26 Python tests, pydoc, and pdoc passed. pdoc emitted the existing native `__hash__` warnings.
- [x] (2026-06-04) Ran fast pyperf smokes:
  - `jsonmodem_values_chunked:medium_response.json` ~= 503 us.
  - `jsonmodem_values_feed_chunks:medium_response.json` ~= 490 us.
  - `jiter_cumulative_partial_prefixes:medium_response.json` ~= 379 us.
  - `jsonmodem_values:llm_partial_content` ~= 230 us.
  - `jsonriver:llm_partial_content` ~= 12.2 ms.
  - `partial_json_parser:llm_partial_content` ~= 515 ms.
  - `json_streamer:llm_partial_content` ~= 77.3 ms.
- [x] (2026-06-04) Added `jsonmodem_values_view_prefixes` after noticing that `jsonmodem_values_chunked` emits only value changes, while cumulative-prefix `jiter` returns a value for every fragment. Manual timing for the stricter "snapshot after every fragment" comparison:
  - `medium_response.json`, 64-byte fragments: `jsonmodem_values_view_prefixes` ~= 599 us; `jiter_cumulative_partial_prefixes` ~= 376 us.
  - `medium_response.json`, 8-byte fragments: `jsonmodem_values_view_prefixes` ~= 3.81 ms; `jiter_cumulative_partial_prefixes` ~= 2.82 ms.
  - `response_large.json`, 64-byte fragments: `jsonmodem_values_view_prefixes` ~= 6.39 ms; `jiter_cumulative_partial_prefixes` ~= 7.35 ms.
- [x] (2026-06-04) Ran `.agent/check.sh`: rustfmt, release build, Rust tests, clippy, public docs, cfg-miri clippy, and actionlint passed. Miri was skipped by the script's default `AGENT_CHECK_MIRI_DISABLE=true`.
- [x] (2026-06-04) Implemented `JsonModemMutableValues`, which mutates one Python root object as events arrive and reports changed paths.
- [x] (2026-06-04) Implemented `JsonModemValueViews` and `JsonModemValueView`, which keep the current value tree in Rust and report changed paths without converting the whole root to Python unless `snapshot()` is called.
- [x] (2026-06-04) Compared mutable-root, read-only-view, snapshot values, and `jiter` cumulative-prefix parsing on the same fragment boundaries.
- [x] (2026-06-04) Ran `.agent/check-py.sh`: extension build, 30 Python tests, pydoc, and pdoc passed. pdoc emitted the existing native `__hash__` warnings.
- [x] (2026-06-04) Ran `.agent/check.sh`: rustfmt, release build, Rust tests, clippy, public docs, cfg-miri clippy, and actionlint passed. Miri was skipped by the script's default `AGENT_CHECK_MIRI_DISABLE=true`.
- [x] (2026-06-04) Ran fast pyperf smokes for the new APIs:
  - `medium_response.json`, 64-byte fragments: snapshot updates ~= 488 us; strict snapshot prefix ~= 614 us; mutable-root prefix ~= 356 us; read-only-view prefix with full snapshot ~= 490 us; read-only changed paths ~= 125 us; `jiter_cumulative_partial_prefixes` ~= 372 us.
  - `medium_response.json`, 8-byte fragments: snapshot updates ~= 488 us; strict snapshot prefix ~= 607 us; mutable-root prefix ~= 359 us; read-only-view prefix with full snapshot ~= 501 us; read-only changed paths ~= 122 us; `jiter_cumulative_partial_prefixes` ~= 371 us.
  - `llm_partial_content`: byte-view pathfilter ~= 268 us; snapshot values ~= 232 us; mutable-root values ~= 214 us; read-only views ~= 216 us; `jsonriver` ~= 12.1 ms; `partial_json_parser` ~= 507 ms; `json_streamer` ~= 74.6 ms.
- [x] (2026-06-04) Audited constant factors after the user noted that `jiter` appeared too close. Found the 8-byte pyperf run was invalid because worker processes reset `--chunk-size 8` to the 64-byte default; fixed `bench_jiter_chunked.py` so workers preserve `JSONMODEM_PY_JITER_CHUNKED_SIZE`.
- [x] (2026-06-04) Reran corrected 8-byte pyperf smoke. `medium_response.json` with 292 fragments:
  - `jsonmodem_values_chunked` ~= 2.22 ms.
  - `jsonmodem_values_feed_chunks` ~= 2.17 ms.
  - `jsonmodem_values_view_prefixes` ~= 3.68 ms.
  - `jsonmodem_mutable_values_prefixes` ~= 2.10 ms.
  - `jsonmodem_mutable_values_feed_chunks` ~= 2.18 ms.
  - `jsonmodem_value_views_prefixes` ~= 3.09 ms.
  - `jsonmodem_value_views_feed_chunks` ~= 3.34 ms.
  - `jsonmodem_value_views_changed_paths` ~= 307 us.
  - `jsonmodem_value_views_changed_paths_feed_chunks` ~= 202 us.
  - `jiter_cumulative_partial_prefixes` ~= 2.73 ms.
- [x] (2026-06-04) Measured a local timing breakdown for `medium_response.json` with 292 fragments:
  - Core `JsonModem` event parsing and consumption ~= 234 us.
  - `JsonModemValueViews` changed-path repeated feeds ~= 312 us.
  - `JsonModemValueViews` changed-path one-call `feed(chunks)` ~= 170-202 us across local and pyperf runs.
  - `jiter` cumulative prefixes without `repr` ~= 1.04 ms; with `repr` ~= 2.76 ms.
  - Conclusion: repeated Python calls and full Python value observation dominate small-fragment timings; the parser itself is already well below corrected `jiter` cumulative parsing.
- [x] (2026-06-04) Tried returning Python's empty iterator for value-style empty feeds to avoid allocating `PyValueIter`; local measurement did not improve and slightly worsened the tiny-fragment path, so the change was reverted.
- [x] (2026-06-04) Reduced one avoidable view overhead by returning a static string from `JsonModemValueView.kind` instead of allocating a Rust `String`.
- [x] (2026-06-04) Ran scaling measurements for a synthetic `{"content": "x" * N}` document with 8-byte fragments to test the asymptotic claim:
  - 2KB payload, 258 fragments: read-only changed paths with one `feed(chunks)` ~= 176 us; `jiter` cumulative prefixes ~= 149 us. At this size, native constant factors dominate.
  - 8KB payload, 1,026 fragments: read-only changed paths with one `feed(chunks)` ~= 576 us; `jiter` cumulative prefixes ~= 1.52 ms.
  - 32KB payload, 4,098 fragments: read-only changed paths with one `feed(chunks)` ~= 2.77 ms; `jiter` cumulative prefixes ~= 20.5 ms.
  - 64KB payload, 8,194 fragments: read-only changed paths with one `feed(chunks)` ~= 5.62 ms; `jiter` cumulative prefixes ~= 80.9 ms.
  - 128KB payload, 16,386 fragments: read-only changed paths with one `feed(chunks)` ~= 14.0 ms; `jiter` cumulative prefixes ~= 315.6 ms.
- [x] (2026-06-04) Confirmed `jiter.from_json(..., partial_mode=True)` returns `{}` for prefixes inside an open string, for example `b'{"content":"xxxx'`. Therefore `jiter` is not producing partial streaming string content for the LLM/tool-call case; it scans prefixes quickly but does less useful output work than jsonmodem byte views or value views.
- [x] (2026-06-04) Identified why some jsonmodem value modes do not show the asymptotic win:
  - `JsonModemValues` is quadratic for growing values because it clones/converts snapshots repeatedly.
  - `JsonModemMutableValues` is quadratic for growing strings because Python strings are immutable and the leaf string is replaced with old-plus-fragment.
  - `JsonModemValueViews` changed-path mode and path-filtered byte views preserve the linear parser behavior because callers can consume fragments/paths without requiring a full Python value after every prefix.
- [x] (2026-06-04) Measured an array-of-N-strings workload, `["abcd", ...]`, with 8-byte fragments:
  - N=256, 225 fragments: events ~= 0.25 ms; changed-path views with one `feed(chunks)` ~= 0.23 ms; mutable updates ~= 0.16 ms; `JsonModemValues` ~= 2.0 ms; `JsonModemValues.view()` after every prefix ~= 2.9 ms; `jiter` cumulative prefixes ~= 0.89 ms.
  - N=1,024, 897 fragments: events ~= 1.0 ms; changed-path views ~= 0.96 ms; mutable updates ~= 0.66 ms; `JsonModemValues` ~= 49 ms; `JsonModemValues.view()` after every prefix ~= 41 ms; `jiter` cumulative prefixes ~= 12.6 ms.
  - N=4,096, 3,585 fragments: events ~= 4.0 ms; changed-path views ~= 4.3 ms; mutable updates ~= 3.2 ms; `JsonModemValues` ~= 723 ms; `JsonModemValues.view()` after every prefix ~= 615 ms; `jiter` cumulative prefixes ~= 189 ms.
  - N=8,192, 7,169 fragments: events ~= 7.7 ms; changed-path views ~= 8.0 ms; mutable updates ~= 6.2 ms; `JsonModemValues` ~= 2.8 s; `JsonModemValues.view()` after every prefix ~= 2.5 s; `jiter` cumulative prefixes ~= 783 ms.
  - N=16,384, 14,337 fragments: events ~= 17 ms; changed-path views ~= 18 ms; mutable updates ~= 13 ms; `JsonModemValues` ~= 12.2 s; `JsonModemValues.view()` after every prefix ~= 10.2 s; `jiter` cumulative prefixes ~= 3.0 s.
- [x] (2026-06-04) Conclusion from the array workload: jsonmodem's parser and changed-path APIs are more than 100x faster than `jiter` cumulative prefixes at larger N. `JsonModemValues` is slower than `jiter` because it eagerly builds repeated Python snapshots of the growing array, which is not the performance API we should optimize for this goal.
- [x] (2026-06-04) Built three experimental read-only value-view variants:
  - `JsonModemValueViewsCached`: same `(index, view, path, is_final)` shape as `JsonModemValueViews`, but returns one cached root view object instead of allocating a new root view per update.
  - `JsonModemValuePaths`: returns `(index, path, is_final)` updates while keeping `view()` available separately. This isolates the cost of returning a root view object in every update.
  - `JsonModemValueViewsPathView`: returns `(index, view, path_view, is_final)` with one cached root view and a `PathView` object instead of a tuple path. This tests tuple path allocation vs view path allocation.
- [x] (2026-06-04) Benchmarked the experimental variants with 8-byte fragments:
  - `medium_response.json`, 292 fragments: baseline changed-path `JsonModemValueViews.feed(chunks)` ~= 232 us; cached root view ~= 211 us; path-only ~= 184 us; cached root plus `PathView` path ~= 175 us; `jiter` cumulative prefixes ~= 3.07 ms.
  - `array_strings_1024`, 897 fragments: baseline changed-path views ~= 1.04 ms; cached root view ~= 0.89 ms; path-only ~= 0.72 ms; cached root plus `PathView` path ~= 0.88 ms; `jiter` cumulative prefixes ~= 13.4 ms.
  - `array_strings_4096`, 3,585 fragments: baseline changed-path views ~= 4.71 ms; cached root view ~= 4.15 ms; path-only ~= 3.52 ms; cached root plus `PathView` path ~= 3.77 ms; `jiter` cumulative prefixes ~= 203 ms.
  - `array_strings_16384`, 14,337 fragments: baseline changed-path views ~= 21.8 ms; cached root view ~= 18.4 ms; path-only ~= 15.7 ms; cached root plus `PathView` path ~= 15.4 ms; `jiter` cumulative prefixes ~= 3.27 s.
- [x] (2026-06-04) Added array string workloads and variant benchmark modes to `crates/jsonmodem-py/benchmarks/bench_jiter_chunked.py`.
- [x] (2026-06-04) Simplified the public Python API after the user requested it:
  - `JsonModem` remains the event stream API.
  - `JsonModemValues` is now the reused read-only root view API, using the cached root view plus `PathView` changed paths.
  - The old snapshot adapter and experimental variant names are no longer registered in the native module, exported from the top-level package, or listed in public stubs/docs.
- [x] (2026-06-04) Answered the read-only typing question in docs: Python has no general `ReadOnly[T]` for arbitrary objects; `typing.ReadOnly` is for `TypedDict` fields, `Final` prevents rebinding rather than mutation, and `Mapping`/`Sequence` or protocol classes are the type-checker-friendly read-only interface pattern.
- [x] (2026-06-04) Reran fast smokes after simplifying `JsonModemValues`:
  - `medium_response.json`, 8-byte fragments: `jsonmodem_values_chunked` ~= 276 us; `jsonmodem_values_feed_chunks` ~= 168 us; `jsonmodem_values_view_prefixes` full-materialization control ~= 3.48 ms; `jiter_cumulative_partial_prefixes` ~= 2.92 ms.
  - `llm_partial_content`: `jsonmodem_pathfilter_byteviews` ~= 284 us; `jsonmodem_values` ~= 220 us; `jsonriver` ~= 12.6 ms; `partial_json_parser` ~= 528 ms; `json_streamer` ~= 78.5 ms.
- [x] (2026-06-04) Consolidated event APIs:
  - `JsonModem(paths=...)` replaces the standalone path-filter parser.
  - `JsonModem(byte_views=True)` replaces the standalone byte-view parser.
  - `JsonModem(paths=..., byte_views=True)` handles filtered byte-view streams.
  - Standalone `JsonModemByteViews` and `JsonModemPathFilter` are no longer registered in the native module, exported by `jsonmodem.__init__`, or listed in public stubs/docs.
  - `JsonModem(byte_views=True).feed(...)` now accepts either one immutable byte buffer or an iterable of immutable byte buffers.
- [x] (2026-06-04) Added overload-oriented stubs for `JsonModem` so `byte_views=True` directs type checkers toward byte-view events while the default directs them toward decoded events.

## API Shape

The public Python API should remain simple:

- `JsonModem` is the event stream API. `paths=` filters decoded events. `byte_views=True` returns byte-view payloads. `paths=` and `byte_views=True` can be combined.
- `JsonModemValues` is the reused read-only value view API.

`JsonModem(options=None, *, paths=None, byte_views=False)` owns the streaming event parser. Without `byte_views`, `feed()` accepts one `str`, `bytes`, `bytearray`, or contiguous `memoryview`, or an iterable of those fragments, and returns `(kind, PathView, payload)` events. With `paths`, only matching event paths are emitted. With `byte_views=True`, `feed()` accepts immutable `bytes` or read-only contiguous `memoryview` fragments, or an iterable of those fragments, and returns byte-view string payloads where possible.

`JsonModemValues(options=None)` keeps the current value tree in Rust and returns updates for the changed path. `feed()` accepts the same decoded-input types as `JsonModem.feed()`: one `str`, `bytes`, `bytearray`, or contiguous `memoryview`, or an iterable of those fragments.

Each yielded item is a normal Python tuple:

```python
(index, view, path, is_final)
```

`index` is the root value number. `view` is the same reused `JsonModemValueView` root object across updates. `path` is a `PathView` for the value that changed. `is_final` marks whether that root is complete. `JsonModemValues.view()` returns the current root view.

`JsonModemValueView` is read-only by API surface: it has `kind`, `path`, `snapshot()`, `__getitem__`, and `__len__`, but no mutator methods. `view["field"]` or `view[index]` returns another read-only view. `view.snapshot()` converts the selected value to normal Python objects only when requested.

The old snapshot adapter and experimental value-view variants are retained in the Rust source for implementation history but are not registered in the native module, exported from `jsonmodem.__init__`, or listed in public stubs.

## Measurement

Current benchmark modes compare:

- `jsonmodem_values_chunked`: feed each fragment and consume `JsonModemValues` changed-path view updates.
- `jsonmodem_values_feed_chunks`: pass the fragment iterable to one `feed()` call and consume changed-path view updates.
- `jsonmodem_values_view_prefixes`: feed each fragment and call `view().snapshot()` after every fragment as a full-materialization control.
- `jiter_cumulative_partial_prefixes`: parse every cumulative prefix with `partial_mode=True`.
- `jsonmodem_pathfilter_byteviews`: consume matching string fragments as byte views through `JsonModem(paths="content", byte_views=True)` in the realistic LLM/tool-call scenario.
- `jsonmodem_values`: consume `JsonModemValues` changed-path view updates in the realistic LLM/tool-call scenario.
- LLM partial parser references from the realistic benchmark suite when installed.

The headline comparison must use the same fragment boundaries for every parser.

## Design Notes

The mutable-root adapter was useful as an experiment but should not be part of
the Python API. It keeps object and array roots stable, but Python strings are
immutable, so growing string leaves require replacement and can become
quadratic.

The read-only view API is optimized for callers that do not need a complete
Python object after every fragment. It should return changed paths and a root
view object that can inspect only the requested parts. This avoids recursively
building Python `dict`/`list` snapshots on each turn.

## Validation

Record exact commands and outcomes here as they are run.

- `.agent/check-py.sh`: passed on 2026-06-04 with 26 Python tests.
- `.agent/check-py.sh`: passed on 2026-06-04 with 30 Python tests after adding mutable-root and read-only-view APIs.
- `.venv/bin/python crates/jsonmodem-py/benchmarks/bench_jiter_chunked.py --workload medium_response.json --group partial_values --chunk-size 64 --fast --output target/python-perf/partial-values-medium-smoke.json`: passed with expected fast-mode pyperf warnings.
- `.venv/bin/python crates/jsonmodem-py/benchmarks/bench_realistic_scenarios.py --scenario llm_tool_arguments --group llm_partial --fast --output target/python-perf/realistic-llm-partial-values-smoke.json`: passed with expected fast-mode pyperf warnings.
- `.venv/bin/python -m pyperf check target/python-perf/partial-values-medium-smoke.json`: passed with expected fast-mode warnings.
- `.venv/bin/python -m pyperf check target/python-perf/realistic-llm-partial-values-smoke.json`: passed with expected fast-mode warnings.
- `PATH="$HOME/.local/bin:$PATH" .agent/check.sh`: passed on 2026-06-04. Miri was skipped by default.
- `.venv/bin/python crates/jsonmodem-py/benchmarks/bench_jiter_chunked.py --workload medium_response.json --group partial_values --chunk-size 64 --list`: includes `jsonmodem_values_view_prefixes`.
- `.venv/bin/python crates/jsonmodem-py/benchmarks/bench_jiter_chunked.py --workload medium_response.json --group partial_values --chunk-size 64 --list`: includes `jsonmodem_mutable_values_prefixes`, `jsonmodem_value_views_prefixes`, and `jsonmodem_value_views_changed_paths`.
- `.venv/bin/python crates/jsonmodem-py/benchmarks/bench_realistic_scenarios.py --scenario llm_tool_arguments --group llm_partial --list`: includes `jsonmodem_mutable_values:llm_partial_content` and `jsonmodem_value_views:llm_partial_content`.
- `.venv/bin/python crates/jsonmodem-py/benchmarks/bench_jiter_chunked.py --workload medium_response.json --group partial_values --chunk-size 64 --fast --output target/python-perf/partial-values-medium-newapis-64-smoke.json`: passed with expected fast-mode pyperf warnings.
- `.venv/bin/python crates/jsonmodem-py/benchmarks/bench_jiter_chunked.py --workload medium_response.json --group partial_values --chunk-size 8 --fast --output target/python-perf/partial-values-medium-newapis-8-smoke.json`: passed with expected fast-mode pyperf warnings.
- `.venv/bin/python crates/jsonmodem-py/benchmarks/bench_realistic_scenarios.py --scenario llm_tool_arguments --group llm_partial --fast --output target/python-perf/realistic-llm-partial-newapis-smoke.json`: passed with expected fast-mode pyperf warnings.
- `.venv/bin/python -m pyperf check target/python-perf/partial-values-medium-newapis-64-smoke.json`: passed with expected fast-mode warnings.
- `.venv/bin/python -m pyperf check target/python-perf/partial-values-medium-newapis-8-smoke.json`: passed with expected fast-mode warnings.
- `.venv/bin/python -m pyperf check target/python-perf/realistic-llm-partial-newapis-smoke.json`: passed with expected fast-mode warnings.
- `PATH="$HOME/.local/bin:$PATH" .agent/check.sh`: passed on 2026-06-04 after adding mutable-root and read-only-view APIs. Miri was skipped by default.
- `.venv/bin/python crates/jsonmodem-py/benchmarks/bench_jiter_chunked.py --workload medium_response.json --group partial_values --chunk-size 8 --fast --output target/python-perf/partial-values-medium-newapis-8-fixed-smoke.json`: passed with expected fast-mode pyperf warnings; metadata confirms `chunk_size_bytes: 8`.
- `.venv/bin/python -m pyperf check target/python-perf/partial-values-medium-newapis-8-fixed-smoke.json`: passed with expected fast-mode warnings.
- `.agent/check-py.sh`: passed on 2026-06-04 with 30 Python tests after fixing benchmark chunk-size inheritance and returning a static `JsonModemValueView.kind`.
- `PATH="$HOME/.local/bin:$PATH" .agent/check.sh`: passed on 2026-06-04 after fixing benchmark chunk-size inheritance and returning a static `JsonModemValueView.kind`. Miri was skipped by default.
- `.agent/check-py.sh`: passed on 2026-06-04 with 33 Python tests after adding `JsonModemValueViewsCached`, `JsonModemValuePaths`, and `JsonModemValueViewsPathView`.
- `.venv/bin/python crates/jsonmodem-py/benchmarks/bench_jiter_chunked.py --workload medium_response.json --group partial_values --chunk-size 8 --fast --output target/python-perf/value-view-variants-medium-8-smoke.json`: passed with expected fast-mode pyperf warnings.
- `.venv/bin/python -m pyperf check target/python-perf/value-view-variants-medium-8-smoke.json`: passed with expected fast-mode warnings; metadata confirms `chunk_size_bytes: 8`.
- `PATH="$HOME/.local/bin:$PATH" .agent/check.sh`: passed on 2026-06-04 after adding the experimental value-view variants and benchmark modes. Miri was skipped by default.
- `.agent/check-py.sh`: passed on 2026-06-04 with 25 Python tests after simplifying public `JsonModemValues`.
- `.venv/bin/python crates/jsonmodem-py/benchmarks/bench_jiter_chunked.py --workload medium_response.json --group partial_values --chunk-size 8 --fast --output target/python-perf/simple-values-medium-8-smoke.json`: passed with expected fast-mode pyperf warnings.
- `.venv/bin/python -m pyperf check target/python-perf/simple-values-medium-8-smoke.json`: passed with expected fast-mode warnings; metadata confirms `chunk_size_bytes: 8`.
- `.venv/bin/python crates/jsonmodem-py/benchmarks/bench_realistic_scenarios.py --scenario llm_tool_arguments --group llm_partial --fast --output target/python-perf/simple-values-llm-partial-smoke.json`: passed with expected fast-mode pyperf warnings.
- `.venv/bin/python -m pyperf check target/python-perf/simple-values-llm-partial-smoke.json`: passed with expected fast-mode warnings.
- `PATH="$HOME/.local/bin:$PATH" .agent/check.sh`: passed on 2026-06-04 after simplifying public `JsonModemValues`. Miri was skipped by the script's default `AGENT_CHECK_MIRI_DISABLE=true`.
- `.agent/check-py.sh`: passed on 2026-06-04 with 25 Python tests after narrowing native module registration to the simplified API.
- `.venv/bin/python - <<'PY' ...`: confirmed `JsonModemValueViewsCached`, `JsonModemValueViews`, `JsonModemValuePaths`, `JsonModemMutableValues`, and `JsonModemValueSnapshots` are absent from both `jsonmodem` and `jsonmodem._jsonmodem`.
- `PATH="$HOME/.local/bin:$PATH" .agent/check.sh`: passed on 2026-06-04 after narrowing native module registration. Miri was skipped by the script's default `AGENT_CHECK_MIRI_DISABLE=true`.
- `.agent/check-py.sh`: passed on 2026-06-04 with 26 Python tests after consolidating `JsonModem(paths=...)` and `JsonModem(byte_views=True)`.
- `.venv/bin/python - <<'PY' ...`: confirmed `JsonModemByteViews` and `JsonModemPathFilter` are absent from both `jsonmodem` and `jsonmodem._jsonmodem`, and `JsonModem(paths="content", byte_views=True).feed([...])` returns byte-view events.
- `.venv/bin/python crates/jsonmodem-py/benchmarks/bench_realistic_scenarios.py --scenario llm_tool_arguments --group llm_partial --fast --output target/python-perf/unified-jsonmodem-llm-partial-smoke.json`: passed with expected fast-mode pyperf warnings.
- `.venv/bin/python -m pyperf check target/python-perf/unified-jsonmodem-llm-partial-smoke.json`: passed with expected fast-mode warnings.
- `PATH="$HOME/.local/bin:$PATH" .agent/check.sh`: passed on 2026-06-04 after consolidating `JsonModem(paths=...)` and `JsonModem(byte_views=True)`. Miri was skipped by the script's default `AGENT_CHECK_MIRI_DISABLE=true`.
