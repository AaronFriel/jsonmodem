use alloc::{
    string::{String, ToString},
    vec,
    vec::Vec,
};
use std::dbg;

use quickcheck::{QuickCheck, TestResult};

use crate::{ParseEvent, ParserOptions, StreamingParser, Value, event::reconstruct_values};

/// Repro for missing string roots in multi-value stream reconstruction.
/// Currently fails because no complete Value events are emitted for top-level
/// strings.
#[test]
fn repro_multi_value_string_root() {
    let payload = "\"x\"";
    let mut parser = StreamingParser::new(ParserOptions {
        allow_multiple_json_values: true,
        emit_non_scalar_values: true,
        ..Default::default()
    });
    parser.feed(payload);
    let events: Vec<_> = parser.map(|x| x.unwrap()).collect();
    assert_eq!(
        &events,
        &[ParseEvent::String {
            path: vec![],
            fragment: String::from("x"),
            is_final: true,
            value: None,
        },]
    );
    let reconstructed = reconstruct_values(events);
    // Expect one string root, but current implementation drops string roots
    // entirely.
    assert_eq!(reconstructed, vec![Value::String(String::from("x"))]);
}

/// Property: A stream consisting of multiple JSON roots must round-trip through
/// the incremental parser regardless of input partitioning.
#[test]
fn multi_value_roundtrip_quickcheck() {
    #[allow(clippy::needless_pass_by_value)]
    fn prop(values: Vec<Value>, splits: Vec<usize>) -> TestResult {
        if values.is_empty() {
            return TestResult::discard();
        }

        // Join all roots separated by a single space (valid JSON whitespace).
        let payload: String = values
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" ");

        let mut parser = StreamingParser::new(ParserOptions {
            allow_multiple_json_values: true,
            emit_non_scalar_values: true,
            ..Default::default()
        });
        let mut events = Vec::<crate::event::ParseEvent>::new();

        // For debugging purposes:
        let mut chunks = vec![];

        // Feed payload in arbitrary chunk sizes.
        let chars: Vec<char> = payload.chars().collect();
        let mut idx = 0;
        let mut remaining = chars.len();

        for s in &splits {
            if remaining == 0 {
                break;
            }
            let size = 1 + (s % remaining);
            let end = idx + size;
            let chunk: String = chars[idx..end].iter().collect();
            chunks.push(chunk.clone());
            parser.feed(&chunk);
            for event in parser.by_ref() {
                match event {
                    Ok(event) => events.push(event),
                    Err(err) => {
                        dbg!(chunks, err);
                        return TestResult::failed();
                    }
                }
            }
            idx = end;
            remaining -= size;
        }
        if remaining > 0 {
            let chunk: String = chars[idx..].iter().collect();

            chunks.push(chunk.clone());
            parser.feed(&chunk);
            for event in parser.by_ref() {
                match event {
                    Ok(event) => events.push(event),
                    Err(err) => {
                        dbg!(chunks, err);
                        return TestResult::failed();
                    }
                }
            }
        }
        let parser = parser.finish();
        for event in parser {
            match event {
                Ok(event) => events.push(event),
                Err(err) => {
                    dbg!(chunks, err);
                    return TestResult::failed();
                }
            }
        }

        let reconstructed = reconstruct_values(events);
        let original: Vec<Value> = values.into_iter().collect();

        let result = reconstructed == original;
        if !result {
            dbg!(chunks);
            dbg!(&original);
            dbg!(&reconstructed);
            dbg!(reconstructed == original);
        }

        TestResult::from_bool(result)
    }

    #[cfg(not(miri))]
    let tests = if is_ci::cached() { 10_000 } else { 1_000 };
    #[cfg(miri)]
    let tests = 10;

    QuickCheck::new()
        .tests(tests)
        .quickcheck(prop as fn(Vec<Value>, Vec<usize>) -> TestResult);
}

#[test]
fn multi_value_roundtrip_repro() {
    let chunks = ["{\"/ꑆ\u{fff2}\u{4a9d3}‼\"", ":\"\u{e1cac}\",\">]\":false}"];

    let mut parser = StreamingParser::new(ParserOptions {
        allow_multiple_json_values: true,
        emit_non_scalar_values: true,
        panic_on_error: true,
        ..Default::default()
    });
    let mut events = vec![];
    for chunk in &chunks {
        parser.feed(chunk);
        for event in parser.by_ref() {
            match event {
                Ok(event) => events.push(event),
                Err(err) => {
                    dbg!(&err);
                    panic!("Error while parsing: {}", err);
                }
            }
        }
    }

    let parser = parser.finish();
    for event in parser {
        match event {
            Ok(event) => events.push(event),
            Err(err) => {
                dbg!(&err);
                panic!("Error while parsing: {}", err);
            }
        }
    }
}
