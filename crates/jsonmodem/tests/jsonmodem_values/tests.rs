#![cfg(not(miri))]
use std::string::ToString;

use insta::assert_snapshot;
use jsonmodem::{
    BufferOptions, ImValueAssembler, JsonModemValues, ParserOptions, StreamingValue, Value,
    ValuesOptions,
};
use quickcheck::QuickCheck;
use serde_json::{self, Value as SerdeValue};

fn chunk_input(input: &str, chunk_size: usize) -> Vec<String> {
    let chunk_size = chunk_size.max(1);
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < input.len() {
        let end = core::cmp::min(start + chunk_size, input.len());
        chunks.push(input[start..end].to_owned());
        start = end;
    }
    if chunks.is_empty() {
        chunks.push(String::new());
    }
    chunks
}

fn collect_streaming_values(chunks: &[&str], options: ValuesOptions) -> Vec<StreamingValue<Value>> {
    let mut modem = JsonModemValues::with_options(ParserOptions::default(), options);
    let mut out = Vec::new();
    for chunk in chunks {
        out.extend(
            modem
                .feed(chunk)
                .map(|res| res.expect("values iterator error")),
        );
    }
    out.extend(modem.finish().map(|res| res.expect("values finish error")));
    out
}

fn collect_im_streaming_values(
    chunks: &[&str],
    options: ValuesOptions,
) -> Vec<StreamingValue<String>> {
    let assembler = ImValueAssembler::new(BufferOptions::default());
    let mut modem =
        JsonModemValues::with_buffer_builder(ParserOptions::default(), options, assembler);
    let mut out = Vec::new();
    for chunk in chunks {
        for value in modem.feed(chunk) {
            let value = value.expect("im values iterator error");
            out.push(StreamingValue {
                index: value.index,
                value: value.value.to_string(),
                is_final: value.is_final,
            });
        }
    }
    for value in modem.finish() {
        let value = value.expect("im values finish error");
        out.push(StreamingValue {
            index: value.index,
            value: value.value.to_string(),
            is_final: value.is_final,
        });
    }
    out
}

#[test]
fn im_backend_basic_roundtrip() {
    let assembler = ImValueAssembler::new(BufferOptions::default());
    let mut values = JsonModemValues::with_buffer_builder(
        ParserOptions::default(),
        ValuesOptions::default(),
        assembler,
    );

    let mut finals = Vec::new();
    for value in values.feed("[1,2,3]") {
        let value = value.expect("iterator error");
        if value.is_final {
            finals.push(value.value.to_string());
        }
    }

    for value in values.finish() {
        let value = value.expect("finish error");
        if value.is_final {
            finals.push(value.value.to_string());
        }
    }

    assert_eq!(finals, ["[1,2,3]".to_string()]);
}

#[test]
fn im_backend_handles_nested_chunks() {
    let assembler = ImValueAssembler::new(BufferOptions::default());
    let mut values = JsonModemValues::with_buffer_builder(
        ParserOptions::default(),
        ValuesOptions::default(),
        assembler,
    );

    let chunks = [
        "{\"outer\":{\"inner\":\"va",
        "lu\",\"list\":[1,",
        "2,3]},\"flag\":true}",
    ];

    let mut finals = Vec::new();
    for chunk in &chunks {
        for value in values.feed(chunk) {
            let value = value.expect("iterator error");
            if value.is_final {
                finals.push(value.value.to_string());
            }
        }
    }

    for value in values.finish() {
        let value = value.expect("finish error");
        if value.is_final {
            finals.push(value.value.to_string());
        }
    }

    let actual = finals
        .pop()
        .expect("expected a final value from immutable backend");
    let parsed: SerdeValue = serde_json::from_str(&actual).unwrap();
    let expected: SerdeValue =
        serde_json::from_str("{\"outer\":{\"inner\":\"valu\",\"list\":[1,2,3]},\"flag\":true}")
            .unwrap();
    assert_eq!(parsed, expected);
}

fn render_streaming_values(chunks: &[&str], options: ValuesOptions) -> String {
    let mut lines = String::new();
    for value in collect_streaming_values(chunks, options) {
        use core::fmt::Write;
        writeln!(
            lines,
            "index={} final={} value={}",
            value.index, value.is_final, value.value
        )
        .unwrap();
    }
    lines
}

