# Python Incremental Parsing Performance and Profiling

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must be kept up to date as work proceeds.

This plan follows `PLANS.md` at the repository root. It also builds on `plans/py/execplan.md`, which introduced the current PyO3 event-stream binding, and `plans/perf/jsonmodem_jiter_execplan.md`, which records Rust-side Jiter comparison work.

## Purpose / Big Picture

After this work, Python users should be able to compare `jsonmodem` fairly against current high-performance Python JSON libraries for the incremental parsing API. The target is a 10x speedup over the current Python event binding on realistic streams of JSON document fragments, especially tiny fragments from HTTP bodies and LLM tool-call chunks. The optimization target is `JsonModem.feed()`, `JsonModemByteViews`, `JsonModemPathFilter`, and related incremental APIs, not `jsonmodem.loads()`.

The current public design has one main feed API. `JsonModem.feed()` accepts either one chunk or an iterable of chunks. Earlier `feed_many()`, pooled `JsonEvent`, `feed_objects()`, `feed_many_objects()`, `finish_objects()`, and `warm_event_pool()` experiments are historical and intentionally removed from the public Python API because performance should be the default, not a separate opt-in mode.

The experimental `jsonmodem.loads()`, `jsonmodem.string_ranges()`, and `jsonmodem.string_range_table()` helpers are also historical and removed from the public Python API. The Rust core never depended on them; they existed only in the Python extension for measurement.

Every headline comparison must process the same incoming fragment boundaries. For `jiter`, the fair document comparison is reparsing every cumulative prefix with `partial_mode=True`, because that asks both libraries to report partial progress after every fragment. For libraries without a true incremental or partial API, results must be labeled as full-decode or reference-only and kept out of the optimization target.

The direct incremental competitor is Pydantic's `jiter` Python interface in cumulative-prefix partial mode. The streaming comparisons include `ijson`, `json-stream`, `jsonriver`, `partial-json-parser`, and `json-streamer` where their APIs match the use case. Full native decoders such as the Python standard library `json`, `orjson`, `msgspec`, `python-rapidjson`, `ujson`, and `pysimdjson` are useful reference results for end-to-end application tradeoffs, but they are not targets for this Python incremental optimization plan.

LLM partial-parser comparisons include `jsonriver`, `partial-json-parser`, and `json-streamer`. `streaming-json-parser` is recorded in the research note but skipped in executable benchmarks because version `0.1.0` fails to import its documented parser module in this environment.

## Progress

