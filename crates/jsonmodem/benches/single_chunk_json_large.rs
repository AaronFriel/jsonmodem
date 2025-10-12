#![expect(missing_docs)]

mod streaming_json_common;

use std::{env, time::Duration};

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use streaming_json_common::{
    run_jiter_value, run_jiter_value_owned, run_jsonmodem_buffers_single,
    run_jsonmodem_events_single, run_jsonmodem_values_single,
};

fn bench_single_chunk_json_large(c: &mut Criterion) {
    let medium_payload = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/benches/jiter_data/medium_response.json"
    ))
    .unwrap();
    let large_payload = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/benches/jiter_data/response_large.json"
    ))
    .unwrap();

    let mut group = c.benchmark_group("single_chunk_json_large");

    for (label, payload) in [
        ("medium_response", medium_payload.as_str()),
        ("response_large", large_payload.as_str()),
    ] {
        group.bench_with_input(
            BenchmarkId::new("jsonmodem_events", label),
            &label,
            |b, _| {
                b.iter(|| {
                    let total = run_jsonmodem_events_single(black_box(payload));
                    black_box(total);
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("jsonmodem_buffers", label),
            &label,
            |b, _| {
                b.iter(|| {
                    let total = run_jsonmodem_buffers_single(black_box(payload));
                    black_box(total);
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("jsonmodem_values", label),
            &label,
            |b, _| {
                b.iter(|| {
                    let total = run_jsonmodem_values_single(black_box(payload));
                    black_box(total);
                });
            },
        );

        if env::var_os("JSONMODEM_BENCH_COMPARISON").is_some() {
            group.bench_with_input(BenchmarkId::new("jiter_value", label), &label, |b, _| {
                b.iter(|| {
                    let total = run_jiter_value(black_box(payload));
                    black_box(total);
                });
            });

            group.bench_with_input(
                BenchmarkId::new("jiter_value_owned", label),
                &label,
                |b, _| {
                    b.iter(|| {
                        let total = run_jiter_value_owned(black_box(payload));
                        black_box(total);
                    });
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

criterion_group! { name = benches; config = criterion(); targets = bench_single_chunk_json_large }
criterion_main!(benches);
