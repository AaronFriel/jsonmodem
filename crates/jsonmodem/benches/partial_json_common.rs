#![allow(missing_docs)]
#![allow(dead_code)]

#[path = "parse_partial_json_port.rs"]
pub mod parse_partial_json_port;
use jsonmodem::{
    NonScalarValueMode, ParserOptions, StreamingParser, StreamingValuesParser, StringValueMode,
};

/// Deterministically create a JSON document of exactly `target_len` bytes.
pub fn make_json_payload(target_len: usize) -> String {
    let overhead = "{\"data\":\"\"}".len();
    assert!(target_len >= overhead);

    let mut s = String::with_capacity(target_len);
    s.push_str("{\"data\":\"");
    s.extend(std::iter::repeat_n('a', target_len - overhead));
    s.push_str("\"}");
    debug_assert_eq!(s.len(), target_len);
    s
}

pub fn run_streaming_parser(payload: &str, parts: usize) -> usize {
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

pub fn run_streaming_values_parser(payload: &str, parts: usize) -> usize {
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

pub fn run_parse_partial_json(payload: &str, parts: usize) -> usize {
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

#[cfg(feature = "comparison")]
pub mod partial_json_fixer {
    use serde_json::Value;

    // Minimal shim so we do not depend on the external crate when building
    // offline for CI.  The behaviour is: attempt repair (`super::fix_json`) →
    // try parsing repaired → fall back to raw.
    pub fn fix_json_parse(partial_json: &str) -> Result<Value, serde_json::Error> {
        let repaired = super::parse_partial_json_port::fix_json(partial_json);
        serde_json::from_str(&repaired).or_else(|_| serde_json::from_str(partial_json))
    }
}

#[cfg(feature = "comparison")]
pub fn run_fix_json_parse(payload: &str, parts: usize) -> usize {
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

#[cfg(feature = "comparison")]
pub fn run_jiter_partial(payload: &str, parts: usize) -> usize {
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

#[cfg(feature = "comparison")]
pub fn run_jiter_partial_owned(payload: &str, parts: usize) -> usize {
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
