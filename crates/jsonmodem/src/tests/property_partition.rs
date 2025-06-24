use alloc::{
    string::{String, ToString},
    vec::Vec,
};

use quickcheck::QuickCheck;

use crate::{
    StreamingParser, StringValueMode, Value, event::reconstruct_values, options::ParserOptions,
};

/// Property: Feeding a JSON document in arbitrary chunk sizes must yield the
/// exact same `Value` when reconstructed from the emitted `ParseEvent`s.
#[test]
fn partition_roundtrip_quickcheck() {
    #[allow(clippy::needless_pass_by_value)]
    fn prop(value: Value, splits: Vec<usize>, string_value_mode: StringValueMode) -> bool {
        let src = value.to_string();
        if src.is_empty() {
            return true;
        }

        // Stream parser in `stream` mode so that structural container events are
        // emitted.
        let mut parser = StreamingParser::new(ParserOptions {
            allow_multiple_json_values: true,
            emit_non_scalar_values: true,
            string_value_mode,
            ..Default::default()
        });
        let mut events = Vec::<crate::event::ParseEvent>::new();

        // Feed the JSON text in arbitrarily sized UTF-8-safe chunks (derived from
        // `splits`).
        let chars: Vec<char> = src.chars().collect();
        let mut idx = 0;
        let mut remaining = chars.len();

        for s in splits {
            if remaining == 0 {
                break;
            }
            let size = 1 + (s % remaining);
            let end = idx + size;
            let chunk: String = chars[idx..end].iter().collect();
            parser.feed(&chunk);
            for event in parser.by_ref() {
                events.push(event.unwrap());
            }
            idx = end;
            remaining -= size;
        }
        if remaining > 0 {
            let chunk: String = chars[idx..].iter().collect();
            parser.feed(&chunk);
            for event in parser.by_ref() {
                events.push(event.unwrap());
            }
        }

        // Flush any pending events.
        let parser = parser.finish();
        for event in parser {
            events.push(event.unwrap());
        }

        let reconstructed = reconstruct_values(events.clone());
        reconstructed.len() == 1 && reconstructed[0] == value
    }

    #[cfg(not(miri))]
    let tests = if is_ci::cached() { 10_000 } else { 1_000 };
    #[cfg(miri)]
    let tests = 10;

    QuickCheck::new()
        .tests(tests)
        .quickcheck(prop as fn(Value, Vec<usize>, StringValueMode) -> bool);
}
