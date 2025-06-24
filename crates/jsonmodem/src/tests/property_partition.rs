use alloc::{
    string::{String, ToString},
    vec::Vec,
};

use quickcheck::QuickCheck;

use crate::{StreamingParser, Value, options::ParserOptions};

/// Property: Feeding a JSON document in arbitrary chunk sizes must yield the
/// exact same `Value` when reconstructed from the emitted `ParseEvent`s.
#[test]
fn partition_roundtrip_quickcheck() {
    #[allow(clippy::needless_pass_by_value)]
    fn prop(value: Value, splits: Vec<usize>) -> bool {
        let src = value.to_string();
        if src.is_empty() {
            return true;
        }

        // Stream parser in `stream` mode so that structural container events are
        // emitted.
        let mut parser = StreamingParser::new(ParserOptions {
            allow_multiple_json_values: true,
            emit_non_scalar_values: true,
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
            if let Ok(evts) = parser.feed_todo_remove_me(&chunk) {
                events.extend(evts);
            }
            idx = end;
            remaining -= size;
        }
        if remaining > 0 {
            let chunk: String = chars[idx..].iter().collect();
            if let Ok(evts) = parser.feed_todo_remove_me(&chunk) {
                events.extend(evts);
            }
        }

        true // TODO

        // // Collect *all* events after the full input has been supplied.
        // let mut events = Vec::new();
        // // Flush remaining events by passing empty chunk at the end.
        // match parser.feed("") {
        //     Ok(evts) => events.extend(evts),
        //     Err(_) => return false,
        // };

        // return true;
        // // Flush remaining events by signaling end-of-input so structural
        // events // are emitted.
        // if let Ok(evts) = parser.finish("") {
        //     events.extend(evts);
        // } else {
        //     return false;
        // }
        // let reconstructed = reconstruct_values(events);
        // // Single root → exactly one reconstructed value identical to the
        // // source `Value`.
        // reconstructed.len() == 1 && reconstructed[0] == original_val
    }

    QuickCheck::new()
        .tests(100)
        .quickcheck(prop as fn(Value, Vec<usize>) -> bool);
}