- [x] (2026-06-03T01:41:11Z) Created worktree `/home/friel/c/aaronfriel/jsonmodem-python-perf` on branch `codex/python-performance-profiling` from `origin/py`.
- [x] (2026-06-03T01:41:11Z) Confirmed no pre-existing local Python performance/profiling worktree exists; only remote branches and existing plans cover Python bindings and Rust-side performance.
- [x] (2026-06-03T01:41:11Z) Drafted this ExecPlan with benchmark methodology, competitor list, optimization hypothesis, and acceptance criteria.
- [x] (2026-06-03T02:20:00Z) User clarified that the actual goal is to complete methodology, direct comparison, and optimization work, not only create the worktree and plan. Updated active work to proceed through all three outcomes.
- [x] (2026-06-03T02:31:00Z) Built and validated the existing Python binding with `.agent/setup-py.sh` and `.agent/check-py.sh`; initial check passed with 7 existing tests.
- [x] (2026-06-03T02:43:00Z) Imported focused benchmark fixtures from `origin/perf` under `crates/jsonmodem/benches/jiter_data/` and added `crates/jsonmodem-py/benchmarks/bench_json_libraries.py`.
- [x] (2026-06-03T02:52:00Z) Installed benchmark dependencies in the local venv: `pyperf 2.10.0`, `orjson 3.11.9`, `msgspec 0.21.1`, and `jiter 0.15.0`.
- [x] (2026-06-03T03:02:00Z) Added `jsonmodem.loads(data)` and correctness tests comparing nested native output against `json.loads`; `.agent/check-py.sh` passed with 10 tests.
- [x] (2026-06-03T03:16:00Z) Replaced the first `loads()` implementation, which converted through `JsonModemValues`, with a direct Python object builder driven by parser events.
- [x] (2026-06-03T03:28:00Z) Optimized the direct builder to avoid allocating full owned paths for normal events; it now reads only the current object key and reserves full path allocation for fragmented strings.
- [x] (2026-06-03T03:34:00Z) Ran direct comparison benchmarks on `medium_response.json` and `response_large.json`; recorded fast-mode pyperf results below.
- [x] (2026-06-03T03:48:00Z) Tested and rejected using `RawContext` for Python native `loads()`; it regressed `medium_response.json` from roughly `48.3 us` to roughly `56.3 us`, so the code was reverted to the standard direct builder.
- [x] (2026-06-03T03:58:00Z) Switched native `loads()` from `.to_iter()` to the parser lending iterator so parser event paths are borrowed during native construction; `medium_response.json` improved to roughly `45.4 us`.
- [x] (2026-06-03T04:15:00Z) Tested and rejected a per-load Python string key cache; `medium_response.json` regressed to roughly `51.0 us`, `response_large.json` regressed to roughly `135 us`, and `string_array_unique.json` remained slower than competitors.
- [x] (2026-06-03T04:30:00Z) Added `--alloc-summary` to the benchmark harness and recorded `tracemalloc` peaks for `medium_response.json`, `response_large.json`, and `string_array_unique.json`.
- [x] (2026-06-03T04:45:00Z) Added byte-oriented `jsonmodem.string_ranges(data)` and `jsonmodem.string_range_table(data)` APIs plus tests for byte offsets and escaped-string fallback.
- [x] (2026-06-03T05:05:00Z) Tested a hidden parser backend that tracks only container kind; the parser-backed table still measured only about `8.8x` faster than event tuples on `string_array_unique.json`, so the backend was removed from the final diff.
- [x] (2026-06-03T05:20:00Z) Replaced the packed table path with a direct validating byte scanner for JSON string-value offsets; `string_array_unique.json` measured `jsonmodem_string_range_table ~= 183 us` versus `jsonmodem_events ~= 9.01 ms`, about `49x` faster.
- [x] (2026-06-03T05:25:00Z) Ran `.agent/check-py.sh`; build, tests, and docs passed with 14 Python tests after the byte-range APIs were added.
- [x] (2026-06-03T05:45:00Z) Installed `actionlint 1.7.12` under `~/.local/bin` because `go` was unavailable, then ran `PATH="$HOME/.local/bin:$PATH" .agent/check.sh`; rustfmt, release build, Rust tests, clippy, public docs, cfg-miri clippy, and actionlint passed. Miri was skipped by the script's default `AGENT_CHECK_MIRI_DISABLE=true`.
- [x] (2026-06-03T06:25:00Z) Updated `JsonModem.feed()` to accept `str`, `bytes`, `bytearray`, and contiguous `memoryview` chunks, borrowing bytes-like inputs through Python's buffer protocol during each call.
- [x] (2026-06-03T06:30:00Z) Added package typing for the streaming API in `crates/jsonmodem-py/python/jsonmodem/__init__.pyi`, including `JSONInput`, event tuple shapes, and `JsonModem.feed(chunk: JSONInput)`.
- [x] (2026-06-03T06:32:00Z) Added a FastAPI/Starlette-style request streaming example to `crates/jsonmodem-py/README.md`.
- [x] (2026-06-03T06:38:00Z) Ran `.agent/check-py.sh`; build and 16 Python tests passed. `pdoc` completed with a warning about `DecodeMode.__hash__` in the native-class type stub, but the script exited successfully.
- [x] (2026-06-03T07:05:00Z) Investigated no-copy substring options in CPython and the current parser. Conclusion: Python `str` and `bytes` substrings copy in the normal partial-range case; no-copy payload return must use retained byte owners plus `memoryview` ranges, offset tables, or a custom lazy text-view object.
- [x] (2026-06-03T07:30:00Z) Added `JsonModemByteViews`, a Python streaming parser that accepts `bytes` and read-only contiguous `memoryview` input, returns `memoryview` fragments for borrowed JSON string bytes, and marks transformed fragments with `is_view: False`.
- [x] (2026-06-03T07:36:00Z) Ran `.agent/check-py.sh`; build and 20 Python tests passed. `pdoc` still emitted its existing native `DecodeMode.__hash__` warning and exited successfully.
- [x] (2026-06-03T08:05:00Z) Created `plans/python-performance/streaming-json-research.md` with source-backed competitor and downstream usage notes for HTTP streaming, nested extraction, LLM/tool-call chunks, NDJSON, and partial JSON packages.
- [x] (2026-06-03T08:05:00Z) Added `crates/jsonmodem-py/benchmarks/bench_realistic_scenarios.py`, a pyperf harness for realistic generated scenarios: HTTP nested extraction, LLM tool-call content forwarding, NDJSON warning counting, and deep nested target extraction.
- [x] (2026-06-03T08:18:00Z) Ran `python crates/jsonmodem-py/benchmarks/bench_realistic_scenarios.py --list`; the harness listed the expected realistic benchmark names.
- [x] (2026-06-03T08:30:00Z) Fixed filtered pyperf runs by adding automatic `--inherit-environ JSONMODEM_PY_REALISTIC_SCENARIOS,JSONMODEM_PY_REALISTIC_GROUPS` when `--scenario` or `--group` is used.
- [x] (2026-06-03T08:40:00Z) Ran a fast LLM forwarding smoke benchmark: `python crates/jsonmodem-py/benchmarks/bench_realistic_scenarios.py --scenario llm_tool_arguments --group llm_forward --fast --output target/python-perf/realistic-llm-forward-smoke.json`, then `python -m pyperf check target/python-perf/realistic-llm-forward-smoke.json`.
- [x] (2026-06-03T08:50:00Z) Installed added benchmark dependencies with `uv pip install -r crates/jsonmodem-py/benchmarks/requirements-bench.txt`; this added `ijson 3.5.0`, `json-stream 2.5.1`, and `json-stream-rs-tokenizer 0.5.1`.
- [x] (2026-06-03T09:00:00Z) Ran a fast HTTP nested-extraction smoke benchmark: `python crates/jsonmodem-py/benchmarks/bench_realistic_scenarios.py --scenario http_nested_response --group http_extract --fast --output target/python-perf/realistic-http-extract-smoke.json`, then `python -m pyperf check target/python-perf/realistic-http-extract-smoke.json`.
- [x] (2026-06-03T09:20:00Z) Added `JsonModemPathFilter(paths, *, options=None, byte_views=False)` to filter matching parser events before Python event construction; it supports dotted paths, `*` wildcard components, multiple path patterns, and byte-view payloads.
- [x] (2026-06-03T09:27:00Z) Ran `.agent/check-py.sh`; build and 24 Python tests passed. `pdoc` still emitted the existing native `DecodeMode.__hash__` warning and exited successfully.
- [x] (2026-06-03T09:38:00Z) Ran fast path-filter smoke benchmarks for LLM content forwarding and HTTP nested extraction, then ran `python -m pyperf check` on both output files.
- [x] (2026-06-03T09:55:00Z) Installed optional native-decode competitors with `uv pip install -r crates/jsonmodem-py/benchmarks/requirements-bench.txt`; this added `pysimdjson 7.0.2`, `python-rapidjson 1.23`, and `ujson 5.12.1`.
- [x] (2026-06-03T10:05:00Z) Added optional `python_rapidjson`, `pysimdjson`, and `ujson` native-decode discovery to both Python benchmark harnesses.
- [x] (2026-06-03T10:15:00Z) Added `json_stream:http_nested_extract` and `jsonmodem_pathfilter:deep_nested_target` to `bench_realistic_scenarios.py`.
- [x] (2026-06-03T10:25:00Z) Ran `.agent/check-py.sh`; build and 24 Python tests passed with the same non-fatal `DecodeMode.__hash__` pdoc warning.
- [x] (2026-06-03T10:35:00Z) Ran fast pyperf smokes for `deep_nested` and expanded HTTP extraction, then ran `python -m pyperf check` on both output files.
- [x] (2026-06-03T10:55:00Z) Optimized `JsonModemPathFilter` to match borrowed parser paths through the lending iterator before materializing owned Python-facing records.
- [x] (2026-06-03T11:05:00Z) Ran `.agent/check-py.sh`; build and 24 Python tests passed with the same non-fatal pdoc warning.
- [x] (2026-06-03T11:15:00Z) Ran a fast expanded HTTP extraction smoke after the path-filter optimization and checked the pyperf result.
- [x] (2026-06-03T11:35:00Z) Added a source-backed downstream section to `plans/python-performance/streaming-json-research.md` with concrete GitHub URLs for `ijson`, `json-stream`, `jiter`/Pydantic, `orjson`, `msgspec`, and LLM partial/tool-call usage.
- [x] (2026-06-03T11:45:00Z) Installed and inspected LLM partial-parser packages: `jsonriver 1.0.0`, `partial-json-parser 0.2.1.1.post7`, `streaming-json-parser 0.1.0`, and `json-streamer 0.1.0`.
- [x] (2026-06-03T12:00:00Z) Added `llm_partial` and `har_extract` groups to `bench_realistic_scenarios.py`.
- [x] (2026-06-03T12:15:00Z) Ran fast pyperf smokes for `llm_partial` and `har_extract`, then ran `python -m pyperf check` on both result files.
- [x] (2026-06-03T12:30:00Z) Ran `.agent/check-py.sh`; build and 24 Python tests passed with the same non-fatal pdoc warning.
- [x] (2026-06-03T12:35:00Z) Ran `PATH="$HOME/.local/bin:$PATH" .agent/check.sh`; rustfmt, release build, Rust tests, clippy, public docs, cfg-miri clippy, and actionlint passed. Miri was skipped by the script's default `AGENT_CHECK_MIRI_DISABLE=true`.
- [x] (2026-06-03T13:05:00Z) Added `crates/jsonmodem-py/benchmarks/bench_jiter_chunked.py` for focused `jsonmodem` versus Pydantic `jiter` comparisons on medium/large documents and chunked JSON sequences.
- [x] (2026-06-03T13:20:00Z) Ran fast pyperf smokes for chunked document and sequence comparisons at 64-byte chunks, then ran `python -m pyperf check` on both result files.
- [x] (2026-06-03T14:05:00Z) Added `crates/jsonmodem-py/benchmarks/profile_incremental.py` and profiled 8-byte chunk incremental parsing with `cProfile`, `perf stat`, and `perf record`.
- [x] (2026-06-03T14:20:00Z) Optimized the Python event iterators to reuse interned event/path tag strings from the parser object instead of interning them for every `feed()` iterator.
- [x] (2026-06-03T14:35:00Z) Ran `.agent/check-py.sh`; build, 24 Python tests, and docs passed. `pdoc` still emitted the existing non-fatal `DecodeMode.__hash__` warning.
- [x] (2026-06-03T14:45:00Z) Ran `PATH="$HOME/.local/bin:$PATH" .agent/check.sh`; rustfmt, release build, Rust tests, clippy, public docs, cfg-miri clippy, and actionlint passed. Miri was skipped by the script's default `AGENT_CHECK_MIRI_DISABLE=true`.
- [x] (2026-06-03T15:20:00Z) Added an internal Rust `Vec<EventRecord>` pool for normal event iterators, cached payload dictionary key strings, and avoided cloning active string paths on every continuation fragment.
- [x] (2026-06-03T15:30:00Z) Tested and rejected cached Python path tuples for repeated event paths; the cache lookup moved work into `feed()` and regressed the 5,000-chunk manual timing.
- [ ] (2026-06-03T16:05:00Z) Design and benchmark more aggressive allocation scheduling for the current event UX: private tuple shells, refcount-checked tuple recycling, lazy event objects that support unpacking, GIL release during Rust parsing, and a bounded background top-up thread for Python object shells when the GIL is available.
- [x] (2026-06-03T15:40:00Z) Ran `.agent/check-py.sh`; build, 24 Python tests, and docs passed with the same non-fatal `DecodeMode.__hash__` pdoc warning.
- [x] (2026-06-03T15:50:00Z) Ran `PATH="$HOME/.local/bin:$PATH" .agent/check.sh`; rustfmt, release build, Rust tests, clippy, public docs, cfg-miri clippy, and actionlint passed. Miri was skipped by the script's default `AGENT_CHECK_MIRI_DISABLE=true`.
- [x] (2026-06-03T16:15:00Z) Started and completed the full incremental allocation experiment series requested by the user. No staging or commit is planned. Each experiment below has either a kept implementation with validation or a rejected implementation with benchmark evidence.
- [x] (2026-06-03T17:15:00Z) Completed the first allocation experiment series: kept eager event materialization, kept `feed_many(chunks)`, kept active fragmented-string path reuse for `feed_many()`, rejected private tuple-shell pooling, and rejected lazy event objects.
- [x] (2026-06-03T17:25:00Z) Reran final focused tiny-chunk timings after cleanup and `.agent/check-py.sh`: per-chunk `jsonmodem_events` 50,000 chunks x 8 bytes x 10 repeats `0.401486s`; `jsonmodem_feed_many_events` `0.352570s`; per-chunk `jsonmodem_events` 5,000 chunks x 8 bytes x 20 repeats `0.080025s`; `jsonmodem_feed_many_events` `0.054655s`; `JsonModemPathFilter(byte_views=True)` 50,000 chunks x 8 bytes x 10 repeats `0.585677s`.
- [x] (2026-06-03T17:30:00Z) Reran cProfile for per-chunk `jsonmodem_events` with 50,000 chunks x 8 bytes x 2 repeats. Elapsed was `0.102735s`, with native `JsonModem.feed` at `0.066s` for 100,000 calls.
- [x] (2026-06-03T17:40:00Z) Added `jsonmodem_feed_many_chunked` and `jsonmodem_sequence_feed_many` to `bench_jiter_chunked.py`, then ran fast pyperf comparisons at 8-byte chunking for `response_large.json` and `sequence_large`. Both pyperf files passed `pyperf check` with expected `--fast` instability warnings.
- [x] (2026-06-03T17:50:00Z) Added a Python correctness test proving `feed_many(chunks)` matches repeated `feed(chunk)` for fragmented string events.
- [x] (2026-06-03T17:55:00Z) Final validation passed: `.agent/check-py.sh` built the extension, ran 25 Python tests, and generated docs with the existing non-fatal `DecodeMode.__hash__` pdoc warning; `PATH="$HOME/.local/bin:$PATH" .agent/check.sh` passed rustfmt, release build, Rust tests, clippy, public docs, cfg-miri clippy, and actionlint. Miri was skipped by the script's default `AGENT_CHECK_MIRI_DISABLE=true`.
- [x] (2026-06-03T18:05:00Z) After removing the rejected threaded prototype, reran final validation. `.agent/check-py.sh` passed with 25 Python tests and the existing non-fatal pdoc warning. `PATH="$HOME/.local/bin:$PATH" .agent/check.sh` passed rustfmt, release build, Rust tests, clippy, public docs, cfg-miri clippy, and actionlint; Miri was skipped by the script's default `AGENT_CHECK_MIRI_DISABLE=true`.
- [x] (2026-06-03T18:20:00Z) User clarified that full `.loads()` performance is not a goal. Updated this plan, repository benchmark instructions, Python benchmark docs, and the focused jiter harness so default comparisons parse the same stream of JSON fragments; reassembled full-document decodes are reference-only.
- [x] (2026-06-03T18:30:00Z) Smoke-tested the updated focused harness. `--group documents --list` now prints `jsonmodem_events_chunked`, `jsonmodem_feed_many_chunked`, and `jiter_cumulative_partial_prefixes`; `--group reference --list` prints only reference reassembled decodes for document workloads. `profile_incremental.py` ran `jsonmodem_events`, `jsonmodem_feed_many_events`, and `jiter_cumulative_partial` on 50 chunks x 8 bytes.
- [x] (2026-06-03T18:35:00Z) Ran `.agent/check-py.sh`; build, 25 Python tests, and docs passed with the existing non-fatal `DecodeMode.__hash__` pdoc warning.
- [x] (2026-06-03T18:55:00Z) Researched CPython/PyO3 object pooling options for moving Python-object allocation off the `feed()` hot path. The promising path is not a pool of exact builtin tuples, because CPython already freelists small tuples and exact tuple reuse is constrained by immutability and retained user references. The promising path is a custom PyO3 `#[pyclass(freelist = N)]` event object, ideally `frozen` and sequence-compatible for unpacking, plus a GIL-releasing Rust descriptor phase so a helper thread can acquire the GIL and top up object pools while Rust parsing runs.
- [x] (2026-06-03T19:20:00Z) Implemented the first pooled-object experiment: `JsonEvent` with PyO3 `#[pyclass(freelist = 65536, sequence)]`, `JsonModem.feed_objects()`, `feed_many_objects()`, `finish_objects()`, and `warm_event_pool(count)`. `JsonEvent` unpacks like `(kind, path, payload)` but materializes path and payload objects lazily.
- [x] (2026-06-03T19:35:00Z) Benchmarked pooled event objects. On the synthetic 50,000 x 8-byte fragment stream, tuple `feed_many()` count-only measured roughly `34.8 ms/run`, while pooled `feed_many_objects()` measured roughly `22.7 ms/run`; warming the event freelist before timing moved the parse call to roughly `21.3 ms` after a separate `5.1 ms` warm-up. Forced unpacking is slower with the lazy object path: tuple `feed_many()` unpack measured roughly `39.8 ms/run`, while pooled `feed_many_objects()` unpack measured roughly `68.9 ms/run`.
- [x] (2026-06-03T19:45:00Z) Ran a focused fast pyperf comparison on `response_large.json` split into 8-byte chunks. Results: `jsonmodem_events_chunked ~= 229 us`, `jsonmodem_feed_many_chunked ~= 140 us`, `jsonmodem_feed_many_objects_chunked ~= 128 us`, and cumulative-prefix `jiter.from_json(..., partial_mode=True) ~= 7.19 ms`. `python -m pyperf check` completed with expected fast-mode instability warnings.
- [x] (2026-06-03T20:00:00Z) Wrote `plans/python-performance/final-report.md`, ran `cargo fmt --check`, and reran `.agent/check-py.sh`; release build, 28 Python tests, pydoc, and pdoc passed with the existing non-fatal `DecodeMode.__hash__` pdoc warning.
- [x] (2026-06-04T01:31:37Z) Superseded the experimental API set after user feedback. Removed public `feed_many()`, `JsonEvent`, object-feed variants, and `warm_event_pool()`. `JsonModem.feed()` now accepts either one chunk or an iterable of chunks, returns exact outer tuples for fast unpacking, and uses lightweight `PathView` and `StringPayload` objects by default.
- [x] (2026-06-04T01:31:37Z) Updated Python docs, stubs, tests, focused jiter benchmark names, realistic benchmark access patterns, and benchmark data notes so active guidance centers on `feed()`. Historical plan entries remain for evidence but are no longer the current API.
- [x] (2026-06-04T01:31:37Z) Reran validation and focused timings for the single-feed design. `.agent/check-py.sh` passed with 26 tests; `cargo fmt --check`, `cargo check -p jsonmodem-py`, `pyperf check target/python-perf/jiter-cumulative-prefix-documents-feed-views-fast.json`, and `PATH="$HOME/.local/bin:$PATH" .agent/check.sh` passed. Fast-mode pyperf kept its normal instability warnings, pdoc emitted native-class `__hash__` warnings for generated stubs, and Miri was skipped by the script's default `AGENT_CHECK_MIRI_DISABLE=true`.
- [x] (2026-06-04T07:28:03Z) Ran the full cross-library comparison suite requested by the user and recorded results in `plans/python-performance/comparison-results-20260604.md`. Artifacts: `target/python-perf/jiter-all-8b-20260604.json`, `target/python-perf/full-decode-reference-20260604.json`, and `target/python-perf/realistic-all-20260604.json`. `pyperf check` passed for all three with expected fast-mode warnings.
- [x] (2026-06-04T07:28:03Z) Fixed `bench_json_libraries.py` so selected `--workload` and `--group` options are inherited by pyperf worker processes, matching the jiter and realistic harness behavior.
- [x] (2026-06-04T17:10:00Z) Removed experimental public Python helpers `loads()`, `string_ranges()`, and `string_range_table()` plus their active tests and benchmark imports. Added guards so byte-view mode accepts only stable byte-backed read-only memoryviews for no-copy payload views.
- [x] (2026-06-04T17:45:00Z) Addressed Codex review feedback on `JsonModemPathFilter` typing by documenting the default filtered event shape separately from `ByteViewEvent`.
- [x] (2026-06-04T18:10:00Z) Fixed the PR fuzz workflow after CI exposed a failing `libafl_libfuzzer` runtime build. The fuzz crate now uses `libfuzzer-sys 0.4.12`, and a local 5,000-run `cargo +nightly fuzz run fuzz_jsonmodem -- -runs=5000 -max_total_time=300` completed successfully.
- [x] (2026-06-04T18:20:00Z) Reran PR validation after review fixes. `.agent/check-py.sh` passed with 20 Python tests and the existing non-fatal pdoc warnings. `PATH="$HOME/.local/bin:$PATH" .agent/check.sh` passed rustfmt, release build, Rust tests, clippy, public docs, cfg-miri clippy, and actionlint; Miri was skipped by the script's default `AGENT_CHECK_MIRI_DISABLE=true`.

