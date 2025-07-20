#![allow(missing_docs)]
#![cfg(feature = "comparison")]

mod partial_json_common;
use std::time::Duration;

use criterion::{BatchSize, BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use jsonmodem::{
    NonScalarValueMode, ParserOptions, StreamingParser, StreamingValuesParser, StringValueMode,
};
use partial_json_common::{make_json_payload, parse_partial_json_port, partial_json_fixer};

#[allow(clippy::too_many_lines)]
fn bench_partial_json_incremental(c: &mut Criterion) {
    let payload = make_json_payload(10_000);
    let payload_bytes = payload.as_bytes();

    // Split the payload exactly in half. The first half will be considered the
    // already-received portion while the second half will be fed to the
    // strategies in *parts* equally-sized chunks. Only the cost of processing
    // ONE of those chunks is measured.
    let midpoint = payload_bytes.len() / 2;

    let first_half = &payload[..midpoint];
    let second_half = &payload[midpoint..];

    let mut group = c.benchmark_group("partial_json_incremental");
    group.measurement_time(Duration::from_secs(10));
    group.warm_up_time(Duration::from_secs(5));

    for &parts in &[100usize, 1_000, 5_000] {
        // size of one incremental chunk we want to measure
        let chunk_size = second_half.len().div_ceil(parts);
        let incremental_part = &second_half[..chunk_size];

        group.bench_with_input(
            BenchmarkId::new("streaming_parser_inc", parts),
            &parts,
            |b, &_p| {
                b.iter_batched(
                    || {
                        // setup – not measured
                        let mut parser = StreamingParser::new(ParserOptions::default());
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
            },
        );

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

criterion_group!(benches, bench_partial_json_incremental);
criterion_main!(benches);
