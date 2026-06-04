# Benchmark Data

The checked-in benchmark inputs under `crates/jsonmodem/benches/jiter_data/`
come from the repository's `origin/perf` branch. The Python benchmark harness
uses a focused subset by default:

- `medium_response.json`: LLM/API-style nested response.
- `response_large.json`: larger nested response.
- `string_array.json`: repeated strings.
- `string_array_unique.json`: unique string-heavy array.
- `floats_array.json`: numeric array.
- `massive_ints_array.json`: integer-heavy array.
- `true_object.json`: object with many boolean values.
- `unicode.json`: non-ASCII string handling.

Large external datasets should be downloaded into `target/python-perf/data/`
with checksums recorded in `plans/python-performance/execplan.md`, not checked
into the repository by default.

`bench_realistic_scenarios.py` generates deterministic synthetic inputs from
source-backed application patterns recorded in
`plans/python-performance/streaming-json-research.md`. The generated scenarios
cover streamed HTTP response extraction, LLM tool-call argument streaming, LLM
partial-parser behavior, NDJSON/log records, deeply nested target-field
extraction, and HAR/API capture request URL extraction.

`bench_jiter_chunked.py` uses the checked-in Jiter fixture documents plus
generated newline-delimited JSON sequences to compare `jsonmodem` directly
against Pydantic's `jiter` when input arrives as small chunks. The default
document benchmarks are an incremental comparison: `jsonmodem` consumes every
fragment through `feed()`, while `jiter` reparses every
cumulative prefix with `partial_mode=True`. Reassembled one-shot decode is
available only through `--group reference` and is not the optimization target.
Sequence benchmarks compare `JsonModem(ParserOptions(allow_multiple=True))`
against newline-framed `jiter.from_json()` for each complete line; jiter's
joined `partial_mode=True` first-value behavior is also reference-only.
