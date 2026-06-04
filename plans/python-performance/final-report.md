# Python Incremental Allocation Report

Date: 2026-06-04

Scope: reduce Python object allocation overhead in the incremental
`jsonmodem` parsing path while keeping the Python library ergonomic. All
competitor comparisons use the fair streaming method: the same JSON fragment
boundaries are processed, and `jiter` reparses every cumulative prefix with
`partial_mode=True`.

## Current API

`JsonModem.feed()` is the single feed API. It accepts either one `str`, `bytes`,
`bytearray`, or contiguous `memoryview` chunk, or an iterable of those chunk
types. Passing an iterable is the documented fast path for many small HTTP body
or LLM tool-call fragments.

Events are exact Python outer tuples for fast immediate unpacking:

```python
for kind, path, payload in parser.feed(chunks):
    if kind == "string" and path.endswith("content"):
        sink.write(payload.fragment)
```

The tuple contents are optimized defaults:

- `path` is a lightweight `PathView` with `endswith(...)`, `as_tuple()`, integer indexing, and range indexing.
- string payloads are lightweight `StringPayload` objects with `.fragment`, `.is_initial`, `.is_final`, and compatibility dictionary-style access.

The earlier experimental APIs `feed_many()`, `JsonEvent`, object-feed methods,
`warm_event_pool()`, `loads()`, `string_ranges()`, and `string_range_table()`
were removed. They proved useful for experiments, but this PR keeps the public
Python surface focused on incremental fragment parsing.

## Results

Full cross-library comparison results from 2026-06-04 are recorded in
`plans/python-performance/comparison-results-20260604.md`. The short version:

- fair fragment-stream documents: `jsonmodem.feed(chunks)` beats cumulative-prefix
  `jiter` by about 5.6x on `medium_response.json` and about 58x on
  `response_large.json`;
- LLM partial content: jsonmodem byte-view path filtering is about 292 us,
  compared with `jsonriver` at about 12.5 ms, `json-streamer` at about 77 ms,
  and `partial-json-parser` at about 522 ms;
- full-document decode: `orjson`, `msgspec`, `pysimdjson`, and one-shot `jiter`
  remain useful context, but `jsonmodem` no longer exposes a public
  full-document decode API;
- complete newline-delimited JSON objects: buffered-line `jiter` and native
  full decoders beat current jsonmodem event streaming;
- selective HTTP/deep/HAR extraction: `JsonModemPathFilter` improves event
  parsing substantially but still loses to full decode plus indexing when
  retaining the whole object is acceptable.

Synthetic stream: one JSON document split into 8-byte fragments, with a large
`content` string. Timings are manual wall-clock runs from
`crates/jsonmodem-py/benchmarks/profile_incremental.py` after release rebuilds.

| Scenario | Result |
| --- | ---: |
| `jiter.from_json(cumulative_prefix, partial_mode=True)`, 5,000 fragments | ~31.0 ms/run |
| `JsonModem.feed(chunk)`, 5,000 fragments, count only | ~4.49 ms/run |
| `JsonModem.feed(chunks)`, 5,000 fragments, count only | ~2.78 ms/run |
| `JsonModem.feed(chunks)`, 5,000 fragments, immediate unpack/access | ~3.81 ms/run |
| `jiter.from_json(cumulative_prefix, partial_mode=True)`, 50,000 fragments | ~3.00 s/run |
| `JsonModem.feed(chunk)`, 50,000 fragments, count only | ~43.0 ms/run |
| `JsonModem.feed(chunks)`, 50,000 fragments, count only | ~29.9 ms/run |
| `JsonModem.feed(chunks)`, 50,000 fragments, immediate unpack/access | ~38.2 ms/run |

Focused fixture benchmark:

```bash
.venv/bin/python crates/jsonmodem-py/benchmarks/bench_jiter_chunked.py \
  --workload response_large.json \
  --group documents \
  --chunk-size 8 \
  --fast \
  --output target/python-perf/jiter-cumulative-prefix-documents-feed-views-fast.json
.venv/bin/python -m pyperf check target/python-perf/jiter-cumulative-prefix-documents-feed-views-fast.json
```

Fast-mode results:

| Benchmark | Mean |
| --- | ---: |
| `jsonmodem_events_chunked:response_large.json` | ~223 us |
| `jsonmodem_feed_chunks_chunked:response_large.json` | ~133 us |
| `jiter_cumulative_partial_prefixes:response_large.json` | ~7.14 ms |

`pyperf check` completed with expected fast-mode instability warnings.

## Conclusions

The best current user-facing optimization is batched `feed(chunks)` with exact
outer tuple events and lazy inner path/payload objects. This keeps the common
unpacking UX while removing the extra public APIs.

The measured object-pool idea was real but not worth shipping as a separate
mode. `JsonEvent` plus pre-warming improved opaque count-only consumption, but
immediate unpacking was slower and the API surface got worse.

The next meaningful optimization should avoid event construction entirely for
common forwarding workloads, for example a target-path sink or compact
byte-range result table. That would be a new capability, not another variant of
`feed()`.

## Validation

- `.agent/check-py.sh` passed after the public API removals: release extension
  build, 20 Python tests, pydoc, and pdoc.
- `cargo fmt --check` passed, with the repository's existing stable-toolchain
  warnings for nightly-only formatting options.
- `cargo check -p jsonmodem-py` passed.
- `PATH="$HOME/.local/bin:$PATH" .agent/check.sh` passed: rustfmt, release
  build, Rust tests, clippy, public docs, cfg-miri clippy, and actionlint.
  Miri was skipped by the script's default `AGENT_CHECK_MIRI_DISABLE=true`.
- `cargo +nightly fuzz run fuzz_jsonmodem -- -runs=5000 -max_total_time=300`
  passed after switching the fuzz crate back to `libfuzzer-sys 0.4.12`.
- `pyperf check target/python-perf/jiter-cumulative-prefix-documents-feed-views-fast.json`
  passed with expected fast-mode warnings.
- `pyperf check` also passed for `target/python-perf/jiter-all-8b-20260604.json`,
  `target/python-perf/full-decode-reference-20260604.json`, and
  `target/python-perf/realistic-all-20260604.json`, all with expected
  fast-mode warnings.

The PR branch has been pushed for Codex review; do not merge until the
ChatGPT/Codex review is approving and GitHub checks are green.
