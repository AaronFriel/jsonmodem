//! Manual profiling harness for `JsonModem` vs `Jiter`.
use std::time::Instant;

#[path = "../benches/streaming_json_common.rs"]
mod bench_common;

fn main() {
    let iterations: usize = std::env::var("ITERATIONS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100)
        .max(1);
    let iterations_u32 =
        u32::try_from(iterations).expect("ITERATIONS must fit into a 32-bit unsigned integer");

    let payload = include_str!("../benches/jiter_data/response_large.json");

    let start = Instant::now();
    let mut events_total = 0usize;
    for _ in 0..iterations {
        events_total += bench_common::run_jsonmodem_events_single(payload);
    }
    let jsonmodem_elapsed = start.elapsed();

    let start = Instant::now();
    let mut values_total = 0usize;
    for _ in 0..iterations {
        values_total += bench_common::run_jiter_value(payload);
    }
    let jiter_elapsed = start.elapsed();

    println!(
        "JsonModem events: {events_total} events in {:?} (avg {:?})",
        jsonmodem_elapsed,
        jsonmodem_elapsed / iterations_u32
    );
    println!(
        "Jiter values: {values_total} values in {:?} (avg {:?})",
        jiter_elapsed,
        jiter_elapsed / iterations_u32
    );
}
