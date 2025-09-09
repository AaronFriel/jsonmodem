//! Benchmark – `jsonmodem::StreamingParser`
#![expect(missing_docs)]

use std::{env, time::Duration};

mod streaming_json_common;

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use streaming_json_common::{
    produce_chunks, run_jsonmodem_buffers, run_jsonmodem_events, run_jsonmodem_values,
};

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
    #[cfg(any(fuzzing, debug_assertions))]
    assert_eq!(s.len(), target_len);
    s
}

fn bench_streaming_parser(c: &mut Criterion) {
    let payload = make_json_payload(10_000);

    let mut group = c.benchmark_group("streaming_parser_split");

    for &parts in &[100usize, 1_000, 5_000] {
        let chunks = produce_chunks(&payload, parts);

        group.bench_with_input(
            BenchmarkId::new(parts.to_string(), "jsonmodem_events"),
            &parts,
            |b, &_p| {
                b.iter(|| run_jsonmodem_events(black_box(&chunks)));
            },
        );

        group.bench_with_input(
            BenchmarkId::new(parts.to_string(), "jsonmodem_buffers"),
            &parts,
            |b, &_p| {
                b.iter(|| run_jsonmodem_buffers(black_box(&chunks)));
            },
        );

        group.bench_with_input(
            BenchmarkId::new(parts.to_string(), "jsonmodem_values"),
            &parts,
            |b, &_p| {
                b.iter(|| run_jsonmodem_values(black_box(&chunks)));
            },
        );
    }
    group.finish();
}

fn fast_mode() -> bool {
    env::var_os("JSONMODEM_BENCH_FAST").is_some()
}

fn criterion() -> Criterion {
    let mut c = Criterion::default();
    if fast_mode() {
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
