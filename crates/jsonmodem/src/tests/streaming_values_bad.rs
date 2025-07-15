use alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};

use crate::{
    ParserOptions, StreamingValue, StreamingValuesParser, Value, options::NonScalarValueMode,
    value::Map,
};

#[test]
fn error_empty_document() {
    let mut parser = StreamingValuesParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    parser.feed("").expect("feed failed");
    let err = parser.finish().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid end of input");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 1);
}

#[test]
fn error_comment() {
    let mut parser = StreamingValuesParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    let err = parser.feed("/").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '/' at 1:1");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 1);
}

#[test]
fn error_invalid_characters_in_values() {
    let mut parser = StreamingValuesParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    let err = parser.feed("a").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'a' at 1:1");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 1);
}

#[test]
fn error_invalid_property_name() {
    let mut parser = StreamingValuesParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    let err = parser.feed("{\\a:1}").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '\\\\' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn error_escaped_property_names() {
    let mut parser = StreamingValuesParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    // Hex escapes not accepted as property names
    assert!(
        parser
            .feed("{\\u0061\\u0062:1,\\u0024\\u005F:2,\\u005F\\u0024:3}")
            .is_err()
    );
}

#[test]
fn error_invalid_identifier_start_characters() {
    let mut parser = StreamingValuesParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });

    let err = parser.feed("{\\u0021:1}").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '\\\\' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn error_invalid_characters_following_sign() {
    let mut parser = StreamingValuesParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    let err = parser.feed("-a").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'a' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn error_invalid_characters_following_exponent_indicator() {
    let mut parser = StreamingValuesParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    let err = parser.feed("1ea").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'a' at 1:3");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 3);
}

#[test]
fn error_invalid_characters_following_exponent_sign() {
    let mut parser = StreamingValuesParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    let err = parser.feed("1e-a").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'a' at 1:4");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 4);
}

#[test]
fn error_invalid_new_lines_in_strings() {
    let mut parser = StreamingValuesParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    let err = parser.feed("\"\n\"").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '\\n' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn error_invalid_identifier_in_property_names() {
    let mut parser = StreamingValuesParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    let err = parser.feed("{!:1}").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '!' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn error_invalid_characters_following_array_value() {
    let mut parser = StreamingValuesParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    let err = parser.feed("[1!]").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '!' at 1:3");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 3);
}

#[test]
fn error_invalid_characters_in_literals() {
    let mut parser = StreamingValuesParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    let err = parser.feed("tru!").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '!' at 1:4");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 4);
}

#[test]
fn error_unterminated_escapes() {
    let mut parser = StreamingValuesParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    parser.feed("\"\\").expect("feed failed");
    let err = parser.finish().unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid end of input");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 3);
}

#[test]
fn error_invalid_first_digits_in_hexadecimal_escapes() {
    let mut parser = StreamingValuesParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    let err = parser.feed("\"\\xg\"").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'x' at 1:3");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 3);
}

#[test]
fn error_invalid_second_digits_in_hexadecimal_escapes() {
    let mut parser = StreamingValuesParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    let err = parser.feed("\"\\x0g\"").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'x' at 1:3");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 3);
}

#[test]
fn error_invalid_unicode_escapes() {
    let mut parser = StreamingValuesParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    let err = parser.feed("\"\\u000g\"").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'g' at 1:7");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 7);
}

// Escaped digits 1–9
#[test]
fn error_escaped_digit_1_to_9() {
    for i in 1..=9 {
        let mut parser = StreamingValuesParser::new(ParserOptions {
            non_scalar_values: NonScalarValueMode::All,
            ..Default::default()
        });
        let s = format!("\"\\{i}\"");
        let err = parser.feed(&s).unwrap_err();
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
    let mut parser = StreamingValuesParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    let err = parser.feed("\"\\01\"").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '0' at 1:3");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 3);
}

#[test]
fn error_multiple_values() {
    let mut parser = StreamingValuesParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    let err = parser.feed("1 2").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '2' at 1:3");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 3);
}

#[test]
fn error_control_characters_escaped_in_message() {
    let mut parser = StreamingValuesParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    let err = parser.feed("\x01").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '\\u0001' at 1:1");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 1);
}

#[test]
fn unclosed_objects_before_property_names() {
    let mut parser = StreamingValuesParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    let vals = parser.feed("{").expect("feed failed");
    assert_eq!(
        vals,
        vec![StreamingValue {
            index: 0,
            value: Value::Object(Map::new()),
            is_final: false,
        }]
    );
}

#[test]
fn unclosed_objects_after_property_names() {
    let mut parser = StreamingValuesParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    let vals = parser.feed("{\"a\"").expect("feed failed");
    assert_eq!(
        vals,
        vec![StreamingValue {
            index: 0,
            value: Value::Object(Map::new()),
            is_final: false,
        }]
    );
}

#[test]
fn error_unclosed_objects_before_property_values() {
    let mut parser = StreamingValuesParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    let err = parser.feed("{a:").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'a' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn error_unclosed_objects_after_property_values() {
    let mut parser = StreamingValuesParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    let err = parser.feed("{a:1").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'a' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn unclosed_arrays_before_values() {
    let mut parser = StreamingValuesParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    let vals = parser.feed("[").expect("feed failed");
    assert_eq!(
        vals,
        vec![StreamingValue {
            index: 0,
            value: Value::Array(Vec::new()),
            is_final: false,
        }]
    );
}

#[test]
fn unclosed_arrays_after_values() {
    let mut parser = StreamingValuesParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    let vals = parser.feed("[").expect("feed failed");
    assert_eq!(
        vals,
        vec![StreamingValue {
            index: 0,
            value: Value::Array(Vec::new()),
            is_final: false,
        }]
    );
}

#[test]
fn error_number_with_leading_zero() {
    let mut parser = StreamingValuesParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    let err = parser.feed("0x").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'x' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn error_nan() {
    let mut parser = StreamingValuesParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    let err = parser.feed("NaN").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'N' at 1:1");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 1);
}

#[test]
fn error_infinity() {
    let mut parser = StreamingValuesParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    let err = parser.feed("[Infinity,-Infinity]").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character 'I' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn error_leading_decimal_points() {
    let mut parser = StreamingValuesParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    let err = parser.feed("[.1,.23]").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '.' at 1:2");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 2);
}

#[test]
fn error_trailing_decimal_points() {
    let mut parser = StreamingValuesParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    let err = parser.feed("[0.]").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character ']' at 1:4");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 4);
}

#[test]
fn error_leading_plus_in_number() {
    let mut parser = StreamingValuesParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    let err = parser.feed("+1.23e100").unwrap_err();
    assert_eq!(err.to_string(), "JSON5: invalid character '+' at 1:1");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 1);
}

#[test]
fn error_incorrectly_completed_partial_string() {
    let mut parser = StreamingValuesParser::new(ParserOptions {
        non_scalar_values: NonScalarValueMode::All,
        ..Default::default()
    });
    let vals = parser.feed("\"abc").expect("feed failed");
    assert_eq!(
        vals,
        vec![StreamingValue {
            index: 0,
            value: Value::String(String::from("abc")),
            is_final: false,
        }]
    );
    let err = parser.feed("\"{}").unwrap_err();
    // error: invalid character '{' at position 6 of the stream
    assert_eq!(err.to_string(), "JSON5: invalid character '{' at 1:6");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 6);
}

#[test]
fn error_incorrectly_completed_partial_string_with_suffixes() {
    for &suffix in &["null", "\"", "1", "true", "{}", "[]"] {
        let mut parser = StreamingValuesParser::new(ParserOptions {
            non_scalar_values: NonScalarValueMode::All,
            ..Default::default()
        });
        let vals = parser.feed("\"abc").expect("feed failed");
        assert_eq!(
            vals,
            vec![StreamingValue {
                index: 0,
                value: Value::String(String::from("abc")),
                is_final: false,
            }]
        );
        let error_char = if suffix == "\"" {
            "\\\""
        } else {
            &suffix[0..1]
        };
        let err = parser.feed(&format!("\"{suffix}")).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!("JSON5: invalid character '{error_char}' at 1:6")
        );
        assert_eq!(err.line, 1);
        assert_eq!(err.column, 6);
    }
}
