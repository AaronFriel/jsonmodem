#![allow(missing_docs)]

mod partial_json_common;
use std::time::Duration;

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use jsonmodem::{produce_chunks, produce_prefixes};
#[cfg(feature = "comparison")]
use partial_json_common::{run_fix_json_parse, run_jiter_partial, run_jiter_partial_owned};
use partial_json_common::{
    run_parse_partial_json, run_streaming_parser, run_streaming_values_parser,
};

fn bench_partial_json_big(c: &mut Criterion) {
    let payload = std::fs::read_to_string("./benches/jiter_data/medium_response.json").unwrap();

    let mut group = c.benchmark_group("partial_json_big");
    group.measurement_time(Duration::from_secs(10));
    group.warm_up_time(Duration::from_secs(5));

    for &parts in &[100usize, 1_000, 5_000] {
        let chunks = produce_chunks(&payload, parts);
        let prefixes = produce_prefixes(&payload, parts);
        group.bench_with_input(
            BenchmarkId::new("streaming_parser", parts),
            &parts,
            |b, &_p| {
                b.iter(|| {
                    let v = run_streaming_parser(black_box(&chunks));
                    black_box(v);
                });
            },
        );

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
                    let v = run_parse_partial_json(black_box(&prefixes));
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
                    let v = run_fix_json_parse(black_box(&prefixes));
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
                    let v = run_jiter_partial(black_box(&prefixes));
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
                    let v = run_jiter_partial_owned(black_box(&prefixes));
                    black_box(v);
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_partial_json_big);
criterion_main!(benches);
