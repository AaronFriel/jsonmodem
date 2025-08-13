use alloc::{
    string::{String, ToString},
    vec::Vec,
};

use quickcheck::QuickCheck;

use crate::{
    DefaultStreamingParser, StringValueMode, Value,
    event::test_util::reconstruct_values,
    options::{NonScalarValueMode, ParserOptions},
};

/// Property: Feeding a JSON document in arbitrary chunk sizes must yield the
/// exact same `Value` when reconstructed from the emitted `ParseEvent`s.
#[test]
fn partition_roundtrip_quickcheck() {
    #[expect(clippy::needless_pass_by_value)]
    fn prop(value: Value, splits: Vec<usize>, string_value_mode: StringValueMode) -> bool {
        let src = value.to_string();
        if src.is_empty() {
            return true;
        }

        // Stream parser DefaultStreamingParsere so that structural container events are
        // emitted.
        let mut parser = DefaultStreamingParser::new(ParserOptions {
            allow_multiple_json_values: true,
            non_scalar_values: NonScalarValueMode::All,
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
            for event in parser.feed(&chunk) {
                events.push(event.unwrap());
            }
            idx = end;
            remaining -= size;
        }
        if remaining > 0 {
            let chunk: String = chars[idx..].iter().collect();
            for event in parser.feed(&chunk) {
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

    let tests = if cfg!(any(miri, feature = "test-fast")) {
        10
    } else if is_ci::cached() {
        10_000
    } else {
        1_000
    };

    QuickCheck::new()
        .tests(tests)
        .quickcheck(prop as fn(Value, Vec<usize>, StringValueMode) -> bool);
}
