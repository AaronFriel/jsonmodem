#![allow(dead_code)]

#[path = "parse_partial_json_port.rs"]
pub mod parse_partial_json_port;
use jsonmodem::{
    BufferOptions, JsonModem, JsonModemBuffers, JsonModemValues, ParserOptions, StdBackend,
    lending_iterator::LendingIterator,
};

pub fn produce_chunks(payload: &str, parts: usize) -> Vec<&str> {
    assert!(parts > 0);
    let len = payload.len();
    let chunk_size = len.div_ceil(parts);
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < len {
        let mut end = core::cmp::min(start + chunk_size, len);
        while end < len && !payload.is_char_boundary(end) {
            end += 1;
        }
        chunks.push(&payload[start..end]);
        start = end;
    }
    chunks
}

/// Deterministically create a JSON document of exactly `target_len` bytes.
pub fn make_json_payload(target_len: usize) -> String {
    let overhead = "{\"data\":\"\"}".len();
    assert!(target_len >= overhead);

    let mut s = String::with_capacity(target_len);
    s.push_str("{\"data\":\"");
    s.extend(std::iter::repeat_n('a', target_len - overhead));
    s.push_str("\"}");
    #[cfg(any(fuzzing, debug_assertions))]
    assert_eq!(s.len(), target_len);
    s
}

pub fn run_jsonmodem_events(chunks: &[&str]) -> usize {
    let mut parser = JsonModem::<StdBackend>::new(ParserOptions::default());
    let mut events = 0usize;

    for &chunk in chunks {
        let mut iter = parser.feed(chunk);
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

pub fn run_jsonmodem_buffers(chunks: &[&str]) -> usize {
    let mut parser =
        JsonModemBuffers::<StdBackend, _>::new(ParserOptions::default(), BufferOptions::default());
    let mut events = 0usize;

    for &chunk in chunks {
        let mut iter = parser.feed(chunk);
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

pub fn run_jsonmodem_values(chunks: &[&str]) -> usize {
    let mut parser = JsonModemValues::<StdBackend, _>::new(ParserOptions::default());
    let mut produced = 0usize;

    for &chunk in chunks {
        let mut iter = parser.feed(chunk);
        while let Some(value) = LendingIterator::next(&mut iter) {
            let value = value.expect("values parse failure");
            if value.is_final {
                produced += 1;
            }
        }
    }

    let mut iter = parser.finish();
    while let Some(value) = LendingIterator::next(&mut iter) {
        let value = value.expect("values finish failure");
        if value.is_final {
            produced += 1;
        }
    }

    produced
}

pub fn run_parse_partial_json(chunks: &[&str]) -> usize {
    let mut calls = 0usize;
    let mut prefix = String::new();

    for &chunk in chunks {
        prefix.push_str(chunk);
        let _ = parse_partial_json_port::parse_partial_json(Some(&prefix));
        calls += 1;
    }

    calls
}

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

pub fn run_fix_json_parse(chunks: &[&str]) -> usize {
    let mut calls = 0usize;
    let mut prefix = String::new();

    for &chunk in chunks {
        prefix.push_str(chunk);
        let _ = partial_json_fixer::fix_json_parse(&prefix);
        calls += 1;
    }

    calls
}

pub fn run_jiter_partial(chunks: &[&str]) -> usize {
    use jiter::{JsonValue, PartialMode};
    let mut calls = 0usize;
    let mut prefix = String::new();

    for &chunk in chunks {
        prefix.push_str(chunk);
        let _ =
            JsonValue::parse_with_config(prefix.as_bytes(), false, PartialMode::TrailingStrings)
                .unwrap();
        calls += 1;
    }

    calls
}

pub fn run_jiter_partial_owned(chunks: &[&str]) -> usize {
    use jiter::{JsonValue, PartialMode};
    let mut calls = 0usize;
    let mut prefix = String::new();

    for &chunk in chunks {
        prefix.push_str(chunk);
        let _ =
            JsonValue::parse_with_config(prefix.as_bytes(), false, PartialMode::TrailingStrings)
                .unwrap()
                .into_static();
        calls += 1;
    }

    calls
}
