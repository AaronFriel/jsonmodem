use alloc::{vec, vec::Vec};

use crate::{
    DefaultStreamingParser, ParseEvent, Value,
    event::test_util::reconstruct_values,
    options::{NonScalarValueMode, ParserOptions},
    value::Map,
};

/// Helper to feed JSON chunks and return the final `Value`.
///
/// The core parser emits low-overhead `ParseEvent`s; tests reconstruct the
/// materialized `Value` tree from the event stream for verification.
fn finish_seq(chunks: &[&str]) -> Value {
    let mut parser = DefaultStreamingParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    for &chunk in chunks {
        parser.feed(chunk);
    }
    let events: Vec<_> = parser.finish().map(|r| r.unwrap()).collect();
    let mut vals = reconstruct_values(events);
    assert_eq!(vals.len(), 1, "expected one root value");
    vals.remove(0)
}

#[test]
fn test_empty_object() {
    assert_eq!(finish_seq(&["{}"]), Value::Object(Map::new()));
}

#[test]
fn test_single_property() {
    let mut map = Map::new();
    map.insert("a".into(), Value::Number(1.0));
    assert_eq!(finish_seq(&["{\"a\":1}"]), Value::Object(map));
}

#[test]
fn test_multiple_properties() {
    let mut map = Map::new();
    map.insert("abc".into(), Value::Number(1.0));
    map.insert("def".into(), Value::Number(2.0));
    assert_eq!(finish_seq(&["{\"abc\":1,\"def\":2}"]), Value::Object(map));
}

#[test]
fn test_nested_objects() {
    let mut inner = Map::new();
    inner.insert("b".into(), Value::Number(2.0));

    let mut outer = Map::new();
    outer.insert("a".into(), Value::Object(inner));

    assert_eq!(finish_seq(&["{\"a\":{\"b\":2}}"]), Value::Object(outer));
}

#[test]
fn test_arrays() {
    assert_eq!(finish_seq(&["[]"]), Value::Array(vec![]));
    assert_eq!(finish_seq(&["[1]"]), Value::Array(vec![Value::Number(1.0)]));
    assert_eq!(
        finish_seq(&["[1,2]"]),
        Value::Array(vec![Value::Number(1.0), Value::Number(2.0)])
    );
    assert_eq!(
        finish_seq(&["[1,[2,3]]"]),
        Value::Array(vec![
            Value::Number(1.0),
            Value::Array(vec![Value::Number(2.0), Value::Number(3.0)]),
        ])
    );
}

#[test]
fn test_literals() {
    assert_eq!(finish_seq(&["null"]), Value::Null);
    assert_eq!(finish_seq(&["true"]), Value::Boolean(true));
    assert_eq!(finish_seq(&["false"]), Value::Boolean(false));
}

#[test]
fn test_numbers() {
    assert_eq!(
        finish_seq(&["[-0]"]),
        Value::Array(vec![Value::Number(-0.0)])
    );

    assert_eq!(
        finish_seq(&["[1,23,456,7890]"]),
        Value::Array(vec![
            Value::Number(1.0),
            Value::Number(23.0),
            Value::Number(456.0),
            Value::Number(7890.0),
        ])
    );

    assert_eq!(
        finish_seq(&["[-1,-2,-0.1,-0]"]),
        Value::Array(vec![
            Value::Number(-1.0),
            Value::Number(-2.0),
            Value::Number(-0.1),
            Value::Number(-0.0),
        ])
    );

    assert_eq!(
        finish_seq(&["[1.0,1.23]"]),
        Value::Array(vec![Value::Number(1.0), Value::Number(1.23)])
    );

    assert_eq!(
        finish_seq(&["[1e0,1e-1,1e+1,1.1e0]"]),
        Value::Array(vec![
            Value::Number(1.0),
            Value::Number(0.1),
            Value::Number(10.0),
            Value::Number(1.1),
        ])
    );
}

#[test]
fn test_preserves_proto_property() {
    let mut map = Map::new();
    map.insert("__proto__".into(), Value::Number(1.0));
    assert_eq!(finish_seq(&["{\"__proto__\":1}"]), Value::Object(map));
}

