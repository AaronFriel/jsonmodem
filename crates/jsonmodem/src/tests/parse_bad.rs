use alloc::{format, string::ToString, vec::Vec};

use crate::{
    DefaultStreamingParser, ParserOptions, Value, options::NonScalarValueMode, value::Map,
};

#[test]
fn error_empty_document() {
    let parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.finish().last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid end of input");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 1);
}

#[test]
fn error_comment() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("/").last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '/' at 1:1");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 1);
}

#[test]
fn error_invalid_characters_in_values() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("a").last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'a' at 1:1");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 1);
}

#[test]
fn error_invalid_property_name() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("{\\a:1}").last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '\\\\' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
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
    assert_eq!(err.to_string(), "JSON5: invalid character '\\\\' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn error_invalid_characters_following_sign() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("-a").last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'a' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn error_invalid_characters_following_exponent_indicator() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("1ea").last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'a' at 1:3");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 3);
}

#[test]
fn error_invalid_characters_following_exponent_sign() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("1e-a").last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'a' at 1:4");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 4);
}

#[test]
fn error_missing_exponent_digits_with_space() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("1e ").last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character ' ' at 1:3");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 3);
}

#[test]
fn error_missing_exponent_digits_with_sign() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("1e+ ").last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character ' ' at 1:4");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 4);
}

#[test]
fn error_invalid_new_lines_in_strings() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("\"\n\"").last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '\\n' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn error_invalid_identifier_in_property_names() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("{!:1}").last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '!' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn error_invalid_characters_following_array_value() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("[1!]").last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '!' at 1:3");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 3);
}

#[test]
fn error_invalid_characters_in_literals() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("tru!").last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '!' at 1:4");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 4);
}

#[test]
fn error_unterminated_escapes() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    parser.feed("\"\\");
    let err = parser.finish().last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid end of input");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 3);
}

#[test]
fn error_invalid_first_digits_in_hexadecimal_escapes() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("\"\\xg\"").last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'x' at 1:3");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 3);
}

#[test]
fn error_invalid_second_digits_in_hexadecimal_escapes() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("\"\\x0g\"").last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'x' at 1:3");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 3);
}

#[test]
fn error_invalid_unicode_escapes() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("\"\\u000g\"").last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'g' at 1:7");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 7);
}

// Escaped digits 1–9
#[test]
fn error_escaped_digit_1_to_9() {
    for i in 1..=9 {
        let mut parser = DefaultStreamingParser::new(ParserOptions::default());
        let s = format!("\"\\{i}\"");
        let err = parser.feed(&s).last().unwrap().unwrap_err();
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
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("\"\\01\"").last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '0' at 1:3");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 3);
}

#[test]
fn error_multiple_values() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("1 2").last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '2' at 1:3");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 3);
}

#[test]
fn error_control_characters_escaped_in_message() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("\x01").last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '\\u0001' at 1:1");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 1);
}

#[test]
fn unclosed_objects_before_property_names() {
    let mut parser = DefaultStreamingParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    // Drive the parser to process the already-fed chunk so that the builder is
    // updated with the partial value.
    assert!(parser.feed("{").all(|r| r.is_ok()));
    assert_eq!(parser.current_value(), Some(Value::Object(Map::new())));
}

#[test]
fn unclosed_objects_after_property_names() {
    let mut parser = DefaultStreamingParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    assert!(parser.feed("{\"a\"").all(|r| r.is_ok()));
    assert_eq!(parser.current_value(), Some(Value::Object(Map::new())));
}

#[test]
fn error_unclosed_objects_before_property_values() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("{a:").last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'a' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn error_unclosed_objects_after_property_values() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("{a:1").last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'a' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn unclosed_arrays_before_values() {
    let mut parser = DefaultStreamingParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    assert!(parser.feed("[").all(|r| r.is_ok()));
    assert_eq!(parser.current_value(), Some(Value::Array(Vec::new())));
}

#[test]
fn unclosed_arrays_after_values() {
    let mut parser = DefaultStreamingParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    assert!(parser.feed("[").all(|r| r.is_ok()));
    assert_eq!(parser.current_value(), Some(Value::Array(Vec::new())));
}

#[test]
fn error_number_with_leading_zero() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("0x").last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'x' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn error_nan() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("NaN").last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'N' at 1:1");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 1);
}

#[test]
fn error_infinity() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser
        .feed("[Infinity,-Infinity]")
        .last()
        .unwrap()
        .unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'I' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn error_leading_decimal_points() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("[.1,.23]").last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '.' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn error_trailing_decimal_points() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("[0.]").last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character ']' at 1:4");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 4);
}

#[test]
fn error_leading_plus_in_number() {
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let err = parser.feed("+1.23e100").last().unwrap().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '+' at 1:1");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 1);
}

#[test]
fn error_incorrectly_completed_partial_string() {
    let mut parser = DefaultStreamingParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    assert!(parser.feed("\"abc").all(|r| r.is_ok()));
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
        let mut parser = DefaultStreamingParser::new(ParserOptions {
            non_scalar_values: NonScalarValueMode::All,
            ..Default::default()
        });
        assert!(parser.feed("\"abc").all(|r| r.is_ok()));
        assert_eq!(parser.current_value(), Some(Value::String("abc".into())));
        let error_char = if suffix == "\"" {
            "\\\""
        } else {
            &suffix[0..1]
        };
        let err = parser
            .feed(&format!("\"{suffix}"))
            .last()
            .unwrap()
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            format!("JSON5: invalid character '{error_char}' at 1:6")
        );
        assert_eq!(err.line, 1);
        assert_eq!(err.column, 6);
    }
}
