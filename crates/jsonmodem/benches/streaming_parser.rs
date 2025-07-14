#![allow(missing_docs)]
//! Benchmark – `jsonmodem::StreamingParser`

use std::time::Duration;

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use jsonmodem::{ParserOptions, StreamingParser};

/// Produce a *deterministic* JSON document whose textual representation is at
/// least `target_len` bytes (UTF-8 code units). The resulting string is
/// exactly `target_len` bytes long so that each benchmark scenario operates on
/// the same amount of data.
fn make_json_payload(target_len: usize) -> String {
    // We construct the document as a single large string property inside an
    // object.  This guarantees that the resulting JSON is still valid no
    // matter how long the requested payload is.
    //
    // {"data":"aaaa…"}
    let overhead = "{\"data\":\"\"}".len(); // minimal structure
    assert!(target_len >= overhead, "target_len must be >= {overhead}");

    let content_len = target_len - overhead;
    let mut s = String::with_capacity(target_len);
    s.push_str("{\"data\":\"");
    s.extend(std::iter::repeat_n('a', content_len));
    s.push_str("\"}");
    debug_assert_eq!(s.len(), target_len);
    s
}

/// Run the parser by feeding it `parts` chunks that together form the full
/// `payload`.  The function returns the number of `ParseEvent`s that the
/// parser produced so that the result can be black-boxed by Criterion (to
/// prevent the compiler from optimising the 'work' away).
fn run_streaming_parser(payload: &str, parts: usize) -> usize {
    assert!(parts > 0);
    let chunk_size = payload.len().div_ceil(parts); // ceiling division

    let mut parser = StreamingParser::new(ParserOptions::default());
    let mut produced = 0usize;

    for chunk in payload.as_bytes().chunks(chunk_size) {
        parser.feed(std::str::from_utf8(chunk).expect("chunk is valid UTF-8"));
        for _res in parser.by_ref() {
            // drain any immediately-available events
            produced += 1;
        }
    }

    for res in parser.finish() {
        let _ = res.unwrap();
        produced += 1;
    }

    produced
}

fn bench_streaming_parser(c: &mut Criterion) {
    let payload = make_json_payload(10_000);

    let mut group = c.benchmark_group("streaming_parser_split");
    group.measurement_time(Duration::from_secs(10));
    group.warm_up_time(Duration::from_secs(5));

    for &parts in &[100usize, 1_000, 5_000] {
        group.bench_with_input(BenchmarkId::from_parameter(parts), &parts, |b, &p| {
            b.iter(|| {
                let count = run_streaming_parser(black_box(&payload), p);
                black_box(count);
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_streaming_parser);
criterion_main!(benches);
