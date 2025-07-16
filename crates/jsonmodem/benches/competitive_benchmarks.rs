#![allow(missing_docs)]
//! Benchmark comparison of jiter, `serde_json`, and jsonmodem
//!
//! ```text
//! running 48 tests
//! test big_jiter_iter                    ... bench:   3,662,616 ns/iter (+/- 88,878)
//! test big_jiter_value                   ... bench:   6,998,605 ns/iter (+/- 292,383)
//! test big_serde_value                   ... bench:  29,793,191 ns/iter (+/- 576,173)
//! test bigints_array_jiter_iter          ... bench:      11,836 ns/iter (+/- 414)
//! test bigints_array_jiter_value         ... bench:      28,979 ns/iter (+/- 938)
//! test bigints_array_serde_value         ... bench:     129,797 ns/iter (+/- 5,096)
//! test floats_array_jiter_iter           ... bench:      19,302 ns/iter (+/- 631)
//! test floats_array_jiter_value          ... bench:      31,083 ns/iter (+/- 921)
//! test floats_array_serde_value          ... bench:     208,932 ns/iter (+/- 6,167)
//! test lazy_map_lookup_1_10              ... bench:         615 ns/iter (+/- 15)
//! test lazy_map_lookup_2_20              ... bench:       1,776 ns/iter (+/- 36)
//! test lazy_map_lookup_3_50              ... bench:       4,291 ns/iter (+/- 77)
//! test massive_ints_array_jiter_iter     ... bench:      62,244 ns/iter (+/- 1,616)
//! test massive_ints_array_jiter_value    ... bench:      82,889 ns/iter (+/- 1,916)
//! test massive_ints_array_serde_value    ... bench:     498,650 ns/iter (+/- 47,759)
//! test medium_response_jiter_iter        ... bench:           0 ns/iter (+/- 0)
//! test medium_response_jiter_value       ... bench:       3,521 ns/iter (+/- 101)
//! test medium_response_jiter_value_owned ... bench:       6,088 ns/iter (+/- 180)
//! test medium_response_serde_value       ... bench:       9,383 ns/iter (+/- 342)
//! test pass1_jiter_iter                  ... bench:           0 ns/iter (+/- 0)
//! test pass1_jiter_value                 ... bench:       3,048 ns/iter (+/- 79)
//! test pass1_serde_value                 ... bench:       6,588 ns/iter (+/- 232)
//! test pass2_jiter_iter                  ... bench:         384 ns/iter (+/- 9)
//! test pass2_jiter_value                 ... bench:       1,259 ns/iter (+/- 44)
//! test pass2_serde_value                 ... bench:       1,237 ns/iter (+/- 38)
//! test sentence_jiter_iter               ... bench:         283 ns/iter (+/- 10)
//! test sentence_jiter_value              ... bench:         357 ns/iter (+/- 15)
//! test sentence_serde_value              ... bench:         428 ns/iter (+/- 9)
//! test short_numbers_jiter_iter          ... bench:           0 ns/iter (+/- 0)
//! test short_numbers_jiter_value         ... bench:      18,085 ns/iter (+/- 613)
//! test short_numbers_serde_value         ... bench:      87,253 ns/iter (+/- 1,506)
//! test string_array_jiter_iter           ... bench:         615 ns/iter (+/- 18)
//! test string_array_jiter_value          ... bench:       1,410 ns/iter (+/- 44)
//! test string_array_jiter_value_owned    ... bench:       2,863 ns/iter (+/- 151)
//! test string_array_serde_value          ... bench:       3,467 ns/iter (+/- 60)
//! test true_array_jiter_iter             ... bench:         299 ns/iter (+/- 8)
//! test true_array_jiter_value            ... bench:         995 ns/iter (+/- 29)
//! test true_array_serde_value            ... bench:       1,207 ns/iter (+/- 36)
//! test true_object_jiter_iter            ... bench:       2,482 ns/iter (+/- 84)
//! test true_object_jiter_value           ... bench:       2,058 ns/iter (+/- 45)
//! test true_object_serde_value           ... bench:       7,991 ns/iter (+/- 370)
//! test unicode_jiter_iter                ... bench:         315 ns/iter (+/- 7)
//! test unicode_jiter_value               ... bench:         389 ns/iter (+/- 6)
//! test unicode_serde_value               ... bench:         445 ns/iter (+/- 6)
//! test x100_jiter_iter                   ... bench:          12 ns/iter (+/- 0)
//! test x100_jiter_value                  ... bench:          20 ns/iter (+/- 1)
//! test x100_serde_iter                   ... bench:          72 ns/iter (+/- 3)
//! test x100_serde_value                  ... bench:          83 ns/iter (+/- 3)
//! ```

use std::{fs::File, hint::black_box, io::Read, time::Duration};

