//! Repro cases for multi-value round-trip failures in streaming parser
use alloc::{string::String, vec, vec::Vec};

use crate::{
    backend::StdBackend,
    parser::{JsonModem, ParserOptions},
    test_util::reconstruct_values,
    value::Value,
};

type TestParser = JsonModem<StdBackend>;

fn feed_and_reconstruct(chunks: impl IntoIterator<Item = &'static str>) -> Vec<Value> {
    let input: String = chunks.into_iter().collect();

    let mut parser = TestParser::new(
        ParserOptions::default()
            .with_allow_multiple_json_values(true)
            .with_panic_on_error(true),
    );

    let mut events = parser
        .feed(&input)
        .to_iter()
        .collect::<Result<Vec<_>, _>>()
        .expect("feed should succeed");
    events.extend(
        parser
            .finish()
            .to_iter()
            .collect::<Result<Vec<_>, _>>()
            .expect("finish should succeed"),
    );
    reconstruct_values(events)
}

#[test]
fn repro_multi_value_null_root() {
    let values = feed_and_reconstruct(["null"]);
    assert_eq!(values, vec![Value::Null], "unexpected reconstructed values");
}

#[test]
fn repro_multi_value_string_roots() {
    let values = feed_and_reconstruct(["\"a\" ", "\"b\""]);
    assert_eq!(
        values,
        vec![Value::String("a".into()), Value::String("b".into())],
        "unexpected reconstructed values"
    );
}

#[test]
fn repro_multi_value_boolean_roots() {
    let values = feed_and_reconstruct(["true ", "false"]);
    assert_eq!(
        values,
        vec![Value::Boolean(true), Value::Boolean(false)],
        "unexpected reconstructed values"
    );
}

// Inspect parsing of a composite root with an embedded space in string.
#[test]
fn inspect_composite_root() {
    let payload = "[\"a b\",null]";
    let values = feed_and_reconstruct([payload]);
    // Expect one array with two elements: the string with space and null.
    assert_eq!(
        values,
        vec![Value::Array(vec![Value::String("a b".into()), Value::Null]),],
        "composite root reconstruction failed"
    );
}
