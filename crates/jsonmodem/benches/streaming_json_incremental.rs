//! Benchmarks for incremental streaming scenarios.
#![allow(missing_docs)]
mod streaming_json_common;
use std::time::Duration;

use criterion::{BatchSize, BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use jsonmodem::{
    NonScalarValueMode, ParserOptions, StreamingParser, StreamingValuesParser, StringValueMode,
};
#[cfg(feature = "comparison")]
use streaming_json_common::partial_json_fixer;
use streaming_json_common::{make_json_payload, parse_partial_json_port};

#[allow(clippy::too_many_lines)]
fn bench_streaming_json_incremental(c: &mut Criterion) {
    let payload = make_json_payload(10_000);
    let payload_bytes = payload.as_bytes();

    // Split the payload exactly in half. The first half will be considered the
    // already-received portion while the second half will be fed to the
    // strategies in *parts* equally-sized chunks. Only the cost of processing
    // ONE of those chunks is measured.
    let midpoint = payload_bytes.len() / 2;

    let first_half = &payload[..midpoint];
    let second_half = &payload[midpoint..];

    let mut group = c.benchmark_group("streaming_json_incremental");

    for &parts in &[100usize, 1_000, 5_000] {
        // size of one incremental chunk we want to measure
        let chunk_size = second_half.len().div_ceil(parts);
        let incremental_part = &second_half[..chunk_size];

        for &mode in &[
            NonScalarValueMode::None,
            NonScalarValueMode::Roots,
            NonScalarValueMode::All,
        ] {
            let name = format!("streaming_parser_inc_{mode:?}").to_lowercase();
            group.bench_with_input(BenchmarkId::new(name, parts), &parts, |b, &_p| {
                b.iter_batched(
                    || {
                        // setup – not measured
                        let mut parser = StreamingParser::new(ParserOptions {
                            non_scalar_values: mode,
                            ..Default::default()
                        });
                        parser.feed(first_half);
                        // Drain all events produced so far so that the parser
                        // is ready for the next chunk.
                        for _ in &mut parser {}
                        parser
                    },
                    |mut parser| {
                        // measured section – process *one* additional chunk
                        parser.feed(incremental_part);
                        let mut events = 0usize;
                        for _ in &mut parser {
                            events += 1;
                        }
                        black_box(events);
                    },
                    BatchSize::SmallInput,
                );
            });
        }

        group.bench_with_input(
            BenchmarkId::new("streaming_values_parser_inc", parts),
            &parts,
            |b, &_p| {
                b.iter_batched(
                    || {
                        let mut parser = StreamingValuesParser::new(ParserOptions {
                            non_scalar_values: NonScalarValueMode::Roots,
                            string_value_mode: StringValueMode::Values,
                            ..Default::default()
                        });
                        parser.feed(first_half).unwrap();
                        parser
                    },
                    |mut parser| {
                        let _ = parser.feed(incremental_part).unwrap();
                    },
                    BatchSize::SmallInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("parse_partial_json_inc", parts),
            &parts,
            |b, &_p| {
                b.iter_batched(
                    || {
                        // Buffer pre-filled with the first half.
                        let mut buf = String::with_capacity(payload.len());
                        buf.push_str(first_half);
                        buf
                    },
                    |mut buf| {
                        buf.push_str(incremental_part);
                        let _ = parse_partial_json_port::parse_partial_json(Some(&buf));
                    },
                    BatchSize::SmallInput,
                );
            },
        );

        #[cfg(feature = "comparison")]
        group.bench_with_input(
            BenchmarkId::new("fix_json_parse_inc", parts),
            &parts,
            |b, &_p| {
                b.iter_batched(
                    || {
                        let mut buf = String::with_capacity(payload.len());
                        buf.push_str(first_half);
                        buf
                    },
                    |mut buf| {
                        buf.push_str(incremental_part);
                        let _ = partial_json_fixer::fix_json_parse(&buf);
                    },
                    BatchSize::SmallInput,
                );
            },
        );

        #[cfg(feature = "comparison")]
        group.bench_with_input(
            BenchmarkId::new("jiter_partial_inc", parts),
            &parts,
            |b, &_p| {
                b.iter_batched(
                    || {
                        let mut buf = String::with_capacity(payload.len());
                        buf.push_str(first_half);
                        buf
                    },
                    |mut buf| {
                        buf.push_str(incremental_part);
                        let _ = jiter::JsonValue::parse_with_config(
                            buf.as_bytes(),
                            false,
                            jiter::PartialMode::TrailingStrings,
                        )
                        .unwrap();
                    },
                    BatchSize::SmallInput,
                );
            },
        );

        #[cfg(feature = "comparison")]
        group.bench_with_input(
            BenchmarkId::new("jiter_partial_inc_owned", parts),
            &parts,
            |b, &_p| {
                b.iter_batched(
                    || {
                        let mut buf = String::with_capacity(payload.len());
                        buf.push_str(first_half);
                        buf
                    },
                    |mut buf| {
                        buf.push_str(incremental_part);
                        let _ = jiter::JsonValue::parse_with_config(
                            buf.as_bytes(),
                            false,
                            jiter::PartialMode::TrailingStrings,
                        )
                        .unwrap()
                        .into_static();
                    },
                    BatchSize::SmallInput,
                );
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

criterion_group! { name = benches; config = criterion(); targets = bench_streaming_json_incremental }
criterion_main!(benches);
