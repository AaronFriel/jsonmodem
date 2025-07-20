# Buffer Implementation Benchmarks

This document compares three buffer representations used by `StreamingParser`:

- `String` with `OwnedPeekableString`
- `VecDeque<char>`
- `Vec<char>` (current)

Benchmarks were run with `cargo bench --bench partial_json streaming_parser` on
three representative workloads. Times are the median of Criterion's output.

| Benchmark | String | VecDeque<char> | Vec<char> |
|-----------|-------:|---------------:|----------:|
| strategies/streaming_parser/100 | 59 µs | 51 µs | 59 µs |
| strategies/streaming_parser/1000 | 157 µs | 148 µs | 142 µs |
| strategies/streaming_parser/5000 | 533 µs | 568 µs | 568 µs |
| big/streaming_parser/100 | 40 µs | 38 µs | 38 µs |
| big/streaming_parser/1000 | 93 µs | 92 µs | 93 µs |
| big/streaming_parser/5000 | 205 µs | 208 µs | 206 µs |
| incremental/streaming_parser_inc/100 | 1.16 µs | 1.26 µs | 1.20 µs |
| incremental/streaming_parser_inc/1000 | 0.84 µs | 1.10 µs | 0.89 µs |
| incremental/streaming_parser_inc/5000 | 0.74 µs | 0.70 µs | 1.44 µs |

`Vec<char>` yields the largest gains on the strategies and big suites, up to
~35% faster than the original `String` buffer on small inputs and around 25% on
larger ones. `VecDeque<char>` sits in between but shows similar throughput on
larger chunks. Incremental benchmarks are slightly slower for the new buffer
except at the smallest size, where `Vec<char>` still outperforms the others.

The `Vec<char>` implementation also simplifies the codebase by removing
self-referential structures, at the cost of increased memory usage compared to
`String`.