#[test]
fn test_exponents_more_forms() {
    assert_eq!(
        finish_seq(&["[1e0,1e1,1e-1,1e+1,1.1e0]"]),
        Value::Array(vec![
            Value::Number(1.0),
            Value::Number(10.0),
            Value::Number(0.1),
            Value::Number(10.0),
            Value::Number(1.1),
        ])
    );
}

#[test]
fn test_partial_string_multiple_feeds() {
    assert_eq!(
        finish_seq(&["\"abc", "def", "ghi\""]),
        Value::String("abcdefghi".into())
    );
}

#[test]
fn test_continue_after_array_value() {
    assert_eq!(
        finish_seq(&["[\"1\"", ",\"2\"", "]"]),
        Value::Array(vec![Value::String("1".into()), Value::String("2".into())])
    );
}

#[test]
fn test_continue_within_array_value() {
    assert_eq!(
        finish_seq(&["[\"1\"", ",\"2", "3\"", ",4]"]),
        Value::Array(vec![
            Value::String("1".into()),
            Value::String("23".into()),
            Value::Number(4.0),
        ])
    );
}

#[test]
fn test_continue_string_with_escape() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());

    // Feed the opening quote of the string – this is not enough to complete
    // a JSON value, so we should not receive any events yet and `current_value`
    // must stay `None`.
    assert!(parser.feed("\"").all(|r| r.is_ok()));
    assert!(parser.current_value().is_none());

    // Feed a backslash – still inside the string escape sequence, which is
    // incomplete at this point. Again, we must not observe any completed
    // events and `current_value` should remain unset.
    assert!(parser.feed("\\").all(|r| r.is_ok()));
    assert!(parser.current_value().is_none());
}

#[test]
fn test_integer_split_across_feeds() {
    assert_eq!(finish_seq(&["-", "12"]), Value::Number(-12.0));
}

#[test]
fn test_strings_and_escapes() {
    assert_eq!(finish_seq(&["\"abc\""]), Value::String("abc".into()));

    assert_eq!(
        finish_seq(&["[\"\\\"\",\"'\"]"]),
        Value::Array(vec![Value::String("\"".into()), Value::String("'".into())])
    );

    assert_eq!(
        finish_seq(&["\"\\b\\f\\n\\r\\t\\u01FF\\\\\\\"\""]),
        Value::String("\x08\x0C\n\r\t\u{01FF}\\\"".into())
    );
}

#[test]
fn test_whitespace_inside() {
    assert_eq!(finish_seq(&["{\t\n  \r}\n"]), Value::Object(Map::new()));
}

#[test]
fn test_incremental_complete_after_three_feeds() {
    let v = finish_seq(&["{\"a\": 1", " , \"b\": [2", ",3]} "]);
    if let Value::Object(map) = v {
        assert_eq!(map.get("a"), Some(&Value::Number(1.0)));
    } else {
        panic!("expected object");
    }
}

#[test]
fn test_streaming_multiple_values() {
    let mut parser = DefaultStreamingParser::new(ParserOptions {
        allow_multiple_json_values: true,
        ..Default::default()
    });

    // First chunk – should yield exactly one number event with value `1`.
    let evts: Vec<_> = parser.feed("1 ").map(Result::unwrap).collect();
    let vals: Vec<_> = evts
        .into_iter()
        .filter_map(|ev| match ev {
            ParseEvent::Number { value, .. } => Some(value),
            _ => None,
        })
        .collect();
    assert_eq!(vals, vec![1.0]);

    // Second chunk – should yield exactly one number event with value `2`.
    let evts: Vec<_> = parser.feed(" 2 ").map(Result::unwrap).collect();
    let vals: Vec<_> = evts
        .into_iter()
        .filter_map(|ev| match ev {
            ParseEvent::Number { value, .. } => Some(value),
            _ => None,
        })
        .collect();
    assert_eq!(vals, vec![2.0]);

    // Third chunk – whitespace only, should not emit any events.
    let evts: Vec<_> = parser.feed("   ").map(Result::unwrap).collect();
    assert!(evts.is_empty());
}
