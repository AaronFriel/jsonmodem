//! Benchmarks for incremental streaming scenarios.
#![expect(missing_docs)]
mod streaming_json_common;
use std::{env, time::Duration};

use criterion::{BatchSize, BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use jsonmodem::{
    BufferOptions, JsonModem, JsonModemBuffers, JsonModemValues, ParserOptions, StdBackend,
    lending_iterator::LendingIterator,
};
use streaming_json_common::{make_json_payload, parse_partial_json_port, partial_json_fixer};

#[expect(clippy::too_many_lines)]
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

        group.bench_with_input(
            BenchmarkId::new("jsonmodem_events_inc", parts),
            &parts,
            |b, &_p| {
                b.iter_batched(
                    || {
                        let mut parser = JsonModem::<StdBackend>::new(ParserOptions::default());
                        let mut iter = parser.feed(first_half);
                        while let Some(event) = iter.next() {
                            let _ = black_box(event);
                        }
                        drop(iter);
                        parser
                    },
                    |mut parser| {
                        let mut produced = 0usize;
                        let incremental_part = black_box(incremental_part);
                        let mut iter = parser.feed(incremental_part);
                        while let Some(event) = iter.next() {
                            let _ = black_box(event);
                            produced += 1;
                        }
                        produced
                    },
                    BatchSize::SmallInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("jsonmodem_buffers_inc", parts),
            &parts,
            |b, &_p| {
                b.iter_batched(
                    || {
                        let mut parser = JsonModemBuffers::new(
                            ParserOptions::default(),
                            BufferOptions::default(),
                        );
                        let mut iter = parser.feed(first_half);
                        while let Some(event) = iter.next() {
                            let _ = black_box(event);
                        }
                        drop(iter);
                        parser
                    },
                    |mut parser| {
                        let mut produced = 0usize;
                        let incremental_part = black_box(incremental_part);
                        let mut iter = parser.feed(incremental_part);
                        while let Some(event) = iter.next() {
                            let _ = black_box(event);
                            produced += 1;
                        }
                        produced
                    },
                    BatchSize::SmallInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("jsonmodem_values_inc", parts),
            &parts,
            |b, &_p| {
                b.iter_batched(
                    || {
                        let mut parser = JsonModemValues::new(ParserOptions::default());
                        let mut iter = parser.feed(first_half);
                        while let Some(value) = LendingIterator::next(&mut iter) {
                            value.unwrap();
                        }
                        drop(iter);
                        parser
                    },
                    |mut parser| {
                        let mut produced = 0usize;
                        let incremental_part = black_box(incremental_part);
                        let mut iter = parser.feed(incremental_part);
                        while let Some(event) = LendingIterator::next(&mut iter) {
                            let _ = black_box(event);
                            produced += 1;
                        }
                        produced
                    },
                    BatchSize::SmallInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("streaming_values_parser_inc", parts),
            &parts,
            |b, &_p| {
                b.iter_batched(
                    || {
                        let mut parser = JsonModemValues::new(ParserOptions::default());
                        for value in parser.feed(first_half) {
                            value.unwrap();
                        }
                        parser
                    },
                    |mut parser| {
                        let mut produced = 0usize;
                        let incremental_part = black_box(incremental_part);
                        let mut iter = parser.feed(incremental_part);
                        while let Some(event) = LendingIterator::next(&mut iter) {
                            let _ = black_box(event);
                            produced += 1;
                        }
                        produced
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
                        let mut buf = String::with_capacity(payload.len());
                        buf.push_str(first_half);
                        buf
                    },
                    |mut buf| {
                        buf.push_str(black_box(incremental_part));
                        let parsed = parse_partial_json_port::parse_partial_json(Some(&buf));
                        let _ = black_box(parsed);
                    },
                    BatchSize::SmallInput,
                );
            },
        );

        if env::var_os("JSONMODEM_BENCH_COMPARISON").is_some() {
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
                            buf.push_str(black_box(incremental_part));
                            let parsed = partial_json_fixer::fix_json_parse(&buf);
                            let _ = black_box(parsed);
                        },
                        BatchSize::SmallInput,
                    );
                },
            );

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
                            buf.push_str(black_box(incremental_part));
                            let parsed = jiter::JsonValue::parse_with_config(
                                black_box(buf.as_bytes()),
                                false,
                                jiter::PartialMode::TrailingStrings,
                            )
                            .unwrap();
                            black_box(parsed);
                        },
                        BatchSize::SmallInput,
                    );
                },
            );

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
                            buf.push_str(black_box(incremental_part));
                            let parsed = jiter::JsonValue::parse_with_config(
                                black_box(buf.as_bytes()),
                                false,
                                jiter::PartialMode::TrailingStrings,
                            )
                            .unwrap()
                            .into_static();
                            black_box(parsed);
                        },
                        BatchSize::SmallInput,
                    );
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

criterion_group! { name = benches; config = criterion(); targets = bench_streaming_json_incremental }
criterion_main!(benches);