## Current Experiment Matrix

Baseline to preserve:

    jsonmodem_events, 50,000 chunks x 8 bytes, 10 repeats ~= 49.9-50.0 ms/run
    jsonmodem_events, 5,000 chunks x 8 bytes, 20 repeats ~= 4.86-5.5 ms/run
    JsonModemPathFilter(byte_views=True), 50,000 chunks x 8 bytes ~= 56.1-62.1 ms/run
    cumulative jiter partial parse, 50,000 chunks x 8 bytes ~= 2.93 s/run

Primary comparison rule: jsonmodem consumes each incoming fragment through its
incremental API. `jiter` document results must parse every cumulative prefix
with `partial_mode=True`. One-shot full-document decode is reference-only.

Experiments to finish:

1. Eager event materialization in `feed()` — kept for now
   Method: create Python `(kind, path, payload)` tuples while `feed()` already owns the GIL, store `PyObject` records, and make `__next__()` return already-built objects.
   Decision rule: keep only if total tiny-chunk time improves or if it enables a later retained-object pool without regressing validation.
   Result: `.agent/check-py.sh` passed with 24 tests. Manual timing improved `jsonmodem_events` 50,000 chunks x 8 bytes x 10 repeats to `0.400985s` total, about `40.1 ms/run`; 5,000 chunks x 8 bytes x 20 repeats to `0.088463s`, about `4.42 ms/run`. `JsonModemPathFilter(byte_views=True)` was unchanged at `0.561186s` total for 50,000 chunks x 8 bytes x 10 repeats.

2. Private Python event shell pool — rejected
   Method: preallocate/fill tuple shells or a custom event shell under the GIL, recycle only when refcount checks prove the caller did not retain the object, and abandon retained objects.
   Decision rule: keep only if retained-event tests pass and tiny-chunk benchmarks improve. Payload dictionaries remain unsafe to mutate/reuse unless replaced with immutable/lazy payload objects.
   Result: `.agent/check-py.sh` passed with 24 tests. Manual timing regressed `jsonmodem_events` 50,000 chunks x 8 bytes x 10 repeats to `0.432363s` total, about `43.2 ms/run`, versus eager-only `40.1 ms/run`; 5,000 chunks x 8 bytes x 20 repeats was `0.090976s`, about `4.55 ms/run`, versus eager-only `4.42 ms/run`. `JsonModemPathFilter(byte_views=True)` improved slightly to `0.547391s`, but that path does not use normal event tuples and is not evidence for keeping this change. The tuple-shell pool was removed.

3. Lazy event object compatible with tuple unpacking — rejected
   Method: return a sequence-like `JsonEvent` object with `__len__`, `__getitem__`, `__iter__`, and optional attributes. Store kind/path/payload descriptors in Rust and materialize Python path/payload objects only on access.
   Decision rule: keep as an opt-in API if normal unpacking works and forwarding/byte-view workloads avoid Python payload allocation.
   Result: implemented an opt-in `JsonModemLazy` with sequence-style event objects and benchmark modes for count-only and forced unpacking. `.agent/check-py.sh` passed with 24 tests. On 50,000 chunks x 8 bytes x 10 repeats, count-only lazy took `0.421882s` versus eager tuples at `0.414007s`, and forced unpacking took `0.795161s`. On 5,000 chunks x 8 bytes x 20 repeats, count-only lazy took `0.082898s` versus eager tuples at `0.086402s`, but forced unpacking took `0.165963s`. The small count-only 5k win does not justify an API that is worse on the larger target and much worse when users inspect fields, so the lazy API was removed.

4. Descriptor-first Rust batching — rejected for current event UX
   Method: parse into compact Rust event descriptors and materialize Python objects in a tight pass. Reuse Rust vectors, path ids, and retained input owners. Avoid `OwnedEvent`, owned paths, and `String` allocation unless escaping requires it.
   Decision rule: keep any descriptor builder that beats eager direct tuple construction and passes existing event tests.
   Result: implemented a temporary `feed_many_threaded(chunks)` prototype that moved parser work onto a worker thread, sent owned Rust event descriptors through a bounded channel, and materialized Python tuples on the caller side. `.agent/check-py.sh` passed with 25 tests. Manual timing was decisively worse: 50,000 chunks x 8 bytes x 10 repeats took `0.880676s` total, about `88.1 ms/run`, versus `feed_many()` at `0.346073s`, about `34.6 ms/run`; 5,000 chunks x 8 bytes x 20 repeats took `0.094884s`, about `4.74 ms/run`, versus `feed_many()` at `0.055970s`, about `2.80 ms/run`. The extra Rust descriptor allocation, path/string ownership, channel synchronization, and thread handoff cost more than any parse/materialize overlap. The threaded prototype was removed.

5. Path reuse and path-node allocation reduction — partly kept
   Method: avoid rebuilding complete path tuples for every event by using parser path state, parent path nodes, repeated current-path records, or bounded immutable path-object caching where lookup cost is demonstrably lower than rebuild cost.
   Decision rule: keep only if the benchmark improves; a previously tested full Python path tuple cache regressed and remains rejected.
   Result: implemented a narrower active-fragmented-string path cache. Using it for normal per-chunk `feed()` helped the 50,000-chunk case only slightly and regressed the 5,000-chunk case, so ordinary `feed()` now bypasses it. Keeping it for `feed_many()` materially improved batching: 50,000 chunks x 8 bytes x 10 repeats improved to `0.356772s` total, about `35.7 ms/run`; 5,000 chunks x 8 bytes x 20 repeats improved to `0.050905s` total, about `2.55 ms/run`.

6. Batch feed for tiny chunks — integrated into `feed()`
   Method: add `feed()` support for either one chunk or an iterable of chunks, consuming many HTTP/LLM-sized chunks in one Rust/Python call while preserving incremental event order.
   Decision rule: keep if it materially reduces per-chunk call/iterator overhead and stays ergonomic for streaming HTTP bodies and LLM token streams.
   Result: first implemented as `feed_many(chunks)`, then removed as a separate public API after user feedback. Current `JsonModem.feed(chunks)` keeps the batching benefit and returns the same event iterator shape as `JsonModem.feed(chunk)`: `2.78 ms/run` for 5,000 8-byte fragments count-only, `3.81 ms/run` with immediate unpack/access, `29.9 ms/run` for 50,000 fragments count-only, and `38.2 ms/run` with immediate unpack/access.

7. Coordinated parsing/allocation schedule — rejected for current event UX
   Method: inside Rust, schedule pure Rust parsing/allocation and CPython-object materialization as separate phases or producer/consumer work. The relevant boundary is CPython-object access versus pure Rust work, not Rust versus Python.
   Decision rule: keep only if it improves the all-in benchmark, including synchronization costs.
   Result: the threaded descriptor experiment is the measured version of this idea for the current event-tuple UX. It lost badly because the parser is already fast relative to object materialization, and making parser output thread-owned requires allocation that the eager direct path avoids.

8. Custom Python event freelist — measured and removed
   Method: replace exact builtin event tuples in an opt-in benchmark mode with a custom sequence-compatible `JsonEvent` pyclass using PyO3's `#[pyclass(freelist = N)]`. Store kind/path/payload descriptors in Rust fields, implement sequence unpacking, and materialize exact path tuples and payload dictionaries only when accessed. This differs from the rejected lazy prototype by using a CPython/PyO3 freelist for the object header and by returning owned `JsonEvent` objects directly from the iterator instead of cloning each yielded object.
   Decision rule: keep only if `for event in parser.feed_many_objects(chunks)` improves for 5,000 and 50,000 8-byte chunks. If exact tuple compatibility is required, keep this as an opt-in API rather than changing `feed()`.
   Result: the opaque-event path improved count-only consumption, but forced unpacking regressed badly and the user rejected API proliferation. The current implementation removed `JsonEvent`, object-feed methods, and explicit warming. It keeps a fast exact outer tuple and moves path/payload allocation behind lightweight `PathView` and `StringPayload` objects.

9. Object-pool top-up before parsing — measured and removed
   Method: split each batch into a Rust-only descriptor parse that runs with the GIL released and a Python-object materialization phase. A helper thread associated with the same interpreter can acquire the GIL during the Rust-only parse and replenish custom `JsonEvent`, path object, and payload shell pools. The helper must never touch parser state without Rust synchronization, and it must stop cleanly before interpreter finalization.
   Decision rule: keep only if the all-in `feed_many()` benchmark improves. This is expected to help only if descriptor parsing is long enough to hide pool top-up and the pool uses custom objects; it is unlikely to help exact builtin tuple/dict output because CPython's internal freelists already cover much of the raw memory reuse.
   Result: implemented a synchronous `warm_event_pool(count)` first because it gives an auditable version of "allocate before parsing" without background-thread lifetime risk. For 50,000 x 8-byte chunks, warming 50,008 event objects took roughly `5.1 ms` and reduced the following object-feed parse call from roughly `25.3 ms` cold to roughly `21.3 ms` warm. Because that only helped the removed object-feed API, the public warm-up API was removed.

Final rerun:

    .agent/check-py.sh
    profile_incremental.py for 5,000 and 50,000 8-byte chunks
    bench_jiter_chunked.py comparing jsonmodem parsing every chunk with jiter partial/cumulative parsing on the same chunk count
    realistic scenario smokes for HTTP nested extraction, LLM forwarding/tool-call partial JSON, nested extraction, and NDJSON/HAR-style data

Final fast jiter comparison at 8-byte chunking before the harness default was
corrected:

    response_large.json:
      reference-only jsonmodem.loads after reassembly ~= 123 us
      reference-only jiter.from_json after reassembly ~= 78.4 us
      jsonmodem per-chunk event stream ~= 231 us
      historical jsonmodem.feed_many event stream ~= 134 us

    sequence_large, 2,000 newline-delimited objects:
      jsonmodem per-chunk allow_multiple event stream ~= 15.8 ms
      historical jsonmodem.feed_many allow_multiple event stream ~= 21.3 ms
      buffered newline framing plus jiter.from_json per line ~= 3.34 ms
      reference-only jiter partial_mode=True on joined sequence ~= 62.9 us, but returns only the first value

