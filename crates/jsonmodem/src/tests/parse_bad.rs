use alloc::{collections::BTreeMap, format, string::String, string::ToString, vec, vec::Vec};
use crate::parser::{ParserOptions, StreamingParserImpl, ParseEvent};
type DefaultStreamingParser = StreamingParserImpl<crate::backend::RustContext>;

// Minimal Value type for reconstructing results in a few tests
#[derive(Clone, Debug, PartialEq)]
enum Value {
    Null,
    Boolean(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Object(BTreeMap<alloc::sync::Arc<str>, Value>),
}
type Map = BTreeMap<alloc::sync::Arc<str>, Value>;

fn insert_at_path(target: &mut Value, path: &[crate::PathItem], val: Value) {
    if path.is_empty() {
        *target = val; return;
    }
    let mut cur = target;
    for comp in &path[..path.len()-1] {
        match comp {
            crate::PathItem::Key(k) => {
                if let Value::Object(map) = cur {
                    cur = map.entry(k.clone()).or_insert(Value::Null);
                } else {
                    *cur = Value::Object(Map::new());
                    if let Value::Object(map) = cur { cur = map.entry(k.clone()).or_insert(Value::Null); }
                }
            }
            crate::PathItem::Index(i) => {
                let i = *i;
                if let Value::Array(vec) = cur {
                    if i >= vec.len() { vec.resize(i+1, Value::Null); }
                    cur = &mut vec[i];
                } else {
                    *cur = Value::Array(Vec::new());
                    if let Value::Array(vec) = cur {
                        if i >= vec.len() { vec.resize(i+1, Value::Null); }
                        cur = &mut vec[i];
                    }
                }
            }
        }
    }
    match path.last().unwrap() {
        crate::PathItem::Key(k) => {
            if let Value::Object(map) = cur { map.insert(k.clone(), val); }
            else { let mut m = Map::new(); m.insert(k.clone(), val); *cur = Value::Object(m); }
        }
        crate::PathItem::Index(i) => {
            let i = *i;
            if let Value::Array(vec) = cur {
                if i >= vec.len() { vec.resize(i+1, Value::Null); }
                vec[i] = val;
            } else {
                let mut v = Vec::new();
                if i >= v.len() { v.resize(i+1, Value::Null); }
                v[i] = val; *cur = Value::Array(v);
            }
        }
    }
}

fn append_string_at_path(target: &mut Value, path: &[crate::PathItem], fragment: &str) {
    if path.is_empty() {
        if let Value::String(s) = target { s.push_str(fragment); } else { *target = Value::String(fragment.into()); }
        return;
    }
    let mut cur = target;
    for comp in &path[..path.len()-1] {
        match comp {
            crate::PathItem::Key(k) => {
                if let Value::Object(map) = cur { cur = map.entry(k.clone()).or_insert(Value::Null); }
                else { *cur = Value::Object(Map::new()); if let Value::Object(map) = cur { cur = map.entry(k.clone()).or_insert(Value::Null); } }
            }
            crate::PathItem::Index(i) => {
                let i = *i;
                if let Value::Array(vec) = cur { if i >= vec.len() { vec.resize(i+1, Value::Null); } cur = &mut vec[i]; }
                else { *cur = Value::Array(Vec::new()); if let Value::Array(vec) = cur { if i >= vec.len() { vec.resize(i+1, Value::Null); } cur = &mut vec[i]; } }
            }
        }
    }
    match path.last().unwrap() {
        crate::PathItem::Key(k) => {
            if let Value::Object(map) = cur {
                if let Some(Value::String(s)) = map.get_mut(k) { s.push_str(fragment); }
                else { map.insert(k.clone(), Value::String(fragment.into())); }
            } else { let mut m = Map::new(); m.insert(k.clone(), Value::String(fragment.into())); *cur = Value::Object(m); }
        }
        crate::PathItem::Index(i) => {
            let i = *i;
            if let Value::Array(vec) = cur {
                if i < vec.len() {
                    if let Value::String(s) = &mut vec[i] { s.push_str(fragment); } else { vec[i] = Value::String(fragment.into()); }
                } else { vec.resize(i+1, Value::Null); vec[i] = Value::String(fragment.into()); }
            } else { let mut v = Vec::new(); if i >= v.len() { v.resize(i+1, Value::Null); } v[i] = Value::String(fragment.into()); *cur = Value::Array(v); }
        }
    }
}

fn reconstruct_values<'a>(
    events: Vec<ParseEvent<'a, crate::backend::RustContext>>,
) -> Vec<Value> {
    let mut finished = Vec::new();
    let mut root = Value::Null;
    let mut building = false;
    for evt in events {
        match evt {
            ParseEvent::ArrayBegin { path } => { insert_at_path(&mut root, &path, Value::Array(Vec::new())); if path.is_empty() { building = true; } }
            ParseEvent::ObjectBegin { path } => { insert_at_path(&mut root, &path, Value::Object(Map::new())); if path.is_empty() { building = true; } }
            ParseEvent::Null { path } => { insert_at_path(&mut root, &path, Value::Null); if path.is_empty() { finished.push(Value::Null); root = Value::Null; building = false; } }
            ParseEvent::Boolean { path, value } => { insert_at_path(&mut root, &path, Value::Boolean(value)); if path.is_empty() { finished.push(Value::Boolean(value)); root = Value::Null; building = false; } }
            ParseEvent::Number { path, value } => { insert_at_path(&mut root, &path, Value::Number(value)); if path.is_empty() { finished.push(Value::Number(value)); root = Value::Null; building = false; } }
            ParseEvent::String { path, fragment, is_final, .. } => {
                append_string_at_path(&mut root, &path, &fragment);
                if is_final && path.is_empty() { finished.push(root.clone()); root = Value::Null; building = false; } else if path.is_empty() { building = true; }
            }
            ParseEvent::ArrayEnd { path } | ParseEvent::ObjectEnd { path } => { if path.is_empty() && building { finished.push(root.clone()); root = Value::Null; building = false; } }
        }
    }
    if building { finished.push(root); }
    finished
}

