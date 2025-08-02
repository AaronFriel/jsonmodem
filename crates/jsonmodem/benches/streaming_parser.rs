//! Benchmark – `jsonmodem::StreamingParser`
#![allow(missing_docs)]

use std::time::Duration;

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use jsonmodem::{NonScalarValueMode, ParserOptions, StreamingParser};

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
fn run_streaming_parser(payload: &str, parts: usize, mode: NonScalarValueMode) -> usize {
    assert!(parts > 0);
    let chunk_size = payload.len().div_ceil(parts); // ceiling division

    let mut parser = StreamingParser::new(ParserOptions {
        non_scalar_values: mode,
        ..Default::default()
    });
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

    for &parts in &[100usize, 1_000, 5_000] {
        for &mode in &[
            NonScalarValueMode::None,
            NonScalarValueMode::Roots,
            NonScalarValueMode::All,
        ] {
            let name = format!("{mode:?}").to_lowercase();
            group.bench_with_input(BenchmarkId::new(parts.to_string(), name), &mode, |b, &m| {
                b.iter(|| {
                    let count = run_streaming_parser(black_box(&payload), parts, m);
                    black_box(count);
                });
            });
        }
    }
    group.finish();
}

fn criterion() -> Criterion {
    let mut c = Criterion::default();
    if cfg!(feature = "bench-fast") {
        c = c
            .warm_up_time(Duration::from_millis(10))
            .measurement_time(Duration::from_millis(100))
            .sample_size(10);
    } else {
        c = c
            .warm_up_time(Duration::from_secs(5))
            .measurement_time(Duration::from_secs(10));
    }
    c
}

criterion_group! { name = benches; config = criterion(); targets = bench_streaming_parser }
criterion_main!(benches);