Rerun the focused jiter harness after the 2026-06-03T18:20Z methodology update
when new headline numbers are needed:

    python crates/jsonmodem-py/benchmarks/bench_jiter_chunked.py --workload response_large.json --group documents --chunk-size 8 --fast --output target/python-perf/jiter-cumulative-prefix-documents.json
    python -m pyperf check target/python-perf/jiter-cumulative-prefix-documents.json

## Surprises & Discoveries

- Observation: there is no active local worktree dedicated to Python performance/profiling.
  Evidence: `git worktree list --porcelain` listed only `/home/friel/c/aaronfriel/jsonmodem`, `/home/friel/c/aaronfriel/jsonmodem-option-derive`, and `/home/friel/c/aaronfriel/jsonmodem-option-facet` before this plan created `/home/friel/c/aaronfriel/jsonmodem-python-perf`.

- Observation: current Python binding converts every event into Python tuples, path tuples, and payload dictionaries, and string fragments are copied into Rust `String` first.
  Evidence: `crates/jsonmodem-py/src/lib.rs` defines `OwnedEvent`, `OwnedPathComponent`, `OwnedStringFragment { fragment: String, ... }`, `convert_path`, `build_path_tuple`, and `build_payload`.

- Observation: true zero-copy `str` is not the right claim for CPython native strings because Python Unicode objects own decoded character storage.
  Evidence: this plan treats native `str` optimization as "avoid Rust allocation and create one Python string object from source bytes"; true borrowed views should use `bytes` or `memoryview` where callers can accept byte-oriented payloads.

- Observation: `origin/py` already includes the Python binding and tests, while `origin/perf` includes Jiter benchmark fixtures and single-chunk benchmark work.
  Evidence: `git ls-tree -r --name-only origin/py` shows `crates/jsonmodem-py/...`; `git ls-tree -r --name-only origin/perf` shows `crates/jsonmodem/benches/jiter_data/...` and `crates/jsonmodem/benches/single_chunk_json_large.rs`.

- Observation: current public sources confirm the competitor set and methodology tools.
  Evidence: Pydantic's `jiter` repository describes `PythonParse` for parsing JSON into Python objects; `orjson` describes itself as a fast, correct Python JSON library with `loads()` roughly 2x as fast as stdlib `json`; `msgspec` documentation describes its JSON implementation as one of the fastest Python options; `pyperf` documents JSON result files, stability checks, metadata, and before/after comparisons.

- Observation: the first native `loads()` implementation, built through `JsonModemValues`, improved over the event tuple API but was still far from competitor performance.
  Evidence: fast-mode `pyperf` on `medium_response.json` measured `jsonmodem_events` around `97.8 us`, `jsonmodem_loads` around `57.9 us`, stdlib `json` around `24.2 us`, `orjson` around `16.3 us`, `msgspec` around `17.0 us`, and `jiter` around `19.3 us`.

- Observation: direct Python object construction alone was not enough; removing full path allocation from the direct builder produced the first meaningful speedup.
  Evidence: on `medium_response.json`, `jsonmodem_loads` moved from roughly `58 us` to `48.3 us` after the builder stopped calling `convert_path()` for normal scalar/container events.

- Observation: larger nested response payloads still show `jsonmodem.loads()` close to the event API, so remaining cost is deeper than Python tuple construction.
  Evidence: fast-mode `pyperf` on `response_large.json` measured `jsonmodem_events` around `136 us`, `jsonmodem_loads` around `130 us`, stdlib `json` around `99.0 us`, `orjson` around `72.5 us`, `msgspec` around `73.5 us`, and `jiter` around `79.7 us`.

- Observation: all current parser contexts still maintain paths for every event, including `RawContext`.
  Evidence: `crates/jsonmodem/src/backend/raw.rs` defines `RawPath = Vec<PathItem<Vec<u8>, usize>>`; the native Python builder does not need full paths for native object construction, only stack order and object keys.

- Observation: `RawContext` is not a shortcut to the 10x goal for native Python values.
  Evidence: after switching `loads()` to `CoreJsonModem<RawContext>`, `.agent/check-py.sh` passed but fast-mode `pyperf` on `medium_response.json` measured `jsonmodem_loads` around `56.3 us`, slower than the accepted standard-backend direct builder result around `48.3 us`.

- Observation: using the parser lending iterator helps because `.to_iter()` clones event paths before the native builder can ignore most of them.
  Evidence: after replacing `.to_iter()` with `LendingIterator::next()` in `parse_native_value`, `medium_response.json` measured `jsonmodem_loads` around `45.4 us`, improved from the previous accepted result around `48.3 us`.

- Observation: a per-load Python string key cache does not help current native `loads()`.
  Evidence: `target/python-perf/key-cache-three-fixtures.json` measured `jsonmodem_loads` around `51.0 us` on `medium_response.json`, around `135 us` on `response_large.json`, and around `1.83 ms` on `string_array_unique.json`; the accepted non-cached medium result remained around `45.4 us`.

- Observation: Python-visible allocation is not the whole story for native `loads()`.
  Evidence: `--alloc-summary` reported `jsonmodem_loads` peaks of `6873` bytes on `medium_response.json`, `41792` bytes on `response_large.json`, and `545120` bytes on `string_array_unique.json`, but wall-clock timings still trail `orjson`, `msgspec`, and `jiter` for full native-object decode.

- Observation: returning Python range tuples is much faster than event tuples but still allocates one tuple per string value.
  Evidence: on `string_array_unique.json`, `jsonmodem_string_ranges` measured roughly `2.12 ms` and had a `tracemalloc` peak around `1.2 MB`; the event tuple API measured roughly `9.01 ms`.

- Observation: a compact byte-range table clears the 10x target for a realistic string-heavy workload.
  Evidence: `target/python-perf/fast-byte-scanner-table.json` measured `jsonmodem_string_range_table:string_array_unique.json` around `183 us` versus `jsonmodem_events:string_array_unique.json` around `9.01 ms`; `--alloc-summary` reported a peak around `80033` bytes for 10,000 string values.

- Observation: the ergonomic streaming input type should match ASGI body streaming.
  Evidence: the current Starlette request documentation at https://www.starlette.dev/requests/ shows `async for chunk in request.stream()` and states that byte chunks are provided without storing the full body in memory. `JsonModem.feed()` now accepts those chunks directly as `bytes`, plus `bytearray`, contiguous `memoryview`, and `str`.

- Observation: CPython does not expose a borrowed partial `str` object.
  Evidence: `Objects/unicodeobject.c` implements `PyUnicode_Substring()` by returning the original object only for the full range, returning the empty singleton for empty output, and otherwise constructing a new Unicode object with `_PyUnicode_FromASCII()` or `PyUnicode_FromKindAndData()`. The Unicode C API documentation also describes Unicode objects as owning canonical character storage.

- Observation: CPython `bytes` partial ranges also copy.
  Evidence: `Objects/bytesobject.c` implements partial `bytes` ranges with `PyBytes_FromStringAndSize(PyBytes_AS_STRING(self) + start, slicelength)`; only the full exact range returns a new reference to the original object.

- Observation: `memoryview` is the CPython-supported no-copy byte-range object.
  Evidence: the Python built-in type documentation says memory views access internal data of buffer-exporting objects without copying. The C API documentation says `PyMemoryView_GetContiguous()` points at the original memory when the exporter is contiguous, otherwise it copies into a new `bytes` object.

- Observation: jsonmodem already identifies borrowed string fragments in Rust.
  Evidence: `crates/jsonmodem/src/event.rs` exposes `ParseEvent::String { fragment: B::Str<'src>, ... }`; `StdBackend` uses `Cow<'src, str>` and `RawContext` uses `Cow<'src, [u8]>`; `crates/jsonmodem/src/parser/scanner/mod.rs` returns borrowed text when the token remains in the current input batch and has no escape/raw transformation.

- Observation: the current Python event API discards borrowed fragment information.
  Evidence: `crates/jsonmodem-py/src/lib.rs` stores each event in `OwnedEvent`; `OwnedStringFragment { fragment: String, ... }` is built with `fragment.into_owned()`, so even borrowed Rust fragments are copied before Python sees them.

- Observation: the byte-view payload can be opaque for many streaming uses.
  Evidence: callers that forward string fragments to a byte stream, append them to a byte accumulator, or hand them to another API can operate on `memoryview` directly. When text is needed, `payload["fragment"].tobytes().decode()` materializes a valid Python `str` for `is_view: True` fragments, while escaped fragments are already returned as `str`.

- Observation: Python streaming JSON competitors mostly optimize different tasks.
  Evidence: `plans/python-performance/streaming-json-research.md` records `json-stream` URL/iterator streaming, `ijson` iterative path extraction, `jiter`/Pydantic full native parsing, `orjson`/`msgspec` full-object parsing, and LLM partial JSON packages. The realistic benchmark harness keeps these groups separate instead of comparing every library to every scenario.

- Observation: memoryview payloads alone are not enough to beat full decode on one large LLM argument object.
  Evidence: the fast smoke run in `target/python-perf/realistic-llm-forward-smoke.json` measured roughly `jsonmodem_byteviews ~= 410 us`, `jsonmodem_events ~= 396 us`, stdlib `json ~= 98.5 us`, `jsonmodem_loads ~= 111 us`, `orjson ~= 24.2 us`, `msgspec ~= 25.6 us`, and `jiter ~= 29.9 us` for forwarding the `content` field from one generated 64 KiB tool-argument object. The event paths still construct path/event objects for the whole input, so the next useful optimization is path filtering or a lower-allocation target-field scanner.

- Observation: `ijson.items()` is the current ergonomic streaming target for nested extraction.
  Evidence: the fast smoke run in `target/python-perf/realistic-http-extract-smoke.json` measured roughly `jsonmodem_events ~= 7.67 ms`, `ijson_items ~= 1.14 ms`, stdlib `json ~= 754 us`, `jsonmodem_loads ~= 1.85 ms`, `orjson ~= 414 us`, `msgspec ~= 437 us`, and `jiter ~= 580 us` for extracting `items.item.metadata.etag` from a generated HTTP-style response. `jsonmodem` needs target-path filtering to avoid building unrelated Python events.

- Observation: the exact PyPI package names `jsoniter`, `json_iterator`, and `json-iterator` do not currently identify active packages through the PyPI JSON endpoint.
  Evidence: 2026-06-03 checks of `https://pypi.org/pypi/<name>/json` returned no package metadata for those names. This plan treats `jiter` as the relevant Pydantic-adjacent Python package and keeps spelling-variant GitHub searches as a research task when API limits allow it.

- Observation: `JsonModemPathFilter` materially improves nested HTTP extraction but does not yet beat `ijson.items()`.
  Evidence: `target/python-perf/realistic-http-extract-pathfilter-smoke.json` measured roughly `jsonmodem_events ~= 7.90 ms`, `jsonmodem_pathfilter ~= 2.16 ms`, `ijson_items ~= 1.15 ms`, stdlib `json ~= 746 us`, `jsonmodem_loads ~= 1.89 ms`, `orjson ~= 418 us`, `msgspec ~= 428 us`, and `jiter ~= 568 us`.

- Observation: path filtering alone does not improve the generated LLM content-forwarding benchmark much.
  Evidence: `target/python-perf/realistic-llm-forward-pathfilter-smoke.json` measured roughly `jsonmodem_pathfilter_byteviews ~= 401 us`, `jsonmodem_byteviews ~= 399 us`, `jsonmodem_events ~= 389 us`, stdlib `json ~= 97.6 us`, `jsonmodem_loads ~= 113 us`, `orjson ~= 24.5 us`, `msgspec ~= 24.9 us`, and `jiter ~= 29.6 us`. The input has few unrelated string events, so parser and path matching cost dominates.

- Observation: `json-stream` has an ergonomic streaming story for URL/file/iterator inputs, but transient objects are cursor-sensitive.
  Evidence: `json_stream.load(io.BytesIO(data))` can iterate the root object and then stream nested `items`, while random access after the stream cursor has moved can raise `TransientAccessException`. The benchmark uses root `.items()` iteration rather than random access.