use criterion::{
    BenchmarkGroup, Criterion, criterion_group, criterion_main, measurement::WallTime,
};
use jiter::{Jiter, JsonValue, Peek};
use jsonmodem::{
    NonScalarValueMode, ParserOptions, StreamingValuesParser, StringValueMode, Value as ModemValue,
};
use serde_json::Value as SerdeValue;

fn read_file(path: &str) -> String {
    let mut file = File::open(path).unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();
    contents
}

fn jiter_value(path: &str, group: &mut BenchmarkGroup<'_, WallTime>) {
    let json = read_file(path);
    let json_data = json.as_bytes();

    group.bench_function("jiter_value", |b| {
        b.iter(|| {
            let v = JsonValue::parse(black_box(json_data), false).unwrap();
            black_box(v)
        });
    });
}

fn jiter_iter_big(path: &str, group: &mut BenchmarkGroup<'_, WallTime>) {
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());

    group.bench_function("jiter_iter", |b| {
        b.iter(|| {
            let mut jiter = Jiter::new(json_data);
            jiter.next_array().unwrap();
            loop {
                if let Some(peek) = jiter.next_array().unwrap() {
                    let i = jiter.known_float(peek).unwrap();
                    black_box(i);
                    while let Some(peek) = jiter.array_step().unwrap() {
                        let i = jiter.known_float(peek).unwrap();
                        black_box(i);
                    }
                }
                if jiter.array_step().unwrap().is_none() {
                    break;
                }
            }
        });
    });
}

fn jiter_iter_pass2(path: &str, group: &mut BenchmarkGroup<'_, WallTime>) {
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());

    #[allow(clippy::items_after_statements)]
    fn find_string(jiter: &mut Jiter) -> String {
        match jiter.peek().unwrap() {
            Peek::String => jiter.known_str().unwrap().to_string(),
            Peek::Array => {
                assert!(jiter.known_array().unwrap().is_some());
                let s = find_string(jiter).to_string();
                assert!(jiter.array_step().unwrap().is_none());
                s
            }
            _ => panic!("Expected string or array"),
        }
    }

    group.bench_function("jiter_iter", |b| {
        b.iter(|| {
            let mut jiter = Jiter::new(json_data);
            let string = find_string(&mut jiter);
            jiter.finish().unwrap();
            black_box(string)
        });
    });
}

fn jiter_iter_string_array(path: &str, group: &mut BenchmarkGroup<'_, WallTime>) {
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());

    group.bench_function("jiter_iter", |b| {
        b.iter(|| {
            let mut jiter = Jiter::new(json_data);
            jiter.next_array().unwrap();
            let i = jiter.known_str().unwrap();
            black_box(i.len());
            while jiter.array_step().unwrap().is_some() {
                let i = jiter.known_str().unwrap();
                black_box(i.len());
            }
            jiter.finish().unwrap();
        });
    });
}

fn jiter_iter_true_array(path: &str, group: &mut BenchmarkGroup<'_, WallTime>) {
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());

    group.bench_function("jiter_iter", |b| {
        b.iter(|| {
            let mut jiter = Jiter::new(json_data);
            let first_peek = jiter.next_array().unwrap().unwrap();
            let i = jiter.known_bool(first_peek).unwrap();
            black_box(i);
            while let Some(peek) = jiter.array_step().unwrap() {
                let i = jiter.known_bool(peek).unwrap();
                black_box(i);
            }
        });
    });
}

fn jiter_iter_true_object(path: &str, group: &mut BenchmarkGroup<'_, WallTime>) {
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());

    group.bench_function("jiter_iter", |b| {
        b.iter(|| {
            let mut jiter = Jiter::new(json_data);
            if let Some(first_key) = jiter.next_object().unwrap() {
                let first_key = first_key.to_string();
                let first_value = jiter.next_bool().unwrap();
                black_box((first_key, first_value));
                while let Some(key) = jiter.next_key().unwrap() {
                    let key = key.to_string();
                    let value = jiter.next_bool().unwrap();
                    black_box((key, value));
                }
            }
        });
    });
}

fn jiter_iter_ints_array(path: &str, group: &mut BenchmarkGroup<'_, WallTime>) {
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());

    group.bench_function("jiter_iter", |b| {
        b.iter(|| {
            let mut jiter = Jiter::new(json_data);
            let first_peek = jiter.next_array().unwrap().unwrap();
            let i = jiter.known_int(first_peek).unwrap();
            black_box(i);
            while let Some(peek) = jiter.array_step().unwrap() {
                let i = jiter.known_int(peek).unwrap();
                black_box(i);
            }
        });
    });
}

