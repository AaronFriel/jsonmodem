//! Port of partial JSON parsing and repair routines from:
//!
//! <https://github.com/vercel/ai/blob/53ea87ab77de821c085e52a26cfa1c069ff3eb39/packages/ai/src/util/parse-partial-json.ts>
//! <https://github.com/vercel/ai/blob/53ea87ab77de821c085e52a26cfa1c069ff3eb39/packages/ai/src/util/fix-json.ts>

#![allow(clippy::enum_glob_use)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::cast_possible_wrap)]
#![cfg(feature = "comparison")]

use serde_json::Value as JsonValue;

/// The outcome of attempting to parse a (possibly partial) JSON string using
/// [`parse_partial_json`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseState {
    /// The input was `None` (undefined).
    UndefinedInput,
    /// The input was valid JSON – no repair was necessary.
    SuccessfulParse,
    /// The input was *not* valid JSON, but it became valid once it was passed
    /// through [`fix_json`].
    RepairedParse,
    /// The input could not be repaired into valid JSON.
    FailedParse,
}

/// Attempt to parse a *possibly* partial JSON snippet.
///
/// The algorithm mimics the behaviour of the TypeScript implementation that
/// first tries to parse the input directly, and only if that fails falls back
/// to a *single-pass* repair routine (`fix_json`).
#[must_use]
pub fn parse_partial_json(input: Option<&str>) -> (Option<JsonValue>, ParseState) {
    match input {
        None => (None, ParseState::UndefinedInput),
        Some(text) => {
            if let Ok(value) = serde_json::from_str::<JsonValue>(text) {
                (Some(value), ParseState::SuccessfulParse)
            } else {
                let fixed = fix_json(text);
                match serde_json::from_str::<JsonValue>(&fixed) {
                    Ok(value) => (Some(value), ParseState::RepairedParse),
                    Err(_) => (None, ParseState::FailedParse),
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum State {
    Root,
    Finish,
    InsideString,
    InsideStringEscape,
    InsideLiteral,
    InsideNumber,
    InsideObjectStart,
    InsideObjectKey,
    InsideObjectAfterKey,
    InsideObjectBeforeValue,
    InsideObjectAfterValue,
    InsideObjectAfterComma,
    InsideArrayStart,
    InsideArrayAfterValue,
    InsideArrayAfterComma,
}

/// Perform a *best-effort* repair of `input`, returning a *valid* JSON string
/// that is semantically *at least* a prefix of the input. The algorithm does
/// **not** attempt to correct semantically invalid documents – that is left to
/// the final `serde_json` parse attempt.
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn fix_json(input: &str) -> String {
    use State::*;

    // We operate on **byte indices** throughout – they line up with the
    // TypeScript implementation that used code unit indices.
    let mut stack = vec![Root];
    let mut last_valid_index: isize = -1;
    let mut literal_start: Option<usize> = None;

    // Convenience closures replicate the local helper functions from the TS
    // port.  They are implemented as inline functions for performance.

    // Handle the beginning of a JSON *value* (object, array, string, number, ...)
    fn process_value_start(
        ch: char,
        idx: usize,
        swap_state: State,
        stack: &mut Vec<State>,
        last_valid_index: &mut isize,
        literal_start: &mut Option<usize>,
    ) {
        use State::*;
        match ch {
            '"' => {
                *last_valid_index = (idx + ch.len_utf8() - 1) as isize;
                stack.pop();
                stack.push(swap_state);
                stack.push(InsideString);
            }
            'f' | 't' | 'n' => {
                *last_valid_index = (idx + ch.len_utf8() - 1) as isize;
                *literal_start = Some(idx);
                stack.pop();
                stack.push(swap_state);
                stack.push(InsideLiteral);
            }
            '-' => {
                stack.pop();
                stack.push(swap_state);
                stack.push(InsideNumber);
            }
            '0'..='9' => {
                *last_valid_index = idx as isize;
                stack.pop();
                stack.push(swap_state);
                stack.push(InsideNumber);
            }
            '{' => {
                *last_valid_index = idx as isize;
                stack.pop();
                stack.push(swap_state);
                stack.push(InsideObjectStart);
            }
            '[' => {
                *last_valid_index = idx as isize;
                stack.pop();
                stack.push(swap_state);
                stack.push(InsideArrayStart);
            }
            _ => {}
        }
    }

    fn process_after_object_value(
        ch: char,
        idx: usize,
        stack: &mut Vec<State>,
        last_valid_index: &mut isize,
    ) {
        use State::*;
        match ch {
            ',' => {
                stack.pop();
                stack.push(InsideObjectAfterComma);
            }
            '}' => {
                *last_valid_index = idx as isize;
                stack.pop();
            }
            _ => {}
        }
    }

    fn process_after_array_value(
        ch: char,
        idx: usize,
        stack: &mut Vec<State>,
        last_valid_index: &mut isize,
    ) {
        use State::*;
        match ch {
            ',' => {
                stack.pop();
                stack.push(InsideArrayAfterComma);
            }
            ']' => {
                *last_valid_index = idx as isize;
                stack.pop();
            }
            _ => {}
        }
    }

    for (idx, ch) in input.char_indices() {
        let Some(current_state) = stack.last() else {
            unreachable!("stack is never empty");
        };

        match current_state {
            Root => {
                process_value_start(
                    ch,
                    idx,
                    Finish,
                    &mut stack,
                    &mut last_valid_index,
                    &mut literal_start,
                );
            }
            InsideObjectStart => match ch {
                '"' => {
                    stack.pop();
                    stack.push(InsideObjectKey);
                }
                '}' => {
                    last_valid_index = idx as isize;
                    stack.pop();
                }
                _ => {}
            },
            InsideObjectAfterComma => {
                if ch == '"' {
                    stack.pop();
                    stack.push(InsideObjectKey);
                }
            }
            InsideObjectKey => {
                if ch == '"' {
                    stack.pop();
                    stack.push(InsideObjectAfterKey);
                }
            }
            InsideObjectAfterKey => {
                if ch == ':' {
                    stack.pop();
                    stack.push(InsideObjectBeforeValue);
                }
            }
            InsideObjectBeforeValue => {
                process_value_start(
                    ch,
                    idx,
                    InsideObjectAfterValue,
                    &mut stack,
                    &mut last_valid_index,
                    &mut literal_start,
                );
            }
            InsideObjectAfterValue => {
                process_after_object_value(ch, idx, &mut stack, &mut last_valid_index);
            }
            InsideString => match ch {
                '"' => {
                    stack.pop();
                    last_valid_index = idx as isize;
                }
                '\\' => stack.push(InsideStringEscape),
                _ => {
                    last_valid_index = idx as isize;
                }
            },
            InsideArrayStart => {
                if ch == ']' {
                    last_valid_index = idx as isize;
                    stack.pop();
                } else {
                    last_valid_index = idx as isize;
                    process_value_start(
                        ch,
                        idx,
                        InsideArrayAfterValue,
                        &mut stack,
                        &mut last_valid_index,
                        &mut literal_start,
                    );
                }
            }
            InsideArrayAfterValue => match ch {
                ',' => {
                    stack.pop();
                    stack.push(InsideArrayAfterComma);
                }
                ']' => {
                    last_valid_index = idx as isize;
                    stack.pop();
                }
                _ => {
                    last_valid_index = idx as isize;
                }
            },
            InsideArrayAfterComma => {
                process_value_start(
                    ch,
                    idx,
                    InsideArrayAfterValue,
                    &mut stack,
                    &mut last_valid_index,
                    &mut literal_start,
                );
            }
            InsideStringEscape => {
                stack.pop();
                last_valid_index = idx as isize;
            }
            InsideNumber => match ch {
                '0'..='9' => {
                    last_valid_index = idx as isize;
                }
                'e' | 'E' | '-' | '.' => {}
                ',' => {
                    stack.pop();
                    if let Some(&state) = stack.last() {
                        match state {
                            InsideArrayAfterValue => process_after_array_value(
                                ch,
                                idx,
                                &mut stack,
                                &mut last_valid_index,
                            ),
                            InsideObjectAfterValue => process_after_object_value(
                                ch,
                                idx,
                                &mut stack,
                                &mut last_valid_index,
                            ),
                            _ => {}
                        }
                    }
                }
                '}' => {
                    stack.pop();
                    if let Some(&state) = stack.last() {
                        if state == InsideObjectAfterValue {
                            process_after_object_value(ch, idx, &mut stack, &mut last_valid_index);
                        }
                    }
                }
                ']' => {
                    stack.pop();
                    if let Some(&state) = stack.last() {
                        if state == InsideArrayAfterValue {
                            process_after_array_value(ch, idx, &mut stack, &mut last_valid_index);
                        }
                    }
                }
                _ => {
                    stack.pop();
                }
            },
            InsideLiteral => {
                let Some(start) = literal_start else {
                    unreachable!("literal_start must be set inside literal");
                };
                let partial_literal = &input[start..=idx];
                if !"false".starts_with(partial_literal)
                    && !"true".starts_with(partial_literal)
                    && !"null".starts_with(partial_literal)
                {
                    stack.pop();
                    if let Some(&state) = stack.last() {
                        if state == InsideObjectAfterValue {
                            process_after_object_value(ch, idx, &mut stack, &mut last_valid_index);
                        } else if state == InsideArrayAfterValue {
                            process_after_array_value(ch, idx, &mut stack, &mut last_valid_index);
                        }
                    }
                } else {
                    last_valid_index = idx as isize;
                }
            }
            Finish => {
                // Do **not** advance: the JSON value is already complete
                // and we must ignore trailing bytes exactly like the TS impl.
            }
        }
    }

    // Build the *fixed* JSON string.
    let mut result = if last_valid_index >= 0 {
        #[allow(clippy::cast_sign_loss)]
        input[..=(last_valid_index as usize)].to_owned()
    } else {
        String::new()
    };

    // Close any open constructs by unwinding the stack.
    for &state in stack.iter().rev() {
        match state {
            InsideString => result.push('"'),
            InsideObjectKey
            | InsideObjectAfterKey
            | InsideObjectAfterComma
            | InsideObjectStart
            | InsideObjectBeforeValue
            | InsideObjectAfterValue => result.push('}'),
            InsideArrayStart | InsideArrayAfterComma | InsideArrayAfterValue => result.push(']'),
            InsideLiteral => {
                let Some(start) = literal_start else {
                    unreachable!("literal_start must be set inside literal");
                };
                let partial_literal = &input[start..];
                if "true".starts_with(partial_literal) {
                    result.push_str(&"true"[partial_literal.len()..]);
                } else if "false".starts_with(partial_literal) {
                    result.push_str(&"false"[partial_literal.len()..]);
                } else if "null".starts_with(partial_literal) {
                    result.push_str(&"null"[partial_literal.len()..]);
                }
            }
            _ => {}
        }
    }

    result
}

// -------------------------------------------------------------------------------------------------
// Tests – ensure behaviour roughly matches expectations.
// -------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[test]
    fn test_fix_json_simple_string() {
        let inp = "\"hello"; // missing closing quote
        let fixed = fix_json(inp);
        assert_eq!(fixed, "\"hello\"");
    }

    #[test]
    fn test_parse_partial_json_success() {
        let (val, state) = parse_partial_json(Some("123"));
        assert_eq!(state, ParseState::SuccessfulParse);
        assert_eq!(val.unwrap(), serde_json::json!(123));
    }

    #[test]
    fn test_parse_partial_json_repaired() {
        let (val, state) = parse_partial_json(Some("[1, 2, 3")); // missing ]
        assert_eq!(state, ParseState::RepairedParse);
        assert_eq!(val.unwrap(), serde_json::json!([1, 2, 3]));
    }

    #[test]
    fn test_parse_partial_json_failed() {
        // The raw input is not valid JSON. However, `fix_json` trims the input
        // down to its last *syntactically* valid prefix which, in this case,
        // is just the opening brace – completed to `{}` during the
        // post-processing phase. Therefore the parse **succeeds after repair**.
        let (val, state) = parse_partial_json(Some("{ invalid json"));
        assert_eq!(state, ParseState::RepairedParse);
        assert_eq!(val.unwrap(), serde_json::json!({}));
    }
    #[test]
    fn unicode_inside_unterminated_string() {
        let inp = r#"{"msg":"¡Hola"#;
        let (val, state) = parse_partial_json(Some(inp));
        assert_eq!(state, ParseState::RepairedParse);
        assert_eq!(val.unwrap()["msg"], "¡Hola");
    }

    #[test]
    fn ignore_trailing_garbage_after_complete_value() {
        let inp = "123abc";
        let (val, state) = parse_partial_json(Some(inp));
        // TS version succeeds; Rust should too once patched.
        assert_eq!(state, ParseState::RepairedParse);
        assert_eq!(val.unwrap(), serde_json::json!(123));
    }
}