- Observation: the expanded HTTP smoke confirms `json_stream` is not the speed target for nested extraction in this generated case.
  Evidence: `target/python-perf/realistic-http-extract-expanded-smoke.json` measured roughly `jsonmodem_events ~= 7.49 ms`, `jsonmodem_pathfilter ~= 2.08 ms`, `ijson_items ~= 1.12 ms`, `json_stream ~= 9.95 ms`, stdlib `json ~= 757 us`, `jsonmodem_loads ~= 1.88 ms`, `orjson ~= 415 us`, `msgspec ~= 431 us`, `jiter ~= 570 us`, `python_rapidjson ~= 833 us`, `pysimdjson ~= 599 us`, and `ujson ~= 574 us`.

- Observation: path filtering helps deep nested extraction by about 3.3x against owned jsonmodem events, but full native decode remains faster for this small generated input.
  Evidence: `target/python-perf/realistic-deep-nested-pathfilter-smoke.json` measured roughly `jsonmodem_events ~= 3.24 ms`, `jsonmodem_pathfilter ~= 984 us`, stdlib `json ~= 325 us`, `jsonmodem_loads ~= 695 us`, `orjson ~= 198 us`, `msgspec ~= 213 us`, `jiter ~= 254 us`, `python_rapidjson ~= 333 us`, `pysimdjson ~= 265 us`, and `ujson ~= 243 us`.

- Observation: matching borrowed parser paths before owned event conversion improves filtered HTTP extraction but does not remove the main remaining parser cost.
  Evidence: `target/python-perf/realistic-http-extract-lending-filter-smoke.json` measured roughly `jsonmodem_events ~= 7.55 ms`, `jsonmodem_pathfilter ~= 1.82 ms`, `ijson_items ~= 1.12 ms`, `json_stream ~= 9.89 ms`, stdlib `json ~= 751 us`, `jsonmodem_loads ~= 1.91 ms`, `orjson ~= 408 us`, `msgspec ~= 434 us`, `jiter ~= 568 us`, `python_rapidjson ~= 841 us`, `pysimdjson ~= 606 us`, and `ujson ~= 580 us`.

- Observation: the exact `jsoniter` package spellings still do not map to PyPI packages.
  Evidence: Python `urllib` checks on 2026-06-03 returned HTTP 404 for `https://pypi.org/pypi/jsoniter/json`, `json_iterator`, `json-iterator`, `json-iter`, and `jsoniterator`. The same check resolved `jiter 0.15.0`, `ijson 3.5.0`, `json-stream 2.5.1`, `orjson 3.11.9`, `msgspec 0.21.1`, `pysimdjson 7.0.2`, `python-rapidjson 1.23`, `ujson 5.12.1`, `jsonriver 1.0.0`, `partial-json-parser 0.2.1.1.post7`, `streaming-json-parser 0.1.0`, and `json-streamer 0.1.0`.

- Observation: partial-parser packages optimize different developer experiences than jsonmodem byte forwarding.
  Evidence: `target/python-perf/realistic-llm-partial-smoke.json` measured roughly `jsonmodem_pathfilter_byteviews ~= 413 us`, `jsonriver ~= 12.4 ms`, `partial_json_parser ~= 509 ms`, and `json_streamer ~= 75.6 ms`. `jsonriver` yields progressively complete Python values; `partial-json-parser` repairs and loads cumulative partial text; `json-streamer` yields parser state and partial objects.

- Observation: `streaming-json-parser` is researched but skipped from executable benchmarks for now.
  Evidence: importing `streaming_json_parser.streaming_json_parser` failed with `ModuleNotFoundError: No module named 'src.streaming_json_parser'` after installing version `0.1.0`.

- Observation: the HAR/API capture workload gives jsonmodem a stronger streaming-path comparison than the smaller HTTP fixture, but full decode is still faster on this generated in-memory body.
  Evidence: `target/python-perf/realistic-har-extract-smoke.json` measured roughly `jsonmodem_events ~= 13.3 ms`, `jsonmodem_pathfilter ~= 2.67 ms`, `json_stream ~= 13.7 ms`, stdlib `json ~= 1.18 ms`, `jsonmodem_loads ~= 3.07 ms`, `orjson ~= 580 us`, `msgspec ~= 602 us`, `jiter ~= 799 us`, `python_rapidjson ~= 1.16 ms`, `pysimdjson ~= 855 us`, and `ujson ~= 797 us`.

- Observation: `jiter` remains faster than `jsonmodem.loads()` for medium and large whole-document parsing even when the input arrives in small chunks and both APIs reassemble before parsing.
  Evidence: `target/python-perf/jiter-chunked-documents-smoke.json` used 64-byte chunks. `medium_response.json` measured roughly `jsonmodem_loads_reassembled ~= 45.1 us`, `jiter_reassembled ~= 19.1 us`, and `jsonmodem_events_chunked ~= 135 us`. `response_large.json` measured roughly `jsonmodem_loads_reassembled ~= 126 us`, `jiter_reassembled ~= 78.8 us`, and `jsonmodem_events_chunked ~= 369 us`.

- Observation: for newline-delimited JSON sequences broken into arbitrary 64-byte chunks, buffered per-line `jiter.from_json()` is much faster than the current Python event-stream path.
  Evidence: `target/python-perf/jiter-chunked-sequences-smoke.json` measured `sequence_medium` at roughly `jsonmodem_sequence_chunked ~= 6.91 ms` versus `jiter_sequence_buffered_lines ~= 859 us`, and `sequence_large` at roughly `jsonmodem_sequence_chunked ~= 27.4 ms` versus `jiter_sequence_buffered_lines ~= 3.47 ms`. `jiter_sequence_partial_first` measured `16.7 us` and `61.9 us` respectively, but it parses only the first JSON value in the joined sequence, so it is recorded as a semantics check rather than a full-sequence competitor.

- Observation: cumulative `jiter.from_json(..., partial_mode=True)` on every tiny chunk is much more expensive than jsonmodem's true feed API.
  Evidence: manual median timings on a single object with a large `"content"` string split into 8-byte chunks measured 5,000 chunks / 39,994 bytes at `jsonmodem_events ~= 8.43 ms`, `JsonModemPathFilter(byte_views=True) ~= 9.16 ms`, and cumulative jiter partial parsing on every chunk `~= 31.4 ms`. At 50,000 chunks / 399,994 bytes, the same methods measured `jsonmodem_events ~= 81.9 ms`, path-filter byte views `~= 92.1 ms`, and cumulative jiter partial parsing `~= 2.93 s`. Reassemble-once parsing stayed much faster for both libraries but does not provide incremental results.

- Observation: the first tiny-chunk profile showed repeated Python string interning as avoidable overhead.
  Evidence: `cProfile` on three 50,000-chunk `jsonmodem_events` runs showed 150,000 `JsonModem.feed()` calls taking about `0.172 s` inside the native method out of about `0.291 s` elapsed. `perf report` on `target/python-perf/perf-jsonmodem-events.data` showed large samples in `_Py_HashBytes`, `PyUnicode_InternInPlace`, `PyObject_Malloc`, `PyTuple_New`, and `PyUnicode_FromStringAndSize`, while Rust parser scanning functions were only a small part of the profile.

- Observation: reusing interned tag strings per parser object removed the largest interning cost and improved tiny-chunk incremental timings.
  Evidence: after storing `InternedStrings` on `JsonModem`, `JsonModemByteViews`, and `JsonModemPathFilter`, median timings on 8-byte chunks improved to 5,000 chunks / 39,994 bytes at `jsonmodem_events ~= 5.95 ms` and `JsonModemPathFilter(byte_views=True) ~= 6.75 ms`; 50,000 chunks / 399,994 bytes improved to `jsonmodem_events ~= 58.9 ms` and path-filter byte views `~= 67.7 ms`. The post-fix perf report in `target/python-perf/perf-jsonmodem-events-after-intern-cache.data` no longer shows `PyUnicode_InternInPlace` as a top cost; remaining cost is Python/PyO3 event object allocation, tuple/path/payload construction, and one Python iterator per `feed()` result.

- Observation: the incremental comparison the user cares about is favorable to jsonmodem when `jiter` is asked to reparse the cumulative partial buffer after every tiny chunk.
  Evidence: after the interned-tag fix, 5,000 chunks x 8 bytes measured `jsonmodem_events ~= 5.95 ms` versus cumulative `jiter.from_json(..., partial_mode=True)` at `~= 31.1 ms`; 50,000 chunks x 8 bytes measured `jsonmodem_events ~= 58.9 ms` versus cumulative jiter partial parsing at `~= 3.06 s`. The remaining problem is not that jiter is faster for the true incremental feedback model; it is that jsonmodem still spends about one microsecond per chunk/event constructing Python-visible event objects.

- Observation: pooling Rust-owned event record buffers is safe and keeps the existing Python iterator UX.
  Evidence: `JsonModem` and non-byte-view `JsonModemPathFilter` now own an internal `Vec<EventRecord>` pool. `feed()` takes a buffer from the pool, fills it, gives it to `PyEventIter`, and `PyEventIter::drop()` clears and returns buffers up to capacity 1024 while bounding the pool at 32 buffers. Returned Python tuples and dictionaries remain fresh and can still be retained by user code.

- Observation: caching immutable payload dictionary key strings is a safe Python-object reuse win.
  Evidence: `InternedStrings` now also keeps `fragment`, `is_initial`, `is_final`, and `is_view` keys. Payload dictionaries are still freshly allocated, but repeated key string creation and hash setup are removed from string-fragment event construction.

- Observation: the active string tracker was doing unnecessary path cloning for continuation fragments.
  Evidence: `OwnedEvent::from_parse_event()` and `OwnedEvent::from_borrowed_parse_event()` now check `contains()` before inserting into `active_strings`, so only the first fragment for a path clones the path into the tracker. Final fragments still remove the tracked path.

- Observation: caching complete Python path tuples did not help this workload.
  Evidence: a tested cache of immutable Python path tuples preserved the visible event shape, but manual timing regressed the 5,000 chunks x 8 bytes case from about `5.1 ms` per run after the accepted pooling/key changes to about `6.3 ms` per run, and cProfile showed more native `feed()` time. The cache was removed.

- Observation: inside `feed()`, jsonmodem controls the Rust implementation and can arrange parsing, Rust allocation, queues, arenas, and event-descriptor construction however it wants. The only hard boundary is touching CPython objects: tuple/dict/string/refcount work must happen while a thread has the GIL.
  Evidence: a useful pipeline is not “Rust thread versus GIL thread”; it is “CPython-object work versus pure Rust work.” The implementation can parse one region while allocating Python objects for another, can parse into Rust descriptors and materialize Python objects later, can reserve Rust arenas ahead of demand, and can fill private Python shells before exposing them to the caller.

- Observation: exact tuple pooling is still possible as an experiment if pooled tuples are treated as private shells and are recycled only when no user reference escaped.
  Evidence: CPython's `PyTuple_SetItem()` steals the new item reference and discards the previous item reference, while the faster unchecked macro is only for brand-new tuples. A compatibility-mode prototype can preallocate `(None, None, None)` event tuples, fill private tuples before returning them, and later recycle only tuples whose reference count proves the iterator is the sole owner. This is CPython-specific and must be guarded by tests that retain events, retain paths, hash events, and interleave iterator destruction.

## Decision Log

- Decision: base this worktree on `origin/py`, not `main`.
  Rationale: `origin/py` contains the current PyO3 binding, tests, and Python setup scripts. Starting from `main` would require reconstructing that work before benchmarking or optimizing Python behavior.
  Date/Author: 2026-06-03 / Codex

- Decision: compare `jsonmodem` against both object-building APIs and streaming/iterator APIs, but keep their results in separate benchmark groups.
  Rationale: comparing event tuples to `orjson.loads()` is misleading. A fair harness must separate native Python value construction, streaming event iteration, partial extraction, and byte-range payload modes.
  Date/Author: 2026-06-03 / Codex

