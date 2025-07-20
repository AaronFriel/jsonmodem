# Buffer Implementation Benchmarks

This document compares three buffer representations used by `StreamingParser`:

- `String` with `OwnedPeekableString`
- `VecDeque<char>`
- `Vec<char>` (current)

Benchmarks were run with `cargo bench --bench partial_json streaming_parser` on
three representative workloads. Times are the median of Criterion's output.

| Benchmark | String | VecDeque<char> | Vec<char> |
|-----------|-------:|---------------:|----------:|
| strategies/streaming_parser/100 | 103 µs | 90 µs | 67 µs |
| strategies/streaming_parser/1000 | 287 µs | 238 µs | 216 µs |
| strategies/streaming_parser/5000 | 1.04 ms | 0.83 ms | 0.83 ms |
| big/streaming_parser/100 | 66 µs | 59 µs | 56 µs |
| big/streaming_parser/1000 | 179 µs | 140 µs | 140 µs |
| big/streaming_parser/5000 | 417 µs | 309 µs | 309 µs |
| incremental/streaming_parser_inc/100 | 1.02 µs | 1.07 µs | 0.81 µs |
| incremental/streaming_parser_inc/1000 | 0.55 µs | 0.65 µs | 0.62 µs |
| incremental/streaming_parser_inc/5000 | 0.53 µs | 0.63 µs | 0.60 µs |

`Vec<char>` yields the largest gains on the strategies and big suites, up to
~35% faster than the original `String` buffer on small inputs and around 25% on
larger ones. `VecDeque<char>` sits in between but shows similar throughput on
larger chunks. Incremental benchmarks are slightly slower for the new buffer
except at the smallest size, where `Vec<char>` still outperforms the others.

The `Vec<char>` implementation also simplifies the codebase by removing
self-referential structures, at the cost of increased memory usage compared to
`String`.
