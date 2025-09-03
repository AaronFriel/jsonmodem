use alloc::{collections::BTreeMap, string::String, sync::Arc, vec, vec::Vec};
use std::println;

use crate::parser::{ParseEvent, ParserOptions, StreamingParserImpl};
type DefaultStreamingParser = StreamingParserImpl<crate::backend::RustContext>;

// Minimal Value type for test reconstruction.
#[derive(Clone, Debug, PartialEq)]
enum Value {
    Null,
    Boolean(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Object(BTreeMap<Arc<str>, Value>),
}

type Map = BTreeMap<Arc<str>, Value>;

fn insert_at_path(target: &mut Value, path: &[crate::PathItem], val: Value) {
    if path.is_empty() {
        *target = val;
        return;
    }

    let mut current = target;
    for comp in &path[..path.len() - 1] {
        match comp {
            crate::PathItem::Key(k) => {
                if let Value::Object(map) = current {
                    current = map.entry(k.clone()).or_insert(Value::Null);
                } else {
                    *current = Value::Object(Map::new());
                    if let Value::Object(map) = current {
                        current = map.entry(k.clone()).or_insert(Value::Null);
                    }
                }
            }
            crate::PathItem::Index(i) => {
                let i = *i;
                if let Value::Array(vec) = current {
                    if i >= vec.len() {
                        vec.resize(i + 1, Value::Null);
                    }
                    current = &mut vec[i];
                } else {
                    *current = Value::Array(Vec::new());
                    if let Value::Array(vec) = current {
                        if i >= vec.len() {
                            vec.resize(i + 1, Value::Null);
                        }
                        current = &mut vec[i];
                    }
                }
            }
        }
    }

    match path.last().unwrap() {
        crate::PathItem::Key(k) => {
            if let Value::Object(map) = current {
                map.insert(k.clone(), val);
            } else {
                let mut map = Map::new();
                map.insert(k.clone(), val);
                *current = Value::Object(map);
            }
        }
        crate::PathItem::Index(i) => {
            let i = *i;
            if let Value::Array(vec) = current {
                if i >= vec.len() {
                    vec.resize(i + 1, Value::Null);
                }
                vec[i] = val;
            } else {
                let mut vec = Vec::new();
                if i >= vec.len() {
                    vec.resize(i + 1, Value::Null);
                }
                vec[i] = val;
                *current = Value::Array(vec);
            }
        }
    }
}

fn append_string_at_path(target: &mut Value, path: &[crate::PathItem], fragment: &str) {
    if path.is_empty() {
        if let Value::String(s) = target {
            s.push_str(fragment);
        } else {
            *target = Value::String(fragment.into());
        }
        return;
    }

    let mut current = target;
    for comp in &path[..path.len() - 1] {
        match comp {
            crate::PathItem::Key(k) => {
                if let Value::Object(map) = current {
                    current = map.entry(k.clone()).or_insert(Value::Null);
                } else {
                    *current = Value::Object(Map::new());
                    if let Value::Object(map) = current {
                        current = map.entry(k.clone()).or_insert(Value::Null);
                    }
                }
            }
            crate::PathItem::Index(i) => {
                let i = *i;
                if let Value::Array(vec) = current {
                    if i >= vec.len() {
                        vec.resize(i + 1, Value::Null);
                    }
                    current = &mut vec[i];
                } else {
                    *current = Value::Array(Vec::new());
                    if let Value::Array(vec) = current {
                        if i >= vec.len() {
                            vec.resize(i + 1, Value::Null);
                        }
                        current = &mut vec[i];
                    }
                }
            }
        }
    }

    match path.last().unwrap() {
        crate::PathItem::Key(k) => {
            if let Value::Object(map) = current {
                if let Some(Value::String(s)) = map.get_mut(k) {
                    s.push_str(fragment);
                } else {
                    map.insert(k.clone(), Value::String(fragment.into()));
                }
            } else {
                let mut map = Map::new();
                map.insert(k.clone(), Value::String(fragment.into()));
                *current = Value::Object(map);
            }
        }
        crate::PathItem::Index(i) => {
            let i = *i;
            if let Value::Array(vec) = current {
                if i < vec.len() {
                    if let Value::String(s) = &mut vec[i] {
                        s.push_str(fragment);
                    } else {
                        vec[i] = Value::String(fragment.into());
                    }
                } else {
                    vec.resize(i + 1, Value::Null);
                    vec[i] = Value::String(fragment.into());
                }
            } else {
                let mut vec = Vec::new();
                if i >= vec.len() {
                    vec.resize(i + 1, Value::Null);
                }
                vec[i] = Value::String(fragment.into());
                *current = Value::Array(vec);
            }
        }
    }
}

fn reconstruct_values<'a>(
    events: Vec<ParseEvent<'a, crate::backend::RustContext>>,
) -> Vec<Value> {
    let mut finished_roots = Vec::new();
    let mut current_root = Value::Null;
    let mut building_root = false;

    for evt in events {
        println!("event: {:?}", &evt);
        match evt {
            ParseEvent::ArrayBegin { path } => {
                insert_at_path(&mut current_root, &path, Value::Array(Vec::new()));
                if path.is_empty() {
                    building_root = true;
                }
            }
            ParseEvent::ObjectBegin { path } => {
                insert_at_path(&mut current_root, &path, Value::Object(Map::new()));
                if path.is_empty() {
                    building_root = true;
                }
            }
            ParseEvent::Null { path } => {
                insert_at_path(&mut current_root, &path, Value::Null);
                if path.is_empty() {
                    finished_roots.push(Value::Null);
                    current_root = Value::Null;
                    building_root = false;
                }
            }
            ParseEvent::Boolean { path, value } => {
                insert_at_path(&mut current_root, &path, Value::Boolean(value));
                if path.is_empty() {
                    finished_roots.push(Value::Boolean(value));
                    current_root = Value::Null;
                    building_root = false;
                }
            }
            ParseEvent::Number { path, value } => {
                insert_at_path(&mut current_root, &path, Value::Number(value));
                if path.is_empty() {
                    finished_roots.push(Value::Number(value));
                    current_root = Value::Null;
                    building_root = false;
                }
            }
            ParseEvent::String { path, fragment, is_final, .. } => {
                append_string_at_path(&mut current_root, &path, &fragment);
                if is_final && path.is_empty() {
                    finished_roots.push(current_root.clone());
                    current_root = Value::Null;
                    building_root = false;
                } else if path.is_empty() {
                    building_root = true;
                }
            }
            ParseEvent::ArrayEnd { path } | ParseEvent::ObjectEnd { path } => {
                if path.is_empty() && building_root {
                    finished_roots.push(current_root.clone());
                    current_root = Value::Null;
                    building_root = false;
                }
            }
        }
    }

    if building_root {
        finished_roots.push(current_root);
    }
    finished_roots
}

/// Helper to feed JSON chunks and return the final `Value`.
///
/// The core parser emits low-overhead `ParseEvent`s; tests reconstruct the
/// materialized `Value` tree from the event stream for verification.
fn finish_seq(chunks: &[&str]) -> Value {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let mut events = Vec::new();
    for &chunk in chunks {
        events.extend(parser.feed(chunk).map(|r| r.unwrap()));
    }
    events.extend(parser.finish().filter_map(Result::ok));
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
    // a JSON value, so we should not receive any events yet.
    let evts: Vec<_> = parser.feed("\"").map(Result::unwrap).collect();
    assert!(evts.is_empty());

    // Feed a backslash – still inside the string escape sequence, which is
    // incomplete at this point. Again, we must not observe any completed
    // events.
    let evts: Vec<_> = parser.feed("\\").map(Result::unwrap).collect();
    assert!(evts.is_empty());
}

#[test]
fn test_integer_split_across_feeds() {
    // Include a trailing delimiter in the second chunk to finalize the number during feed
    assert_eq!(finish_seq(&["-", "12 "]), Value::Number(-12.0));
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
    let v = finish_seq(&["{\"a\": 1 ", ", \"b\": [2", ",3]} "]);
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