- Decision: full `.loads()` performance is reference-only for this plan; all headline Python optimization claims must compare streams of JSON fragments.
  Rationale: the user clarified they only care about optimizing the incremental parsing API. A one-shot full-document decode does not answer whether `jsonmodem` is fast for HTTP body chunks, nested JSON streaming, or LLM tool-call fragments. For `jiter`, the document comparison must parse every cumulative prefix with `partial_mode=True`.
  Date/Author: 2026-06-03 / User + Codex

- Decision: treat the 10x target as 10x over the current `jsonmodem` Python incremental event binding, then report where the optimized incremental path lands against `jiter` cumulative-prefix partial parsing and other streaming competitors.
  Rationale: beating one-shot decoders on full native-object construction is not the user goal. A clear incremental baseline prevents benchmark claims from becoming ambiguous.
  Date/Author: 2026-06-03 / Codex

- Decision: use `pyperf` JSON output for timing evidence and separate allocation evidence from timing evidence.
  Rationale: `pyperf` records benchmark metadata, supports stability checks, and makes before/after comparison auditable. Allocation counts need `tracemalloc`, Python object counters, or native profilers, not wall-clock timing alone.
  Date/Author: 2026-06-03 / Codex

- Decision: treat this plan as an implementation plan, not a setup-only plan.
  Rationale: the user explicitly corrected the scope and expects the methodology, direct comparison, and optimization work to be completed with this plan as the durable tracker.
  Date/Author: 2026-06-03 / Codex

- Decision: add `jsonmodem.loads(data)` as a historical reference API, not as the current optimization target.
  Rationale: the existing event API is not comparable to `orjson.loads()` or `msgspec.json.decode()`, so the one-shot native decode API was useful to separate semantics. The user later clarified that this plan should optimize incremental parsing instead.
  Date/Author: 2026-06-03 / Codex

- Decision: use a direct Python object builder instead of converting through the Rust `Value` tree for `loads()`.
  Rationale: building a Rust `Value` tree and then cloning/converting it to Python performs redundant allocation. The direct builder can construct Python lists and dictionaries as parser events arrive.
  Date/Author: 2026-06-03 / Codex

- Decision: supersede the earlier native `loads()` optimization direction.
  Rationale: direct Python building still paid parser path-maintenance cost, and `StdBackend::push_key_from_str()` created a fresh `Arc<str>` for each property name. That finding is useful historical context, but the current optimization target is incremental parsing over fragment streams. Path/key allocation work should be pursued only when it improves `feed()`, path filtering, byte views, or a future incremental sink API.
  Date/Author: 2026-06-03 / Codex

- Decision: reject the `RawContext` native loads experiment.
  Rationale: it preserved full path maintenance, added byte-to-text conversion at dictionary/string insertion, and regressed the accepted medium benchmark result.
  Date/Author: 2026-06-03 / Codex

- Decision: keep the lending-iterator native builder change.
  Rationale: it removes owned event/path cloning in `loads()` while preserving the public event-stream API and passing the Python binding test suite.
  Date/Author: 2026-06-03 / Codex

- Decision: reject a per-load Python string key cache for `loads()`.
  Rationale: the Rust `HashMap` lookup and key ownership overhead outweighed any reduction in CPython dictionary key conversion on the tested fixtures.
  Date/Author: 2026-06-03 / Codex

- Decision: expose two byte-oriented APIs instead of claiming Python `str` substring borrowing.
  Rationale: `string_ranges(data)` is ergonomic for Python callers and returns `(start, end)` tuples or `None`; `string_range_table(data)` is the fast path and returns packed little-endian `u32` offset pairs with `u32::MAX` sentinels for materialized strings.
  Date/Author: 2026-06-03 / Codex

- Decision: use a direct byte scanner for `string_range_table(data)`.
  Rationale: the full event parser and parser-backed range table still paid event machinery that the packed offset table did not need. The direct scanner validates structure, literals, numbers, and string escapes for valid JSON workloads while emitting only string-value offsets.
  Date/Author: 2026-06-03 / Codex

- Decision: keep the current streaming event API fully owned while making input borrowing cheap.
  Rationale: `feed()` can safely borrow `bytes`, `bytearray`, and contiguous `memoryview` input for the duration of the call because emitted event tuples own their Python strings and path components after the call returns. A future no-copy event payload API must either return offsets tied to retained chunk owners or a segmented byte representation for strings that span chunks.
  Date/Author: 2026-06-03 / Codex

- Decision: do not promise no-copy Python `str` substrings.
  Rationale: CPython partial `str` results are independent Unicode objects, and `str` does not expose the buffer protocol. The honest fast API should return byte-oriented views for unescaped UTF-8 payloads and provide explicit `.text()` or `str(view)` materialization when callers need text.
  Date/Author: 2026-06-03 / Codex

- Decision: design a separate byte-view streaming API instead of changing `JsonModem.feed()` payload semantics.
  Rationale: existing event tuples are stable and easy to retain. A byte-view API can require immutable `bytes` chunks for no-copy payloads, retain those chunks, and return `memoryview` objects or offset records. Mutable `bytearray`/writable `memoryview` input should either be copied, rejected for no-copy mode, or documented as a caller-owned mutation hazard.
  Date/Author: 2026-06-03 / Codex

- Decision: expose `JsonModemByteViews` now as the first no-copy streaming API.
  Rationale: it preserves the existing `(kind, path, payload)` event structure while changing only string payload fragments. `payload["fragment"]` is a `memoryview` when bytes were borrowed from input and a `str` when JSON escaping or parser state required materialization. `payload["is_view"]` lets callers branch cheaply.
  Date/Author: 2026-06-03 / Codex

- Decision: maintain both fixture benchmarks and generated realistic scenarios.
  Rationale: checked-in Jiter fixtures are useful for reproducibility and competitor comparison, while generated scenarios let the harness model source-backed application behavior without vendoring large or license-sensitive external datasets.
  Date/Author: 2026-06-03 / Codex

- Decision: add path-filtered streaming as the next API experiment.
  Rationale: the realistic LLM forwarding smoke result shows that avoiding payload copies does not matter enough when the Python binding still creates every event and every path. For common use cases like "forward `content`" or "extract `items.*.metadata.etag`", a user should be able to subscribe to target paths and avoid building unrelated Python events.
  Date/Author: 2026-06-03 / Codex

- Decision: keep `JsonModemPathFilter` as a user-facing API experiment.
  Rationale: it is easy to type, fits HTTP response and LLM chunk use cases, and reduced nested HTTP extraction time by roughly 3.7x against the owned event API. It still needs Rust-side path matching improvements before it can beat `ijson.items()` on the generated HTTP extraction case.
  Date/Author: 2026-06-03 / Codex

- Decision: include `json-stream` as a realistic streaming UX comparator, not as a full native decode competitor.
  Rationale: `json-stream` is designed around file, URL, and iterator streaming plus nested traversal. Its benchmark result should be read beside `ijson` and `JsonModemPathFilter`, not beside `orjson.loads()`.
  Date/Author: 2026-06-03 / Codex

- Decision: stop short of a larger path-filtering backend in this pass.
  Rationale: the low-risk lending-iterator change reduced HTTP filtered extraction from roughly `2.08 ms` to roughly `1.82 ms`. The remaining likely cost is inside `StdBackend` path maintenance and pattern comparison for every parser event. Avoiding that requires a parser/backend mode that tracks filter progress and emits only target events, which should be designed and tested as a separate Rust change.
  Date/Author: 2026-06-03 / Codex

- Decision: prioritize an incremental sink or compact batch API before more micro-optimizing the existing event tuple API.
  Rationale: after interning reuse, profiles point at Python object construction rather than Rust scanning. The next large gain should avoid constructing `(kind, path, payload)` tuples for every fragment. Candidate APIs are a path-targeted byte sink that forwards only matching string bytes or a compact result table containing retained input owners plus byte ranges. These APIs match HTTP body chunks and LLM token streams better than forcing callers through one iterator object per chunk.
  Date/Author: 2026-06-03 / Codex

- Decision: do not blindly reuse returned Python event tuples or payload dictionaries under the current UX, but allow a CPython-specific private-shell tuple experiment.
  Rationale: callers can retain every returned event tuple, and string payload dictionaries are mutable. Reusing live objects would break retained-event semantics. A narrower experiment may preallocate valid private tuple shells, fill them before exposure, and recycle them only when reference-count checks show no user reference escaped. This must remain behind a benchmarked implementation flag until correctness and performance are proven.
  Date/Author: 2026-06-03 / Codex

- Decision: keep the Rust event-record buffer pool and payload-key cache, but reject the Python path tuple cache.
  Rationale: the buffer pool directly addresses tiny-chunk allocation churn without changing Python ownership. Payload keys are immutable and repeated. The path tuple cache added lookup overhead in the hottest path and did not reduce total end-to-end cost in the manual tiny-chunk benchmark.

- Decision: do not ship the lazy `JsonEvent` sequence object as a separate API.
  Rationale: the object-feed experiment proved the opaque-event idea can reduce count-only overhead, but it regressed immediate unpacking and created API proliferation. The shipped direction is a single `JsonModem.feed()` that returns an exact outer tuple for unpacking and uses `PathView` / `StringPayload` to defer the expensive inner containers.
  Date/Author: 2026-06-03 / Codex

## Outcomes & Retrospective

Current outcome: the benchmark harness, competitor comparison, `JsonModemByteViews`, `JsonModemPathFilter`, single `JsonModem.feed()` API, and tiny-chunk profiling harness are implemented in this worktree. `JsonModem.feed()` accepts either one chunk or an iterable of chunks, returns an exact outer tuple for fast unpacking, and uses `PathView` / `StringPayload` for lower-allocation path and string payload access. The historical `loads()` path improved the medium fixture from roughly `97.8 us` for event tuples to roughly `45.4 us`, but full native-object decode is not the current goal and the helper is no longer public API.

Historical note: byte-range extraction already clears a 10x internal baseline for one string-heavy task. On `string_array_unique.json`, the direct byte table path measured roughly `183 us` versus `9.01 ms` for the event tuple API, about `49x` faster, while keeping the source bytes as the payload owner and returning offsets into that input. The remaining target is the incremental stream API.

For the user's true incremental benchmark, jsonmodem is faster than cumulative `jiter` partial parsing and the single `feed(chunks)` path is the current recommended API. The next performance phase should avoid Python event tuple construction for target string forwarding by adding a focused sink API or a compact byte-range batch API, but that would be a deliberate new capability rather than another feed variant.

Remaining work before a PR: optimize path filtering and event emission below the current parser path-maintenance and Python-object construction cost, then rerun the fair fragment-stream comparisons. Do not reintroduce full-document decode or string-range helper APIs unless the public API direction changes explicitly.

## Context and Orientation

The Rust parser crate lives in `crates/jsonmodem`. The Python extension lives in `crates/jsonmodem-py` and is built with PyO3 and maturin. The current Python API exposes `JsonModem.feed()` and `JsonModem.finish()` returning iterators of `(kind, path, payload)` tuples. `feed()` accepts one chunk or an iterable of chunks. That API is useful for low-level streaming, but it still creates Python objects per parser event and must be benchmarked against other incremental or partial parsers, not against one-shot full-document decode.

This work adds performance-oriented incremental Python paths. "Byte-range mode" means string-like JSON payloads can be exposed as `bytes` or `memoryview` referring to the original input buffer where JSON escaping and UTF-8 validation permit it. "Native Python value" and `loads()` work are retained only as historical measurement context and are not part of the public API.

The existing Rust performance plan at `plans/perf/jsonmodem_jiter_execplan.md` is relevant because it identifies Jiter datasets and single-chunk Rust parsing gaps. This Python plan should import those datasets or reproduce them in this worktree before running Python comparisons.

## Benchmark Methodology

