#![expect(missing_docs)]

mod streaming_json_common;

use std::{hint::black_box, time::Duration};

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use jsonmodem::{
    JsonModem, JsonModemBuffers, ParserOptions, StdBackend, lending_iterator::LendingIterator,
};
use streaming_json_common::{
    produce_chunks, run_jsonmodem_events, run_jsonmodem_events_single, run_jsonmodem_values_single,
};

fn make_ascii_string_payload(bytes: usize) -> String {
    format!("{{\"k\":\"{}\"}}", "a".repeat(bytes))
}

fn make_object_payload(fields: usize, value_len: usize) -> String {
    let mut s = String::with_capacity(fields * (value_len + 20));
    s.push('{');
    for i in 0..fields {
        if i > 0 {
            s.push(',');
        }
        s.push('"');
        s.push('k');
        s.push_str(&i.to_string());
        s.push_str("\":\"");
        s.push_str(&"v".repeat(value_len));
        s.push('"');
    }
    s.push('}');
    s
}

fn run_drop_empty_feed_cycles(cycles: usize) -> usize {
    let options = ParserOptions::default().with_allow_multiple_json_values(true);
    let mut parser = JsonModem::<StdBackend>::new(options);
    let mut events = 0usize;
    for _ in 0..cycles {
        let mut iter = parser.feed("null ");
        while let Some(event) = iter.next() {
            event.unwrap();
            events += 1;
        }
    }
    let mut iter = parser.finish();
    while let Some(event) = iter.next() {
        event.unwrap();
        events += 1;
    }
    events
}

fn run_buffers_single(payload: &str) -> usize {
    let mut parser = JsonModemBuffers::<StdBackend, _>::new(
        ParserOptions::default(),
        jsonmodem::BufferOptions::default(),
    );
    let mut events = 0usize;
    {
        let mut iter = parser.feed(payload);
        while let Some(event) = iter.next() {
            event.unwrap();
            events += 1;
        }
    }
    let mut iter = parser.finish();
    while let Some(event) = iter.next() {
        event.unwrap();
        events += 1;
    }
    events
}

fn bench_hotspot_targets(c: &mut Criterion) {
    let mut group = c.benchmark_group("hotspot_targets");

    let scanner_payload = make_ascii_string_payload(256 * 1024);
    group.bench_function("parser_events_ascii_single_chunk_256k", |b| {
        b.iter(|| {
            let total = run_jsonmodem_events_single(black_box(scanner_payload.as_str()));
            black_box(total);
        });
    });

    let scanner_chunks = produce_chunks(&scanner_payload, 1024);
    group.bench_function("parser_events_ascii_cross_chunk_256k_1024", |b| {
        b.iter(|| {
            let total = run_jsonmodem_events(black_box(&scanner_chunks));
            black_box(total);
        });
    });

    group.bench_with_input(
        BenchmarkId::new("iterator_drop_small_value_cycles", 4096usize),
        &4096usize,
        |b, &cycles| {
            b.iter(|| {
                let total = run_drop_empty_feed_cycles(black_box(cycles));
                black_box(total);
            });
        },
    );

    let object_payload = make_object_payload(1024, 48);
    group.bench_function("buffers_object_assembly_1024x48", |b| {
        b.iter(|| {
            let total = run_buffers_single(black_box(object_payload.as_str()));
            black_box(total);
        });
    });

    group.bench_function("values_object_assembly_1024x48", |b| {
        b.iter(|| {
            let total = run_jsonmodem_values_single(black_box(object_payload.as_str()));
            black_box(total);
        });
    });

    group.finish();
}

fn criterion() -> Criterion {
    let mut c = Criterion::default();
    if std::env::var_os("JSONMODEM_BENCH_FAST").is_some() {
        c = c
            .warm_up_time(Duration::from_millis(10))
            .measurement_time(Duration::from_millis(120))
            .sample_size(10);
    } else {
        c = c
            .warm_up_time(Duration::from_secs(5))
            .measurement_time(Duration::from_secs(10));
    }
    c
}

criterion_group! { name = benches; config = criterion(); targets = bench_hotspot_targets }
criterion_main!(benches);