#[test]
fn values_partial_produces_updates_per_feed() {
    let mut modem = JsonModemValues::with_options(
        ParserOptions::default(),
        ValuesOptions::default().with_partial(true),
    );

    let first: Vec<_> = modem
        .feed("{\"title\":\"hel")
        .to_iter()
        .map(Result::unwrap)
        .collect();
    assert!(
        !first.is_empty(),
        "partial mode should yield updates before the value is complete",
    );

    let second: Vec<_> = modem
        .feed("lo\",\"count\":1}")
        .to_iter()
        .map(Result::unwrap)
        .collect();
    assert!(
        second.iter().any(|value| value.is_final),
        "final chunk should still produce a closing value",
    );
}

#[test]
fn values_view_root_tracks_partial_state() {
    let mut modem = JsonModemValues::with_options(
        ParserOptions::default(),
        ValuesOptions::default().with_partial(true),
    );

    assert!(matches!(modem.view_root(), Value::Null));

    {
        let mut iter = modem.feed("{\"title\":\"hel");
        let _ = iter.next();
    }
    assert!(matches!(modem.view_root(), Value::Object(_)));

    {
        let mut iter = modem.feed("lo\"}");
        let _ = iter.next();
    }
    assert!(matches!(modem.view_root(), Value::Object(_)));
    let closed = modem.finish();
    let _ = closed.to_iter().collect::<Vec<_>>();
}

#[test]
fn snapshot_values_multiple_roots() {
    let chunks = ["1 true {\"a\":\"pa", "rt\",\"items\":[3,4]} null"];
    let rendered = render_streaming_values(&chunks, ValuesOptions::default());

    assert_snapshot!(rendered, @r#"
    index=0 final=true value=1
    index=1 final=true value=true
    index=2 final=true value={"a":"part","items":[3,4]}
    index=3 final=true value=null
    "#);
}

fn values_array_matches_serde(digits: Vec<u8>, chunk_size: u8) -> bool {
    let values: Vec<i16> = digits
        .into_iter()
        .map(|value| i16::from(value % 10))
        .collect();
    let joined = values
        .iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let json = format!("[{joined}]");
    let chunk_size = usize::from(chunk_size.max(1));

    let mut modem = JsonModemValues::new(ParserOptions::default());
    let mut outputs = Vec::new();

    for chunk in chunk_input(&json, chunk_size) {
        for event in modem.feed(&chunk).to_iter() {
            let Ok(event) = event else { return false };
            outputs.push(event);
        }
    }

    let closed = modem.finish();
    for event in closed.to_iter() {
        let Ok(event) = event else { return false };
        outputs.push(event);
    }

    if outputs.is_empty() {
        return true;
    }

    let final_value = match outputs.iter().rev().find(|value| value.is_final) {
        Some(value) => value.value.to_string(),
        None => return false,
    };

    let expected = serde_json::to_string(&SerdeValue::from(values)).unwrap();
    final_value == expected
}

fn im_values_array_matches_serde(digits: Vec<u8>, chunk_size: u8) -> bool {
    let values: Vec<i16> = digits
        .into_iter()
        .map(|value| i16::from(value % 10))
        .collect();
    let joined = values
        .iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let json = format!("[{joined}]");
    let chunk_size = usize::from(chunk_size.max(1));
    let chunked = chunk_input(&json, chunk_size);
    let borrowed = chunked.iter().map(String::as_str).collect::<Vec<_>>();

    let outputs = collect_im_streaming_values(&borrowed, ValuesOptions::default());

    if outputs.is_empty() {
        return true;
    }

    let final_value = match outputs.iter().rev().find(|value| value.is_final) {
        Some(value) => value.value.clone(),
        None => return false,
    };

    let expected = serde_json::to_string(&SerdeValue::from(values)).unwrap();
    final_value == expected
}

#[test]
fn prop_values_array_roundtrip() {
    QuickCheck::new()
        .tests(50)
        .quickcheck(values_array_matches_serde as fn(Vec<u8>, u8) -> bool);
}

#[test]
fn prop_im_values_array_roundtrip() {
    QuickCheck::new()
        .tests(50)
        .quickcheck(im_values_array_matches_serde as fn(Vec<u8>, u8) -> bool);
}
