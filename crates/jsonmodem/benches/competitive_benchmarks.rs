#![allow(missing_docs)]
//! Benchmark comparison of jiter, serde_json, and jsonmodem
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

use std::fs::File;
use std::hint::black_box;
use std::io::Read;
use std::path::Path;

use criterion::{Criterion, criterion_group, criterion_main};
use jiter::{Jiter, JsonValue, Peek};
use jsonmodem::{
    NonScalarValueMode, ParserOptions, StreamingValuesParser, StringValueMode, Value as ModemValue,
};
use serde_json::Value as SerdeValue;

fn read_title(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned()
}

fn read_file(path: &str) -> String {
    let mut file = File::open(path).unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();
    contents
}

fn jiter_value(path: &str, c: &mut Criterion) {
    let title = read_title(path) + "_jiter_value";
    let json = read_file(path);
    let json_data = json.as_bytes();

    c.bench_function(&title, |b| {
        b.iter(|| {
            let v = JsonValue::parse(black_box(json_data), false).unwrap();
            black_box(v)
        });
    });
}

fn jiter_iter_big(path: &str, c: &mut Criterion) {
    let title = read_title(path) + "_jiter_iter";
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());

    c.bench_function(&title, |b| {
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

fn jiter_iter_pass2(path: &str, c: &mut Criterion) {
    let title = read_title(path) + "_jiter_iter";
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());

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

    c.bench_function(&title, |b| {
        b.iter(|| {
            let mut jiter = Jiter::new(json_data);
            let string = find_string(&mut jiter);
            jiter.finish().unwrap();
            black_box(string)
        });
    });
}

fn jiter_iter_string_array(path: &str, c: &mut Criterion) {
    let title = read_title(path) + "_jiter_iter";
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());

    c.bench_function(&title, |b| {
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

fn jiter_iter_true_array(path: &str, c: &mut Criterion) {
    let title = read_title(path) + "_jiter_iter";
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());

    c.bench_function(&title, |b| {
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

fn jiter_iter_true_object(path: &str, c: &mut Criterion) {
    let title = read_title(path) + "_jiter_iter";
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());

    c.bench_function(&title, |b| {
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

fn jiter_iter_ints_array(path: &str, c: &mut Criterion) {
    let title = read_title(path) + "_jiter_iter";
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());

    c.bench_function(&title, |b| {
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

fn jiter_iter_floats_array(path: &str, c: &mut Criterion) {
    let title = read_title(path) + "_jiter_iter";
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());

    c.bench_function(&title, |b| {
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

fn jiter_string(path: &str, c: &mut Criterion) {
    let title = read_title(path) + "_jiter_iter";
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());

    c.bench_function(&title, |b| {
        b.iter(|| {
            let mut jiter = Jiter::new(json_data);
            let string = jiter.next_str().unwrap();
            black_box(string);
            jiter.finish().unwrap();
        });
    });
}

fn serde_value(path: &str, c: &mut Criterion) {
    let title = read_title(path) + "_serde_value";
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());

    c.bench_function(&title, |b| {
        b.iter(|| {
            let value: SerdeValue = serde_json::from_slice(json_data).unwrap();
            black_box(value);
        });
    });
}

fn serde_str(path: &str, c: &mut Criterion) {
    let title = read_title(path) + "_serde_iter";
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());

    c.bench_function(&title, |b| {
        b.iter(|| {
            let value: String = serde_json::from_slice(json_data).unwrap();
            black_box(value);
        });
    });
}

fn x100_serde_iter(c: &mut Criterion) {
    serde_str("./benches/jiter_data/x100.json", c);
}

fn jsonmodem_value(path: &str, c: &mut Criterion) {
    let title = read_title(path) + "_jsonmodem_value";
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());

    c.bench_function(&title, |b| {
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

macro_rules! test_cases {
    ($file_name:ident) => {
        paste::item! {
            fn [< $file_name _jiter_value >](c: &mut Criterion) {
                let path = format!("./benches/jiter_data/{}.json", stringify!($file_name));
                jiter_value(&path, c);
            }
            fn [< $file_name _jiter_iter >](c: &mut Criterion) {
                let file_name = stringify!($file_name);
                let path = format!("./benches/jiter_data/{}.json", file_name);
                if file_name == "big" {
                    jiter_iter_big(&path, c);
                } else if file_name == "pass2" {
                    jiter_iter_pass2(&path, c);
                } else if file_name == "string_array" {
                    jiter_iter_string_array(&path, c);
                } else if file_name == "true_array" {
                    jiter_iter_true_array(&path, c);
                } else if file_name == "true_object" {
                    jiter_iter_true_object(&path, c);
                } else if file_name == "bigints_array" {
                    jiter_iter_ints_array(&path, c);
                } else if file_name == "massive_ints_array" {
                    jiter_iter_ints_array(&path, c);
                } else if file_name == "floats_array" {
                    jiter_iter_floats_array(&path, c);
                } else if file_name == "x100" || file_name == "sentence" || file_name == "unicode" {
                    jiter_string(&path, c);
                }
            }
            fn [< $file_name _serde_value >](c: &mut Criterion) {
                let path = format!("./benches/jiter_data/{}.json", stringify!($file_name));
                serde_value(&path, c);
            }
            fn [< $file_name _jsonmodem_value >](c: &mut Criterion) {
                let path = format!("./benches/jiter_data/{}.json", stringify!($file_name));
                jsonmodem_value(&path, c);
            }
        }
    };
}

test_cases!(pass1);
test_cases!(big);
test_cases!(pass2);
test_cases!(string_array);
test_cases!(true_array);
test_cases!(true_object);
test_cases!(bigints_array);
test_cases!(massive_ints_array);
test_cases!(floats_array);
test_cases!(medium_response);
test_cases!(x100);
test_cases!(sentence);
test_cases!(unicode);
test_cases!(short_numbers);

criterion_group!(
    benches,
    big_jiter_iter,
    big_jiter_value,
    big_serde_value,
    big_jsonmodem_value,
    bigints_array_jiter_iter,
    bigints_array_jiter_value,
    bigints_array_serde_value,
    bigints_array_jsonmodem_value,
    floats_array_jiter_iter,
    floats_array_jiter_value,
    floats_array_serde_value,
    floats_array_jsonmodem_value,
    massive_ints_array_jiter_iter,
    massive_ints_array_jiter_value,
    massive_ints_array_serde_value,
    massive_ints_array_jsonmodem_value,
    medium_response_jiter_iter,
    medium_response_jiter_value,
    medium_response_serde_value,
    medium_response_jsonmodem_value,
    x100_jiter_iter,
    x100_jiter_value,
    x100_serde_iter,
    x100_serde_value,
    x100_jsonmodem_value,
    sentence_jiter_iter,
    sentence_jiter_value,
    sentence_serde_value,
    sentence_jsonmodem_value,
    unicode_jiter_iter,
    unicode_jiter_value,
    unicode_serde_value,
    unicode_jsonmodem_value,
    pass1_jiter_iter,
    pass1_jiter_value,
    pass1_serde_value,
    pass1_jsonmodem_value,
    pass2_jiter_iter,
    pass2_jiter_value,
    pass2_serde_value,
    pass2_jsonmodem_value,
    string_array_jiter_iter,
    string_array_jiter_value,
    string_array_serde_value,
    string_array_jsonmodem_value,
    true_array_jiter_iter,
    true_array_jiter_value,
    true_array_serde_value,
    true_array_jsonmodem_value,
    true_object_jiter_iter,
    true_object_jiter_value,
    true_object_serde_value,
    true_object_jsonmodem_value,
    short_numbers_jiter_iter,
    short_numbers_jiter_value,
    short_numbers_serde_value,
    short_numbers_jsonmodem_value,
);
criterion_main!(benches);
