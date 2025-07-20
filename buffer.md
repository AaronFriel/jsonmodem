# Buffer Implementation Benchmarks

This document compares three buffer representations used by `StreamingParser`:

- `String` with `OwnedPeekableString`
- `VecDeque<char>` (current)
- `Vec<char>`

Benchmarks were run with `cargo bench --bench partial_json_strategies --bench partial_json_big --bench partial_json_incremental --bench streaming_parser` on
three representative workloads. Times are the median of Criterion's output.

| Benchmark | String | VecDeque<char> | Vec<char> |
|-----------|-------:|---------------:|----------:|
| strategies/streaming_parser/100 | 59 µs | 60 µs | 59 µs |
| strategies/streaming_parser/1000 | 157 µs | 221 µs | 142 µs |
| strategies/streaming_parser/5000 | 533 µs | 835 µs | 568 µs |
| big/streaming_parser/100 | 40 µs | 53 µs | 38 µs |
| big/streaming_parser/1000 | 93 µs | 133 µs | 93 µs |
| big/streaming_parser/5000 | 205 µs | 307 µs | 206 µs |
| incremental/streaming_parser_inc/100 | 1.16 µs | 0.93 µs | 1.20 µs |
| incremental/streaming_parser_inc/1000 | 0.84 µs | 0.63 µs | 0.89 µs |
| incremental/streaming_parser_inc/5000 | 0.74 µs | 0.60 µs | 1.44 µs |

Historically, the parser experimented with both `String` and `Vec<char>`
backends. The `Vec<char>` approach gave the best throughput on large inputs,
while `String` was sometimes faster for small streams. The current
`VecDeque<char>` representation offers a reasonable trade‑off between memory
usage and performance. The older buffer implementations have been removed, but
their results are kept for reference above.
