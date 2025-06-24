use alloc::{format, string::ToString, vec::Vec};

use crate::{ParserOptions, StreamingParser, Value, value::Map};

// Helper to feed input and extract current Value (including non-scalar roots).
fn current_value(parser: &StreamingParser) -> Option<Value> {
    parser.current_value()
}

#[test]
fn error_empty_document() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    let err = parser.finish_todo_remove_me("").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid end of input");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 1);
}

#[test]
fn error_comment() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    let err = parser.feed_todo_remove_me("/").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '/' at 1:1");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 1);
}

#[test]
fn error_invalid_characters_in_values() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    let err = parser.feed_todo_remove_me("a").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'a' at 1:1");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 1);
}

#[test]
fn error_invalid_property_name() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    let err = parser.feed_todo_remove_me("{\\a:1}").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '\\\\' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn error_escaped_property_names() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    // Hex escapes not accepted as property names
    assert!(
        parser
            .finish_todo_remove_me("{\\u0061\\u0062:1,\\u0024\\u005F:2,\\u005F\\u0024:3}")
            .is_err()
    );
}

#[test]
fn error_invalid_identifier_start_characters() {
    let mut parser = StreamingParser::new(ParserOptions::default());

    let err = parser.feed_todo_remove_me("{\\u0021:1}").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '\\\\' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn error_invalid_characters_following_sign() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    let err = parser.feed_todo_remove_me("-a").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'a' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn error_invalid_characters_following_exponent_indicator() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    let err = parser.feed_todo_remove_me("1ea").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'a' at 1:3");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 3);
}

#[test]
fn error_invalid_characters_following_exponent_sign() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    let err = parser.feed_todo_remove_me("1e-a").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'a' at 1:4");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 4);
}

#[test]
fn error_invalid_new_lines_in_strings() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    let err = parser.feed_todo_remove_me("\"\n\"").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '\\n' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn error_invalid_identifier_in_property_names() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    let err = parser.feed_todo_remove_me("{!:1}").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '!' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn error_invalid_characters_following_array_value() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    let err = parser.feed_todo_remove_me("[1!]").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '!' at 1:3");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 3);
}

#[test]
fn error_invalid_characters_in_literals() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    let err = parser.feed_todo_remove_me("tru!").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '!' at 1:4");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 4);
}

#[test]
fn error_unterminated_escapes() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    let err = parser.finish_todo_remove_me("\"\\").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid end of input");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 3);
}

#[test]
fn error_invalid_first_digits_in_hexadecimal_escapes() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    let err = parser.feed_todo_remove_me("\"\\xg\"").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'x' at 1:3");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 3);
}

#[test]
fn error_invalid_second_digits_in_hexadecimal_escapes() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    let err = parser.feed_todo_remove_me("\"\\x0g\"").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'x' at 1:3");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 3);
}

#[test]
fn error_invalid_unicode_escapes() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    let err = parser.feed_todo_remove_me("\"\\u000g\"").unwrap_err();
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
        let err = parser.feed_todo_remove_me(&s).unwrap_err();
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
    let err = parser.feed_todo_remove_me("\"\\01\"").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '0' at 1:3");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 3);
}

#[test]
fn error_multiple_values() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    let err = parser.feed_todo_remove_me("1 2").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '2' at 1:3");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 3);
}

#[test]
fn error_control_characters_escaped_in_message() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    let err = parser.feed_todo_remove_me("\x01").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '\\u0001' at 1:1");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 1);
}

#[test]
fn unclosed_objects_before_property_names() {
    let mut parser = StreamingParser::new(ParserOptions {
        emit_non_scalar_values: true,
        ..Default::default()
    });
    parser.feed_todo_remove_me("{").unwrap();
    assert_eq!(current_value(&parser), Some(Value::Object(Map::new())));
}

#[test]
fn unclosed_objects_after_property_names() {
    let mut parser = StreamingParser::new(ParserOptions {
        emit_non_scalar_values: true,
        ..Default::default()
    });
    parser.feed_todo_remove_me("{\"a\"").unwrap();
    assert_eq!(current_value(&parser), Some(Value::Object(Map::new())));
}

#[test]
fn error_unclosed_objects_before_property_values() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    let err = parser.feed_todo_remove_me("{a:").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'a' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn error_unclosed_objects_after_property_values() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    let err = parser.feed_todo_remove_me("{a:1").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'a' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn unclosed_arrays_before_values() {
    let mut parser = StreamingParser::new(ParserOptions {
        emit_non_scalar_values: true,
        ..Default::default()
    });
    parser.feed_todo_remove_me("[").unwrap();
    assert_eq!(current_value(&parser), Some(Value::Array(Vec::new())));
}

#[test]
fn unclosed_arrays_after_values() {
    let mut parser = StreamingParser::new(ParserOptions {
        emit_non_scalar_values: true,
        ..Default::default()
    });
    parser.feed_todo_remove_me("[").unwrap();
    assert_eq!(current_value(&parser), Some(Value::Array(Vec::new())));
}

#[test]
fn error_number_with_leading_zero() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    let err = parser.feed_todo_remove_me("0x").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'x' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn error_nan() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    let err = parser.feed_todo_remove_me("NaN").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'N' at 1:1");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 1);
}

#[test]
fn error_infinity() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    let err = parser
        .feed_todo_remove_me("[Infinity,-Infinity]")
        .unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'I' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn error_leading_decimal_points() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    let err = parser.feed_todo_remove_me("[.1,.23]").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '.' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn error_trailing_decimal_points() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    let err = parser.feed_todo_remove_me("[0.]").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character ']' at 1:4");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 4);
}

#[test]
fn error_leading_plus_in_number() {
    let mut parser = StreamingParser::new(ParserOptions::default());
    let err = parser.feed_todo_remove_me("+1.23e100").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '+' at 1:1");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 1);
}

#[test]
fn error_incorrectly_completed_partial_string() {
    let mut parser = StreamingParser::new(ParserOptions {
        emit_non_scalar_values: true,
        ..Default::default()
    });
    parser.feed_todo_remove_me("\"abc").unwrap();
    assert_eq!(current_value(&parser), Some(Value::String("abc".into())));
    let err = parser.finish_todo_remove_me("\"{}").unwrap_err();
    // error: invalid character '{' at position 6 of the stream
    assert_eq!(err.to_string(), "JSON5: invalid character '{' at 1:6");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 6);
}

#[test]
fn error_incorrectly_completed_partial_string_with_suffixes() {
    for &suffix in &["null", "\"", "1", "true", "{}", "[]"] {
        let mut parser = StreamingParser::new(ParserOptions {
            emit_non_scalar_values: true,
            ..Default::default()
        });
        parser.feed_todo_remove_me("\"abc").unwrap();
        assert_eq!(current_value(&parser), Some(Value::String("abc".into())));
        let error_char = if suffix == "\"" {
            "\\\""
        } else {
            &suffix[0..1]
        };
        let err = parser
            .feed_todo_remove_me(&format!("\"{suffix}"))
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            format!("JSON5: invalid character '{error_char}' at 1:6")
        );
        assert_eq!(err.line, 1);
        assert_eq!(err.column, 6);
    }
}