fn assert_err_contains(err: crate::parser::ParserError<crate::backend::RustContext>, expected_sub: &str, line: usize, col: usize) {
    let s = err.to_string();
    assert!(s.contains(expected_sub), "expected substring {expected_sub:?} in {s:?}");
    assert_eq!(err.line, line);
    assert_eq!(err.column, col);
}

#[test]
fn error_empty_document() {
    let parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.finish().last().unwrap().unwrap_err();
    assert_err_contains(err, "unexpected end of input", 1, 1);
}

#[test]
fn error_comment() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("/").last().unwrap().unwrap_err();
    assert_err_contains(err, "invalid character '/'", 1, 1);
}

#[test]
fn error_invalid_characters_in_values() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("a").last().unwrap().unwrap_err();
    assert_err_contains(err, "invalid character 'a'", 1, 1);
}

#[test]
fn error_invalid_property_name() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("{\\a:1}").last().unwrap().unwrap_err();
    assert_err_contains(err, "invalid character", 1, 2);
}

#[test]
fn error_escaped_property_names() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    // Hex escapes not accepted as property names
    assert!(
        parser
            .feed("{\\u0061\\u0062:1,\\u0024\\u005F:2,\\u005F\\u0024:3}")
            .last()
            .unwrap()
            .is_err()
    );
}

#[test]
fn error_invalid_identifier_start_characters() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());

    let err = parser.feed("{\\u0021:1}").last().unwrap().unwrap_err();
    assert_err_contains(err, "invalid character", 1, 2);
}

#[test]
fn error_invalid_characters_following_sign() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("-a").last().unwrap().unwrap_err();
    assert_err_contains(err, "invalid character 'a'", 1, 2);
}

#[test]
fn error_invalid_characters_following_exponent_indicator() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("1ea").last().unwrap().unwrap_err();
    assert_err_contains(err, "invalid character 'a'", 1, 3);
}

#[test]
fn error_invalid_characters_following_exponent_sign() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("1e-a").last().unwrap().unwrap_err();
    assert_err_contains(err, "invalid character 'a'", 1, 4);
}

#[test]
fn error_missing_exponent_digits_with_space() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("1e ").last().unwrap().unwrap_err();
    assert_err_contains(err, "invalid character ' '", 1, 3);
}

#[test]
fn error_missing_exponent_digits_with_sign() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("1e+ ").last().unwrap().unwrap_err();
    assert_err_contains(err, "invalid character ' '", 1, 4);
}

#[test]
fn error_invalid_new_lines_in_strings() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("\"\n\"").last().unwrap().unwrap_err();
    assert_err_contains(err, "invalid character", 1, 2);
}

#[test]
fn error_invalid_identifier_in_property_names() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("{!:1}").last().unwrap().unwrap_err();
    assert_err_contains(err, "invalid character '!'", 1, 2);
}

#[test]
fn error_invalid_characters_following_array_value() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("[1!]").last().unwrap().unwrap_err();
    assert_err_contains(err, "invalid character '!'", 1, 3);
}

#[test]
fn error_invalid_characters_in_literals() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("tru!").last().unwrap().unwrap_err();
    assert_err_contains(err, "invalid character '!'", 1, 4);
}

#[test]
fn error_unterminated_escapes() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    parser.feed("\"\\");
    let err = parser.finish().last().unwrap().unwrap_err();
    assert_err_contains(err, "unexpected end of input", 1, 3);
}

#[test]
fn error_invalid_first_digits_in_hexadecimal_escapes() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("\"\\xg\"").last().unwrap().unwrap_err();
    assert_err_contains(err, "invalid character 'x'", 1, 3);
}

#[test]
fn error_invalid_second_digits_in_hexadecimal_escapes() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("\"\\x0g\"").last().unwrap().unwrap_err();
    assert_err_contains(err, "invalid character 'x'", 1, 3);
}

