#![allow(missing_docs)]

mod partial_json_common;
use std::time::Duration;

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use partial_json_common::{
    make_json_payload, run_parse_partial_json, run_streaming_parser, run_streaming_values_parser,
};
#[cfg(feature = "comparison")]
use partial_json_common::{run_fix_json_parse, run_jiter_partial, run_jiter_partial_owned};

fn bench_partial_json_strategies(c: &mut Criterion) {
    let payload = make_json_payload(10_000);

    let mut group = c.benchmark_group("partial_json_strategies");
    group.measurement_time(Duration::from_secs(10));
    group.warm_up_time(Duration::from_secs(5));

    for &parts in &[100usize, 1_000, 5_000] {
        group.bench_with_input(
            BenchmarkId::new("streaming_parser", parts),
            &parts,
            |b, &p| {
                b.iter(|| {
                    let v = run_streaming_parser(black_box(&payload), p);
                    black_box(v);
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("streaming_values_parser", parts),
            &parts,
            |b, &p| {
                b.iter(|| {
                    let v = run_streaming_values_parser(black_box(&payload), p);
                    black_box(v);
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("parse_partial_json", parts),
            &parts,
            |b, &p| {
                b.iter(|| {
                    let v = run_parse_partial_json(black_box(&payload), p);
                    black_box(v);
                });
            },
        );

        #[cfg(feature = "comparison")]
        group.bench_with_input(
            BenchmarkId::new("fix_json_parse", parts),
            &parts,
            |b, &p| {
                b.iter(|| {
                    let v = run_fix_json_parse(black_box(&payload), p);
                    black_box(v);
                });
            },
        );

        #[cfg(feature = "comparison")]
        group.bench_with_input(BenchmarkId::new("jiter_partial", parts), &parts, |b, &p| {
            b.iter(|| {
                let v = run_jiter_partial(black_box(&payload), p);
                black_box(v);
            });
        });

        #[cfg(feature = "comparison")]
        group.bench_with_input(
            BenchmarkId::new("jiter_partial_owned", parts),
            &parts,
            |b, &p| {
                b.iter(|| {
                    let v = run_jiter_partial_owned(black_box(&payload), p);
                    black_box(v);
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_partial_json_strategies);
criterion_main!(benches);
