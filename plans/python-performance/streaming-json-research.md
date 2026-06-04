# Streaming JSON Benchmark Research

This note records source-backed use cases for the realistic Python benchmark
suite. It complements `execplan.md`; it should stay dense and auditable.

2026-06-03 update: the primary benchmark goal is incremental parsing of a
stream of JSON fragments. `jsonmodem` should be compared to `jiter` by feeding
the same fragments and having `jiter` parse every cumulative prefix with
`partial_mode=True`. Full-document competitor decoders are reference-only
context and should not be used as the optimization target; `jsonmodem` does not
expose a public `loads()` API in the current PR.

## Package and API Sources

- `jsoniter`, `json_iterator`, and `json-iterator`: PyPI JSON endpoint checks
  on 2026-06-03 returned no active package for these exact names. `jiter` is
  the active Pydantic-adjacent package that matches the user's likely
  "jsoniter" reference in the Python JSON parser space. Source:
  `https://pypi.org/pypi/<name>/json`
  Benchmark implications: keep the plan language explicit. Compare
  `jsonmodem` to `jiter` and still search downstream code for spelling variants
  when GitHub API access is available.

- `json-stream` 2.5.1: PyPI describes it as a streaming JSON encoder and
  decoder that can stream from files, URLs, or iterators, supports nested data,
  multiple documents, and integrations for `requests`, `httpx`, and `urllib`.
  Source: https://pypi.org/project/json-stream/
  Benchmark implications: HTTP response body streaming, nested object traversal,
  multiple JSON documents in one stream, and iterator-fed chunks are first-class
  scenarios.

- `ijson` 3.5.0: GitHub and PyPI describe it as an iterative JSON parser with
  standard Python iterator interfaces. Source: https://github.com/ICRAR/ijson
  Benchmark implications: event iteration and path-targeted item extraction
  should be compared against `ijson.items()` / `ijson.parse()`, especially for
  large JSON arrays and nested fields.

- `jiter` 0.15.0: the upstream repository describes `Jiter` as an iterator over
  JSON data and `PythonParse` as parsing JSON into a Python object. Pydantic
  documentation says Pydantic v2.5.0 and later uses `jiter` for JSON parsing.
  Sources: https://github.com/pydantic/jiter and
  https://pydantic.dev/docs/validation/dev/concepts/json
  Benchmark implications: the primary document comparison is
  `jiter.from_json(cumulative_prefix, partial_mode=True)` after every incoming
  fragment. Full native decode is reference-only. Treat Pydantic-style
  validation as adjacent work rather than a direct parser-only comparison.

- `orjson` 3.11.9: PyPI describes a fast, correct JSON library and notes that
  `loads()` is roughly 2x as fast as stdlib `json`, while reading from files and
  line-delimited JSON is not provided by the library. Source:
  https://pypi.org/project/orjson/
  Benchmark implications: compare full-object decode for HTTP bodies and NDJSON
  line-by-line decode, but do not label `orjson` as a streaming parser.

- `msgspec` 0.21.1: project docs describe a fast serialization and validation
  library with built-in JSON support. Source: https://jcristharif.com/msgspec/
  Benchmark implications: compare native decode and typed decode separately if
  typed schemas are added later.

- `pysimdjson` 7.0.2: PyPI describes Python bindings for the SIMD-accelerated
  `simdjson` parser and notes that a fallback parser is used when SIMD
  instructions are unavailable. Source: https://pypi.org/project/pysimdjson/
  Benchmark implications: include full native decode through
  `simdjson.Parser().parse(data, recursive=True)` when it installs cleanly.

- `python-rapidjson` 1.23: PyPI describes a Python wrapper around RapidJSON
  exposing JSON serialization and deserialization for `bytes`, `str`, or
  file-like instances. Source: https://pypi.org/project/python-rapidjson/
  Benchmark implications: include native decode and consider a file-like
  streaming comparison later if jsonmodem adds a file/iterator helper.

- `ujson` 5.12.1: PyPI describes UltraJSON as an ultra-fast JSON encoder and
  decoder, but the project page says it is maintenance-only and encourages
  migration to `orjson`. Source: https://pypi.org/project/ujson/
  Benchmark implications: include native decode as a legacy high-performance
  baseline, but do not model new jsonmodem API design on `ujson`.

- `jsonriver` 1.0.0: PyPI describes an async iterator that yields progressively
  complete values from string chunks. Source: https://pypi.org/project/jsonriver/
  Benchmark implications: include LLM-style progressive JSON chunks and compare
  usability, not only speed, because jsonriver exposes partial values rather
  than low-level parser events.

- `json-streamer` 0.1.0: PyPI describes parsing incomplete JSON strings from
  streams/generators. Source: https://pypi.org/project/json-streamer/
  Benchmark implications: include incomplete or progressively arriving JSON
  where callers want early events.

- `partial-json-parser`: PyPI describes parsing partial JSON. Source:
  https://pypi.org/project/partial-json-parser/
  Benchmark implications: include LLM structured-output chunks that are not a
  complete JSON document until the final chunk.