The harness must avoid comparing unlike behavior. It should define these benchmark groups:

1. Incremental stream fragments:
   - `JsonModem.feed(chunk)` for every incoming fragment, then `finish()`
   - `JsonModem.feed(chunks)` for the same fragment boundaries in one call, then `finish()`
   - `JsonModemPathFilter` and `JsonModemByteViews` for selected-path and byte-view variants
   - `jiter.from_json(cumulative_prefix, partial_mode=True)` after every fragment for document workloads
   - other partial parsers on the same fragment boundaries when their API supports partial progress

2. Event or iterator scan:
   - current `JsonModem.feed()` plus `finish()` while consuming every event
   - Jiter iterator API if exposed to Python in a comparable form
   - no `orjson` number in this group unless the operation actually scans comparable events

3. Partial extraction:
   - extract one or a few nested fields from a large payload
   - compare `jsonmodem` streaming extraction against full-object decode plus Python indexing for competitors
   - report this separately because it is a valid workload where streaming can avoid full allocation

4. Byte-range payload mode:
   - no-escape ASCII strings and large string bodies
   - compare `jsonmodem` `bytes`/`memoryview` outputs against full Python `str` decodes
   - label the result as byte-oriented, not as a replacement for `loads()`

5. Reference-only full native object decode:
   - `json.loads(bytes_or_str)`, `orjson.loads(bytes)`, `msgspec.json.decode(bytes)`, `jiter.from_json(bytes)`, and `jsonmodem.loads(bytes)`
   - keep these numbers clearly labeled as full-document reference results
   - do not use this group as evidence for or against the incremental optimization target

Use `pyperf.Runner` for timings. Write raw results under `target/python-perf/` or `tmp/python-perf/`, not under version control. For quick iteration use `--fast`; for publishable claims use default or `--rigorous`, then run `python -m pyperf check` and `python -m pyperf compare_to`.

Record hardware, OS, Python version, package versions, CPU governor notes, input file checksums, and command lines for every claim.

## Realistic Workloads

Start with checked-in or reproducible files:

- Jiter fixtures from `origin/perf`: `medium_response.json`, `response_large.json`, `string_array.json`, `string_array_unique.json`, `floats_array.json`, `massive_ints_array.json`, `true_object.json`, and `unicode.json`.
- GitHub activity payloads similar to orjson benchmark fixtures: nested dictionaries, arrays of dictionaries, timestamps, URLs, repeated keys, and mixed string/number fields.
- Conda `current_repodata.json` style workload used by msgspec examples: large nested package metadata with a query for selected fields rather than full object retention.
- LLM/tool-call streaming payloads already represented by jsonmodem medium/large examples: long strings, partial code blocks, nested function-call arguments, and incremental chunks.
- NDJSON/log style data: repeated small objects with common keys and string-heavy values.

Each workload must state what a real application does with the data. Examples: decode entire object for downstream Python processing, scan for one field, stream code text, read many log records, or materialize only package names and sizes.

## Plan of Work

First, create `crates/jsonmodem-py/benchmarks/` with a `pyperf` harness and a small loader module. The harness should install optional competitors only through documented extra dependencies or a benchmark requirements file. It must skip missing optional libraries with an explicit message rather than failing the whole suite.

Second, import benchmark data. Prefer copying the Jiter fixture files from `origin/perf` using `git show origin/perf:path > local file` only for files that are small enough and license-compatible for this repository. For large external datasets, add a script under `crates/jsonmodem-py/benchmarks/` that downloads or synthesizes them into `target/python-perf/data/` with checksums.

Third, run a baseline for the current event binding. Measure total event consumption, object allocation pressure, and memory peak. This baseline is the denominator for the 10x target.

Fourth, keep native-value experiments as historical reference results only. They
are useful for explaining why full-document decode is not the target, but no
public `jsonmodem.loads()` API should be shipped from this work and no new
optimization work should be planned around full-document native decode in this
ExecPlan.

Do not use one-shot native decode timings as headline evidence for incremental
parser performance.

Fifth, add a byte-range API for callers who can work with raw UTF-8 bytes. Candidate public names:

    jsonmodem.scan_bytes(data: bytes | memoryview, paths=None) -> iterator
    JsonModemBytes(...).feed(...)

This API should keep the original Python bytes object alive and return `memoryview` views or bytes references for no-escape string segments where possible. If escaping is present, return a decoded object and mark that the payload was materialized.

Sixth, optimize based on profiles, not guesses. Use `py-spy`, `perf`, `cargo flamegraph`, Python allocation tracing, and targeted Rust profiles. Record every accepted and rejected optimization in this plan. The profiling target is the all-in cost of repeatedly feeding small fragments and consuming emitted events or byte-view payloads.

The next implementation phase should reduce Python object creation in the
incremental APIs. Candidate directions are a target-path byte sink, a compact
batch result that returns retained input owners plus byte ranges, and a parser
mode that tracks only the paths needed by the caller. Improvements must be
measured against the same fragment stream and against `jiter` cumulative-prefix
partial parsing.

For true no-copy byte payloads in the streaming event parser, add source span metadata to string events. The Python API should retain the original `bytes` object and return `memoryview` objects for no-escape strings that lie wholly within a retained input buffer. If the string spans chunks or contains escapes, the API must either materialize a Python `str`/`bytes` object or return a segmented representation that clearly documents it is not one contiguous view. The former one-shot `string_range_table(data)` experiment proved compact offsets can be fast, but it is not public API in this PR.

The byte-view streaming design should use these rules:

- `JsonModemByteViews.feed(chunk: bytes) -> Iterator[ByteEvent]` is the primary no-copy API. It retains each `bytes` owner until no emitted event can reference it.
- For an unescaped string fully contained in one retained chunk, the payload is a `memoryview` over that chunk's payload bytes.
- For a string split across chunks without escapes, either emit multiple fragment events, each with a `memoryview`, or return a `SegmentedBytes` object that holds `(owner, start, end)` records. A single contiguous `memoryview` would require copying.
- For strings with escapes or Unicode escape decoding, return an owned `str`/`bytes` payload or a lazy object that decodes on demand. There is no honest contiguous no-copy decoded output.
- For `bytearray` and writable `memoryview` input, no-copy mode should default to rejecting the chunk or copying it into immutable `bytes`, because Python code can mutate the buffer after parsing and change later payload views.
- For `str` input, no-copy byte payloads are not meaningful unless the API first encodes to UTF-8, which copies. `str` should remain supported by the existing owned event API.

## Concrete Steps

All commands run from `/home/friel/c/aaronfriel/jsonmodem-python-perf`.

1. Set up the Python extension environment:

       .agent/setup-py.sh
       .agent/check-py.sh

2. Add benchmark dependencies:

       uv pip install -r crates/jsonmodem-py/benchmarks/requirements-bench.txt

   The requirements file currently includes `pyperf`, `orjson`, `msgspec`,
   `jiter`, `ijson`, `json-stream`, `pysimdjson`, `python-rapidjson`,
   `ujson`, `jsonriver`, `partial-json-parser`, `streaming-json-parser`, and
   `json-streamer`.

3. Create the benchmark harness:

       crates/jsonmodem-py/benchmarks/bench_json_libraries.py
       crates/jsonmodem-py/benchmarks/requirements-bench.txt
       crates/jsonmodem-py/benchmarks/data/README.md

4. Run quick baseline:

       python crates/jsonmodem-py/benchmarks/bench_json_libraries.py --fast --output target/python-perf/baseline.json
       python -m pyperf check target/python-perf/baseline.json

   This is a reference-only full-decode/event baseline. It is not the headline
   benchmark for this plan.

5. Run the focused incremental jiter comparison:

       python crates/jsonmodem-py/benchmarks/bench_jiter_chunked.py --workload response_large.json --group documents --chunk-size 8 --fast --output target/python-perf/jiter-cumulative-prefix-documents.json
       python crates/jsonmodem-py/benchmarks/profile_incremental.py --mode jsonmodem_events --chunks 50000 --chunk-size 8 --repeats 10
       python crates/jsonmodem-py/benchmarks/profile_incremental.py --mode jsonmodem_feed_chunks_events --chunks 50000 --chunk-size 8 --repeats 10
       python crates/jsonmodem-py/benchmarks/profile_incremental.py --mode jiter_cumulative_partial --chunks 50000 --chunk-size 8 --repeats 1
       python -m pyperf check target/python-perf/jiter-cumulative-prefix-documents.json

6. Keep historical native-value API tests passing:

       cargo test -p jsonmodem-py
       pytest crates/jsonmodem-py/tests
       .agent/check-py.sh

7. Run reference-only full-decode before/after comparison only when needed to
   catch regressions:

       python crates/jsonmodem-py/benchmarks/bench_json_libraries.py --fast --output target/python-perf/native-values.json
       python -m pyperf compare_to target/python-perf/baseline.json target/python-perf/native-values.json --table

8. Run realistic scenario smokes:

       python crates/jsonmodem-py/benchmarks/bench_realistic_scenarios.py --scenario http_nested_response --group http_extract --fast --output target/python-perf/realistic-http-extract.json
       python crates/jsonmodem-py/benchmarks/bench_realistic_scenarios.py --scenario llm_tool_arguments --group llm_forward --fast --output target/python-perf/realistic-llm-forward.json
       python crates/jsonmodem-py/benchmarks/bench_realistic_scenarios.py --scenario llm_tool_arguments --group llm_partial --fast --output target/python-perf/realistic-llm-partial.json
       python crates/jsonmodem-py/benchmarks/bench_realistic_scenarios.py --scenario har_capture --group har_extract --fast --output target/python-perf/realistic-har-extract.json
       python -m pyperf check target/python-perf/realistic-http-extract.json
       python -m pyperf check target/python-perf/realistic-llm-forward.json
       python -m pyperf check target/python-perf/realistic-llm-partial.json
       python -m pyperf check target/python-perf/realistic-har-extract.json

9. Run full repository checks before publishing or merging:

       .agent/check.sh
       .agent/check-py.sh

## Validation and Acceptance

The benchmark harness is accepted when it can run on a fresh Python environment, records package versions, skips missing optional competitors clearly, and produces stable `pyperf` JSON for at least three workloads.

The first performance milestone is accepted when an optimized incremental path is at least 10x faster than the current event tuple path for at least one realistic string-heavy or object-heavy stream, without changing parser correctness for the existing event API.

The stronger milestone is accepted when `jsonmodem` improves substantially over cumulative-prefix `jiter.from_json(..., partial_mode=True)` and other comparable partial/streaming parsers on the same fragment stream while preserving ergonomic Python usage. Full `.loads()` competitiveness is explicitly not an acceptance criterion.

The byte-range mode is accepted only if tests prove returned views keep the input buffer alive, do not expose invalid memory, and fall back correctly when JSON escaping requires materialization.

Correctness validation must include the existing Python tests plus new tests comparing outputs against `json.loads` on valid JSON fixtures.

## Idempotence and Recovery

Benchmark outputs go under `target/python-perf/` or `tmp/python-perf/` and can be deleted at any time. Optional competitor installs are local to the virtual environment. If a benchmark dependency is unavailable on the host, mark it skipped in the recorded result instead of editing the benchmark to hide it.

Optimization experiments should be small and reversible. If an experiment changes public API names, update this plan's `Decision Log` before continuing.

## Artifacts and Notes

Record benchmark output summaries here as work proceeds. Include:

- command line,
- git commit,
- Python version,
- package versions,
- input file checksums,
- median timings,
- speedup versus current event binding,
- speedup or slowdown versus each competitor,
- allocation or memory peak observations.