#[test]
fn error_invalid_unicode_escapes() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("\"\\u000g\"").last().unwrap().unwrap_err();
    assert_err_contains(err, "invalid character 'g'", 1, 7);
}

// Escaped digits 1–9
#[test]
fn error_escaped_digit_1_to_9() {
    for i in 1..=9 {
        let mut parser = DefaultStreamingParser::new(ParserOptions::default());
        let s = format!("\"\\{i}\"");
        let err = parser.feed(&s).last().unwrap().unwrap_err();
        assert_err_contains(err, &format!("invalid character '{i}'"), 1, 3);
    }
}

#[test]
fn error_octal_escapes() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("\"\\01\"").last().unwrap().unwrap_err();
    assert_err_contains(err, "invalid character '0'", 1, 3);
}

#[test]
fn error_multiple_values() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("1 2").last().unwrap().unwrap_err();
    assert_err_contains(err, "invalid character '2'", 1, 3);
}

#[test]
fn error_control_characters_escaped_in_message() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("\x01").last().unwrap().unwrap_err();
    assert_err_contains(err, "invalid character", 1, 1);
}

#[test]
fn unclosed_objects_before_property_names() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let events: Vec<_> = parser.feed("{").map(Result::unwrap).collect();
    let vals = reconstruct_values(events);
    assert_eq!(vals, vec![Value::Object(Map::new())]);
}

#[test]
fn unclosed_objects_after_property_names() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let events: Vec<_> = parser.feed("{\"a\"").map(Result::unwrap).collect();
    let vals = reconstruct_values(events);
    assert_eq!(vals, vec![Value::Object(Map::new())]);
}

#[test]
fn error_unclosed_objects_before_property_values() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("{a:").last().unwrap().unwrap_err();
    assert_err_contains(err, "invalid character 'a'", 1, 2);
}

#[test]
fn error_unclosed_objects_after_property_values() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("{a:1").last().unwrap().unwrap_err();
    assert_err_contains(err, "invalid character 'a'", 1, 2);
}

#[test]
fn unclosed_arrays_before_values() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let events: Vec<_> = parser.feed("[").map(Result::unwrap).collect();
    let vals = reconstruct_values(events);
    assert_eq!(vals, vec![Value::Array(Vec::new())]);
}

#[test]
fn unclosed_arrays_after_values() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let events: Vec<_> = parser.feed("[").map(Result::unwrap).collect();
    let vals = reconstruct_values(events);
    assert_eq!(vals, vec![Value::Array(Vec::new())]);
}

#[test]
fn error_number_with_leading_zero() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("0x").last().unwrap().unwrap_err();
    assert_err_contains(err, "invalid character 'x'", 1, 2);
}

#[test]
fn error_nan() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("NaN").last().unwrap().unwrap_err();
    assert_err_contains(err, "invalid character 'N'", 1, 1);
}

#[test]
fn error_infinity() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser
        .feed("[Infinity,-Infinity]")
        .last()
        .unwrap()
        .unwrap_err();
    assert_err_contains(err, "invalid character 'I'", 1, 2);
}

#[test]
fn error_leading_decimal_points() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("[.1,.23]").last().unwrap().unwrap_err();
    assert_err_contains(err, "invalid character '.'", 1, 2);
}

#[test]
fn error_trailing_decimal_points() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("[0.]").last().unwrap().unwrap_err();
    assert_err_contains(err, "invalid character ']'", 1, 4);
}

#[test]
fn error_leading_plus_in_number() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("+1.23e100").last().unwrap().unwrap_err();
    assert_err_contains(err, "invalid character '+'", 1, 1);
}

#[test]
fn error_incorrectly_completed_partial_string() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let events: Vec<_> = parser.feed("\"abc").map(Result::unwrap).collect();
    let vals = reconstruct_values(events);
    assert_eq!(vals, vec![Value::String("abc".into())]);
    parser.feed("\"{}");
    let err = parser.finish().last().unwrap().unwrap_err();
    // error: invalid character '{' at position 6 of the stream
    assert_err_contains(err, "invalid character '{'", 1, 6);
}

#[test]
fn error_incorrectly_completed_partial_string_with_suffixes() {
    for &suffix in &["null", "\"", "1", "true", "{}", "[]"] {
        let mut parser = DefaultStreamingParser::new(ParserOptions::default());
        let events: Vec<_> = parser.feed("\"abc").map(Result::unwrap).collect();
        let vals = reconstruct_values(events);
        assert_eq!(vals, vec![Value::String("abc".into())]);
        let _error_char = if suffix == "\"" {
            "\\\""
        } else {
            &suffix[0..1]
        };
        let err = parser
            .feed(&format!("\"{suffix}"))
            .last()
            .unwrap()
            .unwrap_err();
        assert_err_contains(err, "invalid character", 1, 6);
    }
}