- `streaming-json-parser`: PyPI describes incremental parsing of streaming JSON.
  Source: https://pypi.org/project/streaming-json-parser/
  Benchmark implications: include LLM chunk workloads and malformed or partial
  intermediate states.

  Local benchmark note: version `0.1.0` installed, but importing
  `streaming_json_parser.streaming_json_parser` failed because the package tries
  to import `src.streaming_json_parser.iterative_state_machine`. It is recorded
  as a researched package but skipped from executable benchmarks until the
  package import path is fixed or a supported import is documented.

## GitHub Usage Signals

GitHub code search was rate-limited after several batches on 2026-06-03. The
queries below returned concrete public examples before the limit was reached:

- HTTP response parsing with `ijson`: hits included `apache/libcloud`
  `contrib/scrape-ec2-sizes.py`, `puppetlabs/puppetdb` `util/pdb/puppetdb.py`,
  `internetarchive/openlibrary` `scripts/promise_batch_imports.py`, and
  `opencybersecurityalliance/firepit` `firepit/raft.py`.
  Benchmark implications: streamed HTTP response body plus path-targeted
  extraction from arrays/objects.

- `json_stream.load()` in application code: hits included `cvat-ai/cvat`
  `cvat/apps/quality_control/quality_reports.py`, `alufers/mitmproxy2swagger`
  `mitmproxy2swagger/har_capture_reader.py`, and `SQLMesh/sqlmesh`
  `sqlmesh/core/state_sync/export_import.py`.
  Benchmark implications: HAR/API response data, import/export state, and
  report generation are realistic nested-data workloads.

- LLM/tool-call streaming: hits included `zylon-ai/private-gpt`
  `private_gpt/components/llm/utils.py`, `PrefectHQ/marvin`
  `src/marvin/engine/events.py`, `sgl-project/sglang`
  `python/sglang/srt/function_call/...`, and `vllm-project/vllm`
  `vllm/tool_parsers/utils.py`.
  Benchmark implications: stream chunks of tool-call arguments, detect nested
  paths early, and forward large string values without materializing them.

## Downstream Packages and Use Sites

This section records specific public URLs found by package metadata checks and
GitHub code search. These are evidence for benchmark scenarios, not claims that
the projects endorse `jsonmodem`.

- Exact `jsoniter` spelling checks: `https://pypi.org/pypi/jsoniter/json`,
  `https://pypi.org/pypi/json_iterator/json`,
  `https://pypi.org/pypi/json-iterator/json`,
  `https://pypi.org/pypi/json-iter/json`, and
  `https://pypi.org/pypi/jsoniterator/json` all returned HTTP 404 on
  2026-06-03.
  Workload implication: benchmark `jiter` as the active Python competitor and
  keep the misspelled names out of package requirements.

- `ijson` large HTTP/array extraction:
  `internetarchive/openlibrary` `scripts/promise_batch_imports.py`
  https://github.com/internetarchive/openlibrary/blob/4aa2b73e68571162fa93edaa9593ca78d6185cf5/scripts/promise_batch_imports.py
  and `apache/libcloud` `contrib/scrape-ec2-prices.py`
  https://github.com/apache/libcloud/blob/b3cca53cfad8f115fb5f17b67e713447bb9b9dc4/contrib/scrape-ec2-prices.py
  both support the HTTP response plus nested item extraction scenario.

- `ijson` API client streaming:
  `puppetlabs/puppetdb` `util/pdb/puppetdb.py`
  https://github.com/puppetlabs/puppetdb/blob/2c2efc97da49502083c0aa8269f9d1ddd9cb9541/util/pdb/puppetdb.py
  and `rbw/pysnow` `pysnow/response.py`
  https://github.com/rbw/pysnow/blob/6ac140aab631ef7029b8f211b15ebd3afcbb151e/pysnow/response.py
  support streaming API response traversal without retaining a whole result
  object.

- `json-stream` HAR/API capture:
  `alufers/mitmproxy2swagger` `mitmproxy2swagger/har_capture_reader.py`
  https://github.com/alufers/mitmproxy2swagger/blob/1f32eae47e1ff501e0409dceba8776e203bf6c76/mitmproxy2swagger/har_capture_reader.py
  supports the generated HAR capture benchmark that extracts request URLs from
  `log.entries`.

- `json-stream` import/export and reports:
  `SQLMesh/sqlmesh` `sqlmesh/core/state_sync/export_import.py`
  https://github.com/SQLMesh/sqlmesh/blob/7c31c5cada234da9553b73ce0e9e01092dc96ebf/sqlmesh/core/state_sync/export_import.py
  and `cvat-ai/cvat` `cvat/apps/quality_control/quality_reports.py`
  https://github.com/cvat-ai/cvat/blob/93ce5c8617dd2d6e0ca08be8f7099ea38c11b3a7/cvat/apps/quality_control/quality_reports.py
  support nested report/import-export traversal.

- `jiter` direct usage and Pydantic-adjacent validation:
  `pydantic/jiter` `crates/jiter-python/bench.py`
  https://github.com/pydantic/jiter/blob/0bb22aa6b3a4d729e6c7bae74c05a5d0f1f654b0/crates/jiter-python/bench.py
  and Pydantic's JSON validation docs
  https://pydantic.dev/docs/validation/dev/concepts/json/
  support full native decode and future typed validation benchmarks.

