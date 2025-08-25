//! Benchmarks for streaming large JSON payloads.
#![expect(missing_docs)]
mod streaming_json_common;
use std::time::Duration;

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use jsonmodem::{NonScalarMode, produce_chunks};
#[cfg(feature = "comparison")]
use streaming_json_common::{run_fix_json_parse, run_jiter_partial, run_jiter_partial_owned};
use streaming_json_common::{
    run_parse_partial_json, run_streaming_parser, run_streaming_values_parser,
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
        for &mode in &[
            NonScalarMode::None,
            NonScalarMode::Roots,
            NonScalarMode::All,
        ] {
            let name = format!("streaming_parser_{mode:?}").to_lowercase();
            group.bench_with_input(BenchmarkId::new(name, parts), &parts, |b, &_p| {
                b.iter(|| {
                    let v = run_streaming_parser(black_box(&chunks), mode);
                    black_box(v);
                });
            });
        }

        group.bench_with_input(
            BenchmarkId::new("streaming_values_parser", parts),
            &parts,
            |b, &_p| {
                b.iter(|| {
                    let v = run_streaming_values_parser(black_box(&chunks));
                    black_box(v);
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("parse_partial_json", parts),
            &parts,
            |b, &_p| {
                b.iter(|| {
                    let v = run_parse_partial_json(black_box(&chunks));
                    black_box(v);
                });
            },
        );

        #[cfg(feature = "comparison")]
        group.bench_with_input(
            BenchmarkId::new("fix_json_parse", parts),
            &parts,
            |b, &_p| {
                b.iter(|| {
                    let v = run_fix_json_parse(black_box(&chunks));
                    black_box(v);
                });
            },
        );

        #[cfg(feature = "comparison")]
        group.bench_with_input(
            BenchmarkId::new("jiter_partial", parts),
            &parts,
            |b, &_p| {
                b.iter(|| {
                    let v = run_jiter_partial(black_box(&chunks));
                    black_box(v);
                });
            },
        );

        #[cfg(feature = "comparison")]
        group.bench_with_input(
            BenchmarkId::new("jiter_partial_owned", parts),
            &parts,
            |b, &_p| {
                b.iter(|| {
                    let v = run_jiter_partial_owned(black_box(&chunks));
                    black_box(v);
                });
            },
        );
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

criterion_group! { name = benches; config = criterion(); targets = bench_streaming_json_large }
criterion_main!(benches);