2026-06-03 fast-mode benchmark artifacts:

    target/python-perf/medium-baseline.json
    target/python-perf/medium-direct-builder.json
    target/python-perf/medium-no-path-alloc.json
    target/python-perf/response-fast.json
    target/python-perf/realistic-llm-forward-smoke.json
    target/python-perf/realistic-http-extract-smoke.json
    target/python-perf/realistic-llm-forward-pathfilter-smoke.json
    target/python-perf/realistic-http-extract-pathfilter-smoke.json
    target/python-perf/realistic-deep-nested-pathfilter-smoke.json
    target/python-perf/realistic-http-extract-expanded-smoke.json
    target/python-perf/realistic-http-extract-lending-filter-smoke.json
    target/python-perf/realistic-llm-partial-smoke.json
    target/python-perf/realistic-har-extract-smoke.json
    target/python-perf/jiter-chunked-documents-smoke.json
    target/python-perf/jiter-chunked-sequences-smoke.json
    target/python-perf/perf-jsonmodem-events.data
    target/python-perf/perf-jsonmodem-events-after-intern-cache.data

Representative historical full-decode/reference results:

    medium_response.json:
      jsonmodem_events ~= 96 us
      jsonmodem.loads ~= 45.4 us after path-allocation reduction plus lending-event iteration
      stdlib json ~= 24.2 us
      orjson ~= 16.2 us
      msgspec ~= 17.0 us
      jiter ~= 18.5 us

    response_large.json:
      jsonmodem_events ~= 136 us
      jsonmodem.loads ~= 130 us
      stdlib json ~= 99.0 us
      orjson ~= 72.5 us
      msgspec ~= 73.5 us
      jiter ~= 79.7 us

Interpretation: the full-decode harness remains useful reference context, and the native API improves the medium payload by about 2x versus the event tuple path. Full native-object decode is not the current optimization target. The byte-range API achieves a 10x internal target on a string-heavy workload by avoiding Python string payload allocation and per-value Python tuple allocation.

Rejected experiment:

    RawContext native loads:
      medium_response.json jsonmodem.loads ~= 56.3 us
      result: reverted because the standard direct builder stayed faster at ~= 48.3 us.

    Per-load Python string key cache:
      medium_response.json jsonmodem.loads ~= 51.0 us
      response_large.json jsonmodem.loads ~= 135 us
      string_array_unique.json jsonmodem.loads ~= 1.83 ms
      result: reverted because the non-cached direct builder stayed faster on the object fixtures.

Accepted optimization history:

    JsonModemValues conversion path:
      medium_response.json jsonmodem.loads ~= 57.9 us

    Direct Python builder with normal-event path allocation avoided:
      medium_response.json jsonmodem.loads ~= 48.3 us

    Direct Python builder using parser lending events:
      medium_response.json jsonmodem.loads ~= 45.4 us

    Parser-backed string range tuples:
      string_array_unique.json jsonmodem_string_ranges ~= 2.12 ms

    Parser-backed packed range table:
      string_array_unique.json jsonmodem_string_range_table ~= 1.0 ms

    Parser-backed packed range table with container-kind-only backend:
      string_array_unique.json jsonmodem_string_range_table ~= 997 us
      result: removed from final diff because the direct byte scanner was much faster.

    Direct byte-scanner packed range table:
      string_array_unique.json jsonmodem_string_range_table ~= 183 us
      event baseline in same run ~= 9.01 ms
      speedup ~= 49x

    Realistic LLM forwarding before path filter:
      jsonmodem_byteviews ~= 410 us
      jsonmodem_events ~= 396 us
      jsonmodem.loads ~= 111 us
      stdlib json ~= 98.5 us
      orjson ~= 24.2 us
      msgspec ~= 25.6 us
      jiter ~= 29.9 us

    Realistic HTTP nested extraction before path filter:
      jsonmodem_events ~= 7.67 ms
      ijson_items ~= 1.14 ms
      jsonmodem.loads ~= 1.85 ms
      stdlib json ~= 754 us
      orjson ~= 414 us
      msgspec ~= 437 us
      jiter ~= 580 us

    Realistic LLM forwarding after path filter:
      jsonmodem_pathfilter_byteviews ~= 401 us
      jsonmodem_byteviews ~= 399 us
      jsonmodem_events ~= 389 us
      jsonmodem.loads ~= 113 us
      stdlib json ~= 97.6 us
      orjson ~= 24.5 us
      msgspec ~= 24.9 us
      jiter ~= 29.6 us

    Realistic HTTP nested extraction after path filter:
      jsonmodem_events ~= 7.90 ms
      jsonmodem_pathfilter ~= 2.16 ms
      ijson_items ~= 1.15 ms
      jsonmodem.loads ~= 1.89 ms
      stdlib json ~= 746 us
      orjson ~= 418 us
      msgspec ~= 428 us
      jiter ~= 568 us

    Realistic HTTP nested extraction with expanded competitors:
      jsonmodem_events ~= 7.49 ms
      jsonmodem_pathfilter ~= 2.08 ms
      ijson_items ~= 1.12 ms
      json_stream ~= 9.95 ms
      jsonmodem.loads ~= 1.88 ms
      stdlib json ~= 757 us
      orjson ~= 415 us
      msgspec ~= 431 us
      jiter ~= 570 us
      python_rapidjson ~= 833 us
      pysimdjson ~= 599 us
      ujson ~= 574 us

    Realistic HTTP nested extraction after borrowed-path filtering:
      jsonmodem_events ~= 7.55 ms
      jsonmodem_pathfilter ~= 1.82 ms
      ijson_items ~= 1.12 ms
      json_stream ~= 9.89 ms
      jsonmodem.loads ~= 1.91 ms
      stdlib json ~= 751 us
      orjson ~= 408 us
      msgspec ~= 434 us
      jiter ~= 568 us
      python_rapidjson ~= 841 us
      pysimdjson ~= 606 us
      ujson ~= 580 us

    Realistic deep nested target extraction:
      jsonmodem_events ~= 3.24 ms
      jsonmodem_pathfilter ~= 984 us
      jsonmodem.loads ~= 695 us
      stdlib json ~= 325 us
      orjson ~= 198 us
      msgspec ~= 213 us
      jiter ~= 254 us
      python_rapidjson ~= 333 us
      pysimdjson ~= 265 us
      ujson ~= 243 us

    Realistic LLM partial-parser UX:
      jsonmodem_pathfilter_byteviews ~= 413 us
      jsonriver ~= 12.4 ms
      partial_json_parser ~= 509 ms
      json_streamer ~= 75.6 ms
      streaming-json-parser result: skipped because the installed parser module failed to import

    Realistic HAR/API capture request URL extraction:
      jsonmodem_events ~= 13.3 ms
      jsonmodem_pathfilter ~= 2.67 ms
      json_stream ~= 13.7 ms
      jsonmodem.loads ~= 3.07 ms
      stdlib json ~= 1.18 ms
      orjson ~= 580 us
      msgspec ~= 602 us
      jiter ~= 799 us
      python_rapidjson ~= 1.16 ms
      pysimdjson ~= 855 us
      ujson ~= 797 us

    Historical Jiter comparison, documents split into 64-byte chunks with
    reassembled full-document decode:
      medium_response.json:
        reference-only jsonmodem.loads after reassembly ~= 45.1 us
        reference-only jiter.from_json after reassembly ~= 19.1 us
        jsonmodem event parser fed chunks ~= 135 us
      response_large.json:
        reference-only jsonmodem.loads after reassembly ~= 126 us
        reference-only jiter.from_json after reassembly ~= 78.8 us
        jsonmodem event parser fed chunks ~= 369 us

    Jiter comparison, newline-delimited JSON sequences split into 64-byte chunks:
      sequence_medium, 500 objects:
        jsonmodem allow_multiple event stream ~= 6.91 ms
        buffered newline framing plus jiter.from_json per line ~= 859 us
        jiter partial_mode=True on joined sequence ~= 16.7 us, but returns only the first value
      sequence_large, 2000 objects:
        jsonmodem allow_multiple event stream ~= 27.4 ms
        buffered newline framing plus jiter.from_json per line ~= 3.47 ms
        jiter partial_mode=True on joined sequence ~= 61.9 us, but returns only the first value

    Tiny-chunk incremental profile, single object with large content string:
      before interned-tag reuse:
        5,000 chunks x 8 bytes:
          jsonmodem event stream ~= 8.43 ms
          JsonModemPathFilter(byte_views=True) ~= 9.16 ms
          cumulative jiter partial parse per chunk ~= 31.4 ms
        50,000 chunks x 8 bytes:
          jsonmodem event stream ~= 81.9 ms
          JsonModemPathFilter(byte_views=True) ~= 92.1 ms
          cumulative jiter partial parse per chunk ~= 2.93 s
      after interned-tag reuse:
        5,000 chunks x 8 bytes:
          jsonmodem event stream ~= 5.95 ms
          JsonModemPathFilter(byte_views=True) ~= 6.75 ms
        50,000 chunks x 8 bytes:
          jsonmodem event stream ~= 58.9 ms
          JsonModemPathFilter(byte_views=True) ~= 67.7 ms
      after Rust record-buffer pooling, cached payload keys, and active-string clone reduction:
        5,000 chunks x 8 bytes:
          jsonmodem event stream manual timing ~= 5.5 ms per run
        50,000 chunks x 8 bytes:
          jsonmodem event stream manual timing ~= 50.0 ms per run
          JsonModemPathFilter(byte_views=True) manual timing ~= 62.1 ms per run
        cProfile on two 50,000-chunk event runs:
          elapsed ~= 125.6 ms total
          native `JsonModem.feed()` time ~= 58 ms total
      interpretation:
        jsonmodem is already much better than cumulative jiter partial parsing for true tiny-chunk incremental feedback, but current Python event materialization still costs roughly 1.2 us per chunk/event after this small fix.

2026-06-03 allocation summary with `tracemalloc`:

    medium_response.json:
      jsonmodem_events peak ~= 5004 bytes
      jsonmodem.loads peak ~= 6873 bytes
      jsonmodem_string_ranges peak ~= 3316 bytes
      jsonmodem_string_range_table peak ~= 313 bytes

    response_large.json:
      jsonmodem_events peak ~= 48760 bytes
      jsonmodem.loads peak ~= 41792 bytes
      jsonmodem_string_ranges peak ~= 1916 bytes
      jsonmodem_string_range_table peak ~= 345 bytes

    string_array_unique.json:
      jsonmodem_events peak ~= 180346 bytes
      jsonmodem.loads peak ~= 545120 bytes
      jsonmodem_string_ranges peak ~= 1201610 bytes
      jsonmodem_string_range_table peak ~= 80033 bytes

## Interfaces and Dependencies

Expected touched files:

    crates/jsonmodem-py/src/lib.rs
        Add native-value decode and byte-range APIs. Avoid the existing `OwnedEvent` conversion path for native-value benchmarks.

    crates/jsonmodem-py/python/jsonmodem/__init__.py
        Export new Python functions and document their names.

    crates/jsonmodem-py/tests/
        Add correctness tests comparing native values with `json.loads`, plus lifetime tests for byte-range outputs.

    crates/jsonmodem-py/benchmarks/
        Add pyperf benchmark harness, optional dependency list, data loader, and README.

    plans/python-performance/execplan.md
        Keep this plan current after every benchmark and optimization pass.

External Python packages for benchmarks:

    pyperf
    orjson
    msgspec
    jiter
    ijson
    json-stream
    python-rapidjson
    ujson
    pysimdjson
    jsonriver
    partial-json-parser
    streaming-json-parser
    json-streamer

Reference sources to re-check before publication:

    https://github.com/pydantic/jiter
    https://pydantic.dev/docs/validation/dev/concepts/json/
    https://github.com/ijl/orjson
    https://jcristharif.com/msgspec/
    https://github.com/ICRAR/ijson
    https://github.com/daggaz/json-stream
    https://pypi.org/project/pysimdjson/
    https://pypi.org/project/python-rapidjson/
    https://pypi.org/project/ujson/
    https://pypi.org/project/jsonriver/
    https://pypi.org/project/partial-json-parser/
    https://pypi.org/project/streaming-json-parser/
    https://pypi.org/project/json-streamer/
    https://pyperf.readthedocs.io/en/latest/
