use alloc::{format, string::ToString, vec::Vec};

use crate::{ParserOptions, StreamingParser, Value, options::NonScalarValueMode, value::Map};

#[test]
fn error_empty_document() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    parser.feed("");
    let err = parser.finish().last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid end of input");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 1);
}

#[test]
fn error_comment() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    parser.feed("/");
    let err = parser.last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '/' at 1:1");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 1);
}

#[test]
fn error_invalid_characters_in_values() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    parser.feed("a");
    let err = parser.last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'a' at 1:1");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 1);
}

#[test]
fn error_invalid_property_name() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    parser.feed("{\\a:1}");
    let err = parser.last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '\\\\' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn error_escaped_property_names() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    parser.feed("{\\u0061\\u0062:1,\\u0024\\u005F:2,\\u005F\\u0024:3}");
    // Hex escapes not accepted as property names
    assert!(parser.last().unwrap().is_err());
}

#[test]
fn error_invalid_identifier_start_characters() {
    let mut parser = StreamingParser::new(ParserOptions::default());

    parser.feed("{\\u0021:1}");
    let err = parser.last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '\\\\' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn error_invalid_characters_following_sign() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    parser.feed("-a");
    let err = parser.last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'a' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn error_invalid_characters_following_exponent_indicator() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    parser.feed("1ea");
    let err = parser.last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'a' at 1:3");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 3);
}

#[test]
fn error_invalid_characters_following_exponent_sign() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    parser.feed("1e-a");
    let err = parser.last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'a' at 1:4");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 4);
}

#[test]
fn error_invalid_new_lines_in_strings() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    parser.feed("\"\n\"");
    let err = parser.last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '\\n' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn error_invalid_identifier_in_property_names() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    parser.feed("{!:1}");
    let err = parser.last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '!' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn error_invalid_characters_following_array_value() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    parser.feed("[1!]");
    let err = parser.last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '!' at 1:3");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 3);
}

#[test]
fn error_invalid_characters_in_literals() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    parser.feed("tru!");
    let err = parser.last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '!' at 1:4");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 4);
}

#[test]
fn error_unterminated_escapes() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    parser.feed("\"\\");
    let err = parser.finish().last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid end of input");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 3);
}

#[test]
fn error_invalid_first_digits_in_hexadecimal_escapes() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    parser.feed("\"\\xg\"");
    let err = parser.last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'x' at 1:3");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 3);
}

#[test]
fn error_invalid_second_digits_in_hexadecimal_escapes() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    parser.feed("\"\\x0g\"");
    let err = parser.last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'x' at 1:3");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 3);
}

#[test]
fn error_invalid_unicode_escapes() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    parser.feed("\"\\u000g\"");
    let err = parser.last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'g' at 1:7");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 7);
}

// Escaped digits 1–9
#[test]
fn error_escaped_digit_1_to_9() {
    for i in 1..=9 {
        let mut parser = StreamingParser::new(ParserOptions::default());
        let s = format!("\"\\{i}\"");
        parser.feed(&s);
        let err = parser.last().unwrap().unwrap_err();
        assert_eq!(
            err.to_string(),
            format!("JSON5: invalid character '{i}' at 1:3")
        );
        assert_eq!(err.line, 1);
        assert_eq!(err.column, 3);
    }
}

#[test]
fn error_octal_escapes() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    parser.feed("\"\\01\"");
    let err = parser.last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '0' at 1:3");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 3);
}

#[test]
fn error_multiple_values() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    parser.feed("1 2");
    let err = parser.last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '2' at 1:3");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 3);
}

#[test]
fn error_control_characters_escaped_in_message() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    parser.feed("\x01");
    let err = parser.last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '\\u0001' at 1:1");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 1);
}

#[test]
fn unclosed_objects_before_property_names() {
    let mut parser = StreamingParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    parser.feed("{");
    // Drive the parser to process the already-fed chunk so that the builder is
    // updated with the partial value.
    assert!(parser.by_ref().all(|r| r.is_ok()));
    assert_eq!(parser.current_value(), Some(Value::Object(Map::new())));
}

#[test]
fn unclosed_objects_after_property_names() {
    let mut parser = StreamingParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    parser.feed("{\"a\"");
    assert!(parser.by_ref().all(|r| r.is_ok()));
    assert_eq!(parser.current_value(), Some(Value::Object(Map::new())));
}

#[test]
fn error_unclosed_objects_before_property_values() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    parser.feed("{a:");
    let err = parser.last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'a' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn error_unclosed_objects_after_property_values() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    parser.feed("{a:1");
    let err = parser.last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'a' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn unclosed_arrays_before_values() {
    let mut parser = StreamingParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    parser.feed("[");
    assert!(parser.by_ref().all(|r| r.is_ok()));
    assert_eq!(parser.current_value(), Some(Value::Array(Vec::new())));
}

#[test]
fn unclosed_arrays_after_values() {
    let mut parser = StreamingParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    parser.feed("[");
    assert!(parser.by_ref().all(|r| r.is_ok()));
    assert_eq!(parser.current_value(), Some(Value::Array(Vec::new())));
}

#[test]
fn error_number_with_leading_zero() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    parser.feed("0x");
    let err = parser.last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'x' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn error_nan() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    parser.feed("NaN");
    let err = parser.last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'N' at 1:1");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 1);
}

#[test]
fn error_infinity() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    parser.feed("[Infinity,-Infinity]");
    let err = parser.last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'I' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn error_leading_decimal_points() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    parser.feed("[.1,.23]");
    let err = parser.last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '.' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn error_trailing_decimal_points() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    parser.feed("[0.]");
    let err = parser.last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character ']' at 1:4");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 4);
}

#[test]
fn error_leading_plus_in_number() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    parser.feed("+1.23e100");
    let err = parser.last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '+' at 1:1");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 1);
}

#[test]
fn error_incorrectly_completed_partial_string() {
    let mut parser = StreamingParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    parser.feed("\"abc");
    assert!(parser.by_ref().all(|r| r.is_ok()));
    assert_eq!(parser.current_value(), Some(Value::String("abc".into())));
    parser.feed("\"{}");
    let err = parser.finish().last().unwrap().unwrap_err();
    // error: invalid character '{' at position 6 of the stream
    assert_eq!(err.to_string(), "JSON5: invalid character '{' at 1:6");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 6);
}

#[test]
fn error_incorrectly_completed_partial_string_with_suffixes() {
    for &suffix in &["null", "\"", "1", "true", "{}", "[]"] {
        let mut parser = StreamingParser::new(ParserOptions {
            non_scalar_values: NonScalarValueMode::All,
            ..Default::default()
        });
        parser.feed("\"abc");
        assert!(parser.by_ref().all(|r| r.is_ok()));
        assert_eq!(parser.current_value(), Some(Value::String("abc".into())));
        let error_char = if suffix == "\"" {
            "\\\""
        } else {
            &suffix[0..1]
        };
        parser.feed(&format!("\"{suffix}"));
        let err = parser.last().unwrap().unwrap_err();
        assert_eq!(
            err.to_string(),
            format!("JSON5: invalid character '{error_char}' at 1:6")
        );
        assert_eq!(err.line, 1);
        assert_eq!(err.column, 6);
    }
}
