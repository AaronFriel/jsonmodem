//! Benchmarks for streaming large JSON payloads.
#![expect(missing_docs)]
mod streaming_json_common;
use std::{env, time::Duration};

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use streaming_json_common::{
    produce_chunks, run_fix_json_parse, run_jiter_partial, run_jiter_partial_owned,
    run_jsonmodem_buffers, run_jsonmodem_events, run_jsonmodem_values, run_parse_partial_json,
};

fn bench_streaming_json_large(c: &mut Criterion) {
    let payload = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/benches/jiter_data/response_large.json"
    ))
    .unwrap();

    let mut group = c.benchmark_group("streaming_json_large");

    for &parts in &[100usize, 1_000, 5_000] {
        let chunks = produce_chunks(&payload, parts);
        group.bench_with_input(
            BenchmarkId::new("jsonmodem_events", parts),
            &parts,
            |b, &_p| {
                b.iter(|| run_jsonmodem_events(black_box(&chunks)));
            },
        );

        group.bench_with_input(
            BenchmarkId::new("jsonmodem_buffers", parts),
            &parts,
            |b, &_p| {
                b.iter(|| run_jsonmodem_buffers(black_box(&chunks)));
            },
        );

        group.bench_with_input(
            BenchmarkId::new("jsonmodem_values", parts),
            &parts,
            |b, &_p| {
                b.iter(|| run_jsonmodem_values(black_box(&chunks)));
            },
        );

        if env::var_os("JSONMODEM_BENCH_COMPARISON").is_some() {
            group.bench_with_input(
                BenchmarkId::new("parse_partial_json", parts),
                &parts,
                |b, &_p| {
                    b.iter(|| run_parse_partial_json(black_box(&chunks)));
                },
            );

            group.bench_with_input(
                BenchmarkId::new("fix_json_parse", parts),
                &parts,
                |b, &_p| {
                    b.iter(|| run_fix_json_parse(black_box(&chunks)));
                },
            );

            group.bench_with_input(
                BenchmarkId::new("jiter_partial", parts),
                &parts,
                |b, &_p| {
                    b.iter(|| run_jiter_partial(black_box(&chunks)));
                },
            );

            group.bench_with_input(
                BenchmarkId::new("jiter_partial_owned", parts),
                &parts,
                |b, &_p| {
                    b.iter(|| run_jiter_partial_owned(black_box(&chunks)));
                },
            );
        }
    }

    group.finish();
}

fn criterion() -> Criterion {
    let mut c = Criterion::default();
    if env::var_os("JSONMODEM_BENCH_FAST").is_some() {
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

criterion_group! { name = benches; config = criterion(); targets = bench_streaming_json_large }
criterion_main!(benches);
