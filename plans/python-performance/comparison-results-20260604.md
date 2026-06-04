# Python JSON Library Comparison Results

Date: 2026-06-04

These results compare `jsonmodem` against Python JSON libraries in three
separate categories. Do not collapse these into one ranking: each category
answers a different user question.

All timings below are `pyperf --fast` means from this worktree. `pyperf check`
passed for all three result files with expected fast-mode stability warnings.

Update: later PR cleanup removed the experimental public `jsonmodem.loads()`,
`jsonmodem.string_ranges()`, and `jsonmodem.string_range_table()` helpers. Rows
below with those names are preserved as historical measurements only; active
benchmarks no longer import or publish those helpers.

Artifacts:

- `target/python-perf/jiter-all-8b-20260604.json`
- `target/python-perf/full-decode-reference-20260604.json`
- `target/python-perf/realistic-all-20260604.json`

## Fair Fragment-Stream Comparison

This is the headline comparison for incremental document parsing. `jsonmodem`
consumes every incoming 8-byte fragment through its incremental API. `jiter`
reparses every cumulative prefix with `partial_mode=True`.

| Benchmark | Mean |
| --- | ---: |
| `jsonmodem_events_chunked:medium_response.json` | 85.7 us |
| `jsonmodem_feed_chunks_chunked:medium_response.json` | 68.6 us |
| `jiter_cumulative_partial_prefixes:medium_response.json` | 383.5 us |
| `jsonmodem_events_chunked:response_large.json` | 233.2 us |
| `jsonmodem_feed_chunks_chunked:response_large.json` | 127.2 us |
| `jiter_cumulative_partial_prefixes:response_large.json` | 7.39 ms |

Result: `jsonmodem.feed(chunks)` is about 5.6x faster than cumulative-prefix
`jiter` on `medium_response.json`, and about 58x faster on
`response_large.json`.

## JSON Sequence Comparison

This compares newline-delimited complete JSON objects. `jsonmodem` parses the
stream with `allow_multiple=True`; `jiter` buffers complete lines and parses
each line when it is complete. This does not ask `jiter` for partial progress
inside an incomplete object.

| Benchmark | Mean |
| --- | ---: |
| `jsonmodem_sequence_chunked:sequence_medium` | 4.35 ms |
| `jsonmodem_sequence_feed_chunks:sequence_medium` | 3.92 ms |
| `jiter_sequence_buffered_lines:sequence_medium` | 877.6 us |
| `jsonmodem_sequence_chunked:sequence_large` | 17.52 ms |
| `jsonmodem_sequence_feed_chunks:sequence_large` | 15.91 ms |
| `jiter_sequence_buffered_lines:sequence_large` | 3.52 ms |

Result: for complete newline-delimited objects, buffered-line `jiter` wins by
roughly 4.5x. This is not the same workload as partial document progress after
each fragment.

## Full-Document Reference Decoders

These are reference-only for the current jsonmodem goal. They answer: "If the
whole document is already available and we want native Python objects, how do
libraries compare?"

| Benchmark | Mean |
| --- | ---: |
| `jsonmodem_events:medium_response.json` | 67.7 us |
| `stdlib_json:medium_response.json` | 23.5 us |
| `jsonmodem_loads:medium_response.json` | 45.1 us |
| `orjson:medium_response.json` | 15.8 us |
| `msgspec:medium_response.json` | 16.6 us |
| `jiter:medium_response.json` | 18.1 us |
| `python_rapidjson:medium_response.json` | 22.1 us |
| `pysimdjson:medium_response.json` | 17.9 us |
| `ujson:medium_response.json` | 18.8 us |
| `jsonmodem_string_ranges:medium_response.json` | 27.4 us |
| `jsonmodem_string_range_table:medium_response.json` | 3.8 us |
| `jsonmodem_events:response_large.json` | 98.4 us |
| `stdlib_json:response_large.json` | 97.5 us |
| `jsonmodem_loads:response_large.json` | 126.4 us |
| `orjson:response_large.json` | 71.6 us |
| `msgspec:response_large.json` | 72.8 us |
| `jiter:response_large.json` | 78.7 us |
| `python_rapidjson:response_large.json` | 84.3 us |
| `pysimdjson:response_large.json` | 71.7 us |
| `ujson:response_large.json` | 78.3 us |
| `jsonmodem_string_ranges:response_large.json` | 52.3 us |
| `jsonmodem_string_range_table:response_large.json` | 12.6 us |
| `jsonmodem_events:string_array_unique.json` | 5.38 ms |
| `stdlib_json:string_array_unique.json` | 682.9 us |
| `jsonmodem_loads:string_array_unique.json` | 1.83 ms |
| `orjson:string_array_unique.json` | 624.9 us |
| `msgspec:string_array_unique.json` | 599.9 us |
| `jiter:string_array_unique.json` | 797.9 us |
| `python_rapidjson:string_array_unique.json` | 821.6 us |
| `pysimdjson:string_array_unique.json` | 674.6 us |
| `ujson:string_array_unique.json` | 658.6 us |
| `jsonmodem_string_ranges:string_array_unique.json` | 2.07 ms |
| `jsonmodem_string_range_table:string_array_unique.json` | 176.7 us |