- `orjson` full native decode:
  `elastic/elastic-serverless-forwarder` `share/json.py`
  https://github.com/elastic/elastic-serverless-forwarder/blob/6a2e2046f3621bceee08abb63be30ff747d36d63/share/json.py
  and `vulnersCom/api` `vulners/base.py`
  https://github.com/vulnersCom/api/blob/3fa161799a6005a06ffbc6feba39e9e523f5001b/vulners/base.py
  support full-body response decode comparisons where streaming is not the
  primary user need.

- `msgspec` API, IPC, and package metadata:
  `apache/airflow` `task-sdk/src/airflow/sdk/api/client.py`
  https://github.com/apache/airflow/blob/c767af5a47b521bb7689913925e5cc07ceb926da/task-sdk/src/airflow/sdk/api/client.py
  and `jcrist/msgspec` `examples/conda-repodata/query_repodata.py`
  https://github.com/jcrist/msgspec/blob/c4a719560c2404b01751e2884da4c0f953f3f638/examples/conda-repodata/query_repodata.py
  support native decode and typed/package-metadata extraction benchmarks.

- Pydantic `model_validate_json`:
  GitHub code search returned examples such as `dreadnode/parley`
  https://github.com/dreadnode/parley/blob/247a227da60452a7f7282b8f4e18e8a97a1e66dd/parley.py
  and `dottxt-ai/cursed`
  https://github.com/dottxt-ai/cursed/blob/da77409b430100150952dd2f8194e671d34a27bb/scp/api.py
  for JSON-to-model validation. This is adjacent to parser benchmarking and
  should be a separate typed-validation group if added.

- LLM partial/tool-call parsing:
  `PrefectHQ/marvin` `src/marvin/engine/events.py`
  https://github.com/PrefectHQ/marvin/blob/7c3e20a580a4c04cc52170157b1ab9549332bd4a/src/marvin/engine/events.py
  and `vllm-project/vllm` `vllm/tool_parsers/utils.py`
  https://github.com/vllm-project/vllm/blob/e0081ef8cf0e1e36b4363137de430a73979bc1ab/vllm/tool_parsers/utils.py
  support the LLM tool-call chunk benchmark.

- LLM streaming SDK paths:
  `stanfordnlp/dspy` `dspy/streaming/streaming_listener.py`
  https://github.com/stanfordnlp/dspy/blob/4a05ace642dee8bca7340b798e753b5b231e9b3a/dspy/streaming/streaming_listener.py
  and `anthropics/anthropic-sdk-python`
  `src/anthropic/lib/streaming/_types.py`
  https://github.com/anthropics/anthropic-sdk-python/blob/ddd43b7ceb79f433dfafe488d95f48c22801186b/src/anthropic/lib/streaming/_types.py
  support the developer-UX requirement for typed streaming request/response
  bodies and tool-call payloads.

## Benchmark Scenarios to Encode

1. HTTP response body, incremental fragments:
   A medium or large JSON response body arrives as bytes. Feed the same
   fragments to `JsonModem.feed()` and compare to
   `jiter.from_json(cumulative_prefix, partial_mode=True)` after each fragment.
   Full decode with stdlib `json`, `orjson`, `msgspec`, `jiter`, and
   `jsonmodem.loads()` is reference-only.

2. HTTP response body, nested extraction:
   A large response contains many items, and the caller needs one nested string
   or numeric field per item. Compare full decode plus Python indexing against
   `jsonmodem` events and `ijson.items()`.

3. Request body streaming:
   A FastAPI/Starlette-style request stream yields bytes. Feed chunks directly
   to `JsonModem` and `JsonModemByteViews`.

4. LLM tool-call JSON chunks:
   A tool-call argument object arrives in small chunks. The benchmark should
   count when paths become available and forward a large string field to a sink.

5. Byte-view forwarding:
   Caller does not inspect string contents, only forwards them. Use
   `JsonModemByteViews` memoryview fragments and compare against owned Python
   `str` fragments.

6. NDJSON/log stream:
   Many small objects arrive one per line. Compare line-by-line full decode
   against `JsonModem` with `ParserOptions(allow_multiple=True)`.

7. Deep nested JSON:
   The target field is deeply nested in repeated objects. This should stress
   path construction and expose whether path pooling or a path filter API is
   needed.

8. Escaped-string fallback:
   Include strings with JSON escapes to measure the cost and API behavior when
   no-copy memoryview output is impossible.

9. LLM partial parser UX:
   Compare jsonmodem byte-view forwarding with `jsonriver`,
   `partial-json-parser`, and `json-streamer` on the same chunked tool-call
   argument payload. Treat `streaming-json-parser` as researched but skipped
   while its installed parser module fails to import.

10. HAR/API capture extraction:
   A HAR-like capture contains many request/response entries, and the caller
   needs request URLs for import or conversion. Compare jsonmodem event
   scanning, `JsonModemPathFilter`, `json-stream`, and full decode plus Python
   indexing.