fn jiter_iter_floats_array(path: &str, group: &mut BenchmarkGroup<'_, WallTime>) {
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());

    group.bench_function("jiter_iter", |b| {
        b.iter(|| {
            let mut jiter = Jiter::new(json_data);
            let first_peek = jiter.next_array().unwrap().unwrap();
            let i = jiter.known_float(first_peek).unwrap();
            black_box(i);
            while let Some(peek) = jiter.array_step().unwrap() {
                let i = jiter.known_float(peek).unwrap();
                black_box(i);
            }
        });
    });
}

fn jiter_string(path: &str, group: &mut BenchmarkGroup<'_, WallTime>) {
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());

    group.bench_function("jiter_iter", |b| {
        b.iter(|| {
            let mut jiter = Jiter::new(json_data);
            let string = jiter.next_str().unwrap();
            black_box(string);
            jiter.finish().unwrap();
        });
    });
}

fn serde_value(path: &str, group: &mut BenchmarkGroup<'_, WallTime>) {
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());

    group.bench_function("serde_value", |b| {
        b.iter(|| {
            let value: SerdeValue = serde_json::from_slice(json_data).unwrap();
            black_box(value);
        });
    });
}

fn serde_str(path: &str, group: &mut BenchmarkGroup<'_, WallTime>) {
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());

    group.bench_function("serde_iter", |b| {
        b.iter(|| {
            let value: String = serde_json::from_slice(json_data).unwrap();
            black_box(value);
        });
    });
}

fn jsonmodem_value(path: &str, group: &mut BenchmarkGroup<'_, WallTime>) {
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());

    group.bench_function("jsonmodem_value", |b| {
        b.iter(|| {
            let mut parser = StreamingValuesParser::new(ParserOptions {
                non_scalar_values: NonScalarValueMode::Roots,
                string_value_mode: StringValueMode::Values,
                ..Default::default()
            });
            let mut values = parser
                .feed(core::str::from_utf8(json_data).unwrap())
                .unwrap();
            values.extend(parser.finish().unwrap());
            let v: ModemValue = values.pop().unwrap().value;
            black_box(v);
        });
    });
}

struct Dataset {
    name: &'static str,
    jiter_iter: Option<fn(&str, &mut BenchmarkGroup<'_, WallTime>)>,
    serde_iter: bool,
}

fn bench_dataset(cfg: &Dataset, c: &mut Criterion) {
    let path = format!("./benches/jiter_data/{}.json", cfg.name);
    let mut group = c.benchmark_group(cfg.name);
    group.measurement_time(Duration::from_secs(3));
    group.warm_up_time(Duration::from_secs(1));
    jiter_value(&path, &mut group);
    if let Some(f) = cfg.jiter_iter {
        f(&path, &mut group);
    }
    if cfg.serde_iter {
        serde_str(&path, &mut group);
    }
    serde_value(&path, &mut group);
    jsonmodem_value(&path, &mut group);
    group.finish();
}

pub fn competitive_benches(c: &mut Criterion) {
    let datasets = [
        Dataset {
            name: "pass1",
            jiter_iter: None,
            serde_iter: false,
        },
        Dataset {
            name: "big",
            jiter_iter: Some(jiter_iter_big),
            serde_iter: false,
        },
        Dataset {
            name: "pass2",
            jiter_iter: Some(jiter_iter_pass2),
            serde_iter: false,
        },
        Dataset {
            name: "string_array",
            jiter_iter: Some(jiter_iter_string_array),
            serde_iter: false,
        },
        Dataset {
            name: "true_array",
            jiter_iter: Some(jiter_iter_true_array),
            serde_iter: false,
        },
        Dataset {
            name: "true_object",
            jiter_iter: Some(jiter_iter_true_object),
            serde_iter: false,
        },
        Dataset {
            name: "bigints_array",
            jiter_iter: Some(jiter_iter_ints_array),
            serde_iter: false,
        },
        Dataset {
            name: "massive_ints_array",
            jiter_iter: Some(jiter_iter_ints_array),
            serde_iter: false,
        },
        Dataset {
            name: "floats_array",
            jiter_iter: Some(jiter_iter_floats_array),
            serde_iter: false,
        },
        Dataset {
            name: "medium_response",
            jiter_iter: None,
            serde_iter: false,
        },
        Dataset {
            name: "x100",
            jiter_iter: Some(jiter_string),
            serde_iter: true,
        },
        Dataset {
            name: "sentence",
            jiter_iter: Some(jiter_string),
            serde_iter: true,
        },
        Dataset {
            name: "unicode",
            jiter_iter: Some(jiter_string),
            serde_iter: true,
        },
        Dataset {
            name: "short_numbers",
            jiter_iter: None,
            serde_iter: false,
        },
    ];

    for cfg in datasets {
        bench_dataset(&cfg, c);
    }
}

criterion_group!(benches, competitive_benches);
criterion_main!(benches);
