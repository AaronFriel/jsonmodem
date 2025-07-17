#![allow(missing_docs)]

mod parse_partial_json_port;

use std::time::Duration;

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use jsonmodem::{
    NonScalarValueMode, ParserOptions, StreamingParser, StreamingValuesParser, StringValueMode,
};

/// Deterministically create a JSON document of exactly `target_len` bytes.
fn make_json_payload(target_len: usize) -> String {
    let overhead = "{\"data\":\"\"}".len();
    assert!(target_len >= overhead);

    let mut s = String::with_capacity(target_len);
    s.push_str("{\"data\":\"");
    s.extend(std::iter::repeat_n('a', target_len - overhead));
    s.push_str("\"}");
    debug_assert_eq!(s.len(), target_len);
    s
}

fn run_streaming_parser(payload: &str, parts: usize) -> usize {
    let chunk_size = payload.len().div_ceil(parts);
    let mut parser = StreamingParser::new(ParserOptions::default());
    let mut events = 0usize;

    for chunk in payload.as_bytes().chunks(chunk_size) {
        parser.feed(std::str::from_utf8(chunk).unwrap());
        for _ in &mut parser {
            events += 1;
        }
    }

    for res in parser.finish() {
        let _ = res.unwrap();
        events += 1;
    }

    events
}

fn run_streaming_values_parser(payload: &str, parts: usize) -> usize {
    let chunk_size = payload.len().div_ceil(parts);
    let mut parser = StreamingValuesParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::Roots,
        string_value_mode: StringValueMode::Values,
        ..Default::default()
    });
    let mut produced = 0usize;

    for chunk in payload.as_bytes().chunks(chunk_size) {
        let values = parser.feed(std::str::from_utf8(chunk).unwrap()).unwrap();
        produced += values.iter().filter(|v| v.is_final).count();
    }

    let values = parser.finish().unwrap();
    produced + values.iter().filter(|v| v.is_final).count()
}

fn run_parse_partial_json(payload: &str, parts: usize) -> usize {
    let chunk_size = payload.len().div_ceil(parts);
    let mut buf = String::with_capacity(payload.len());
    let mut calls = 0;

    for chunk in payload.as_bytes().chunks(chunk_size) {
        buf.push_str(std::str::from_utf8(chunk).unwrap());
        let _ = parse_partial_json_port::parse_partial_json(Some(&buf));
        calls += 1;
    }

    calls
}

mod partial_json_fixer {
    use serde_json::Value;

    // Minimal shim so we do not depend on the external crate when building
    // offline for CI.  The behaviour is: attempt repair (`super::fix_json`) →
    // try parsing repaired → fall back to raw.
    pub fn fix_json_parse(partial_json: &str) -> Result<Value, serde_json::Error> {
        let repaired = super::parse_partial_json_port::fix_json(partial_json);
        serde_json::from_str(&repaired).or_else(|_| serde_json::from_str(partial_json))
    }
}

fn run_fix_json_parse(payload: &str, parts: usize) -> usize {
    let chunk_size = payload.len().div_ceil(parts);
    let mut buf = String::with_capacity(payload.len());
    let mut calls = 0;

    for chunk in payload.as_bytes().chunks(chunk_size) {
        buf.push_str(std::str::from_utf8(chunk).unwrap());
        let _ = partial_json_fixer::fix_json_parse(&buf);
        calls += 1;
    }

    calls
}

fn run_jiter_partial(payload: &str, parts: usize) -> usize {
    use jiter::{JsonValue, PartialMode};

    let chunk_size = payload.len().div_ceil(parts);
    let mut buf = String::with_capacity(payload.len());
    let mut calls = 0usize;

    for chunk in payload.as_bytes().chunks(chunk_size) {
        buf.push_str(std::str::from_utf8(chunk).unwrap());
        let _ = JsonValue::parse_with_config(buf.as_bytes(), false, PartialMode::TrailingStrings)
            .unwrap();
        calls += 1;
    }

    calls
}

fn run_jiter_partial_owned(payload: &str, parts: usize) -> usize {
    use jiter::{JsonValue, PartialMode};

    let chunk_size = payload.len().div_ceil(parts);
    let mut buf = String::with_capacity(payload.len());
    let mut calls = 0usize;

    for chunk in payload.as_bytes().chunks(chunk_size) {
        buf.push_str(std::str::from_utf8(chunk).unwrap());
        let _ = JsonValue::parse_with_config(buf.as_bytes(), false, PartialMode::TrailingStrings)
            .unwrap()
            .into_static();
        calls += 1;
    }

    calls
}

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

        group.bench_with_input(BenchmarkId::new("jiter_partial", parts), &parts, |b, &p| {
            b.iter(|| {
                let v = run_jiter_partial(black_box(&payload), p);
                black_box(v);
            });
        });

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

fn bench_partial_json_big(c: &mut Criterion) {
    let payload = std::fs::read_to_string("./benches/jiter_data/medium_response.json").unwrap();

    let mut group = c.benchmark_group("partial_json_big");
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

        group.bench_with_input(BenchmarkId::new("jiter_partial", parts), &parts, |b, &p| {
            b.iter(|| {
                let v = run_jiter_partial(black_box(&payload), p);
                black_box(v);
            });
        });

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

use criterion::BatchSize;

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

criterion_group!(
    benches,
    bench_partial_json_strategies,
    bench_partial_json_big,
    bench_partial_json_incremental
);

criterion_main!(benches);
