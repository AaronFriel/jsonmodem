#![allow(missing_docs)]

mod streaming_json_common;
use std::time::Duration;

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use jsonmodem::{NonScalarValueMode, produce_chunks};
use streaming_json_common::{
    make_json_payload, run_parse_partial_json, run_streaming_parser, run_streaming_values_parser,
};
#[cfg(feature = "comparison")]
use streaming_json_common::{run_fix_json_parse, run_jiter_partial, run_jiter_partial_owned};

fn bench_streaming_json_strategies(c: &mut Criterion) {
    let payload = make_json_payload(10_000);

    let mut group = c.benchmark_group("streaming_json_strategies");
    group.measurement_time(Duration::from_secs(10));
    group.warm_up_time(Duration::from_secs(5));

    for &parts in &[100usize, 1_000, 5_000] {
        let chunks = produce_chunks(&payload, parts);
        for &mode in &[
            NonScalarValueMode::None,
            NonScalarValueMode::Roots,
            NonScalarValueMode::All,
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

criterion_group!(benches, bench_streaming_json_strategies);
criterion_main!(benches);