Result: full-object decode is dominated by `orjson`, `msgspec`, `pysimdjson`,
and one-shot `jiter`. Historical `jsonmodem.loads()` measurements are not the
feature to optimize. The historical packed string range table experiment showed
that returning compact byte-offset metadata is much faster than building Python
objects, but that helper is no longer public API.

## Realistic Application Scenarios

These scenarios model common application behaviors: selective HTTP extraction,
LLM content forwarding, LLM partial JSON chunks, NDJSON line parsing, deep
nested extraction, and HAR request URL extraction.

| Benchmark | Mean |
| --- | ---: |
| `jsonmodem_events:http_nested_extract` | 4.61 ms |
| `jsonmodem_pathfilter:http_nested_extract` | 1.59 ms |
| `ijson_items:http_nested_extract` | 1.15 ms |
| `json_stream:http_nested_extract` | 9.92 ms |
| `stdlib_json:http_nested_extract` | 762.6 us |
| `jsonmodem_loads:http_nested_extract` | 1.92 ms |
| `orjson:http_nested_extract` | 441.2 us |
| `msgspec:http_nested_extract` | 448.1 us |
| `jiter:http_nested_extract` | 620.7 us |
| `python_rapidjson:http_nested_extract` | 849.5 us |
| `pysimdjson:http_nested_extract` | 626.3 us |
| `ujson:http_nested_extract` | 603.4 us |
| `jsonmodem_pathfilter_byteviews:llm_forward_content` | 301.2 us |
| `jsonmodem_byteviews:llm_forward_content` | 301.7 us |
| `jsonmodem_events:llm_forward_content` | 293.8 us |
| `stdlib_json:llm_forward_content` | 98.5 us |
| `jsonmodem_loads:llm_forward_content` | 117.2 us |
| `orjson:llm_forward_content` | 25.3 us |
| `msgspec:llm_forward_content` | 25.3 us |
| `jiter:llm_forward_content` | 29.3 us |
| `python_rapidjson:llm_forward_content` | 41.9 us |
| `pysimdjson:llm_forward_content` | 21.5 us |
| `ujson:llm_forward_content` | 50.7 us |
| `jsonmodem_pathfilter_byteviews:llm_partial_content` | 291.9 us |
| `jsonriver:llm_partial_content` | 12.49 ms |
| `partial_json_parser:llm_partial_content` | 522.05 ms |
| `json_streamer:llm_partial_content` | 77.05 ms |
| `jsonmodem_events:ndjson_warning_count` | 11.69 ms |
| `stdlib_json:ndjson_warning_count` | 4.81 ms |
| `jsonmodem_loads:ndjson_warning_count` | 5.26 ms |
| `orjson:ndjson_warning_count` | 1.19 ms |
| `msgspec:ndjson_warning_count` | 1.07 ms |
| `jiter:ndjson_warning_count` | 1.47 ms |
| `python_rapidjson:ndjson_warning_count` | 2.43 ms |
| `pysimdjson:ndjson_warning_count` | 1.63 ms |
| `ujson:ndjson_warning_count` | 1.61 ms |
| `jsonmodem_events:deep_nested_target` | 2.87 ms |
| `jsonmodem_pathfilter:deep_nested_target` | 633.1 us |
| `stdlib_json:deep_nested_target` | 336.5 us |
| `jsonmodem_loads:deep_nested_target` | 733.2 us |
| `orjson:deep_nested_target` | 218.4 us |
| `msgspec:deep_nested_target` | 230.9 us |
| `jiter:deep_nested_target` | 279.7 us |
| `python_rapidjson:deep_nested_target` | 354.3 us |
| `pysimdjson:deep_nested_target` | 284.3 us |
| `ujson:deep_nested_target` | 263.8 us |
| `jsonmodem_events:har_request_urls` | 7.81 ms |
| `jsonmodem_pathfilter:har_request_urls` | 2.35 ms |
| `json_stream:har_request_urls` | 13.67 ms |
| `stdlib_json:har_request_urls` | 1.19 ms |
| `jsonmodem_loads:har_request_urls` | 3.30 ms |
| `orjson:har_request_urls` | 612.2 us |
| `msgspec:har_request_urls` | 617.0 us |
| `jiter:har_request_urls` | 828.5 us |
| `python_rapidjson:har_request_urls` | 1.20 ms |
| `pysimdjson:har_request_urls` | 907.0 us |
| `ujson:har_request_urls` | 830.5 us |

## Conclusions

- `jsonmodem` clearly wins the fair partial-document fragment-stream
  comparison against cumulative-prefix `jiter`.
- `jsonmodem` clearly wins the LLM partial-parser comparison against
  `jsonriver`, `partial-json-parser`, and `json-streamer`.
- `jsonmodem` currently loses NDJSON line parsing and complete-line JSON
  sequence parsing to full decoders / line-buffered `jiter`.
- `jsonmodem` path filtering improves selective extraction substantially, but
  current HTTP/deep/HAR extraction still loses to full decode plus indexing
  when retaining the full object is acceptable.
- The fastest jsonmodem path is byte-range metadata, not event emission. The
  packed range table results show that the next optimization should avoid
  Python event construction for targeted forwarding/extraction workloads.
