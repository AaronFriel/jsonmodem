//! Repro cases for multi-value round-trip failures in streaming parser
use alloc::{vec, vec::Vec};

use crate::{ParseEvent, ParserOptions, StreamingParser, Value, event::reconstruct_values};

fn feed_and_reconstruct(payload: &str) -> (Vec<ParseEvent>, Vec<Value>) {
    let mut parser = StreamingParser::new(ParserOptions {
        allow_multiple_json_values: true,
        emit_non_scalar_values: true,
        panic_on_error: true,
        ..Default::default()
    });
    // Feed the payload and collect events
    let mut events = parser.feed_todo_remove_me(payload).expect("feed failed");
    events.extend(parser.finish_todo_remove_me("").expect("finish failed"));
    (events.clone(), reconstruct_values(events))
}

#[test]
fn repro_multi_value_null_root() {
    let (_, values) = feed_and_reconstruct("null");
    assert_eq!(values, vec![Value::Null], "unexpected reconstructed values");
}

#[test]
fn repro_multi_value_string_roots() {
    let (events, values) = feed_and_reconstruct("\"a\" \"b\"");
    assert_eq!(
        events,
        vec![
            ParseEvent::String {
                path: vec![],
                fragment: "a".into(),
                is_final: true,
                value: None,
            },
            ParseEvent::String {
                path: vec![],
                fragment: "b".into(),
                value: None,
                is_final: true,
            },
        ],
    );

    assert_eq!(
        values,
        vec![Value::String("a".into()), Value::String("b".into())],
        "unexpected reconstructed values"
    );
}

#[test]
fn repro_multi_value_boolean_roots() {
    let (_, values) = feed_and_reconstruct("true false");
    assert_eq!(
        values,
        vec![Value::Boolean(true), Value::Boolean(false)],
        "unexpected reconstructed values"
    );
}

#[test]
fn repro_multi_value_number_roots() {
    let (_, values) = feed_and_reconstruct("1 2.0");
    assert_eq!(
        values,
        vec![Value::Number(1.0), Value::Number(2.0)],
        "unexpected reconstructed values"
    );
}

// Inspect parsing of a composite root with an embedded space in string.
#[test]
fn inspect_composite_root() {
    let payload = "[\"a b\",null]";
    let (_, values) = feed_and_reconstruct(payload);
    // Expect one array with two elements: the string with space and null.
    assert_eq!(
        values,
        vec![Value::Array(vec![Value::String("a b".into()), Value::Null]),],
        "composite root reconstruction failed"
    );
}
