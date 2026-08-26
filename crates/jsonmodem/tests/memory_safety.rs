//! Deterministic streaming assertions that run without snapshot filesystem
//! access.

use jsonmodem::{
    BufferOptions, BufferedEvent, JsonModemBuffers, JsonModemValues, ParserOptions, Value,
    ValuesOptions, lending_iterator::LendingIterator,
};

fn final_values(chunks: &[&str]) -> Vec<serde_json::Value> {
    let mut parser = JsonModemValues::with_options(
        ParserOptions::default().with_allow_multiple_json_values(true),
        ValuesOptions::default(),
    );
    let mut values = Vec::new();
    for chunk in chunks {
        for event in parser.feed(chunk).to_iter() {
            let event = event.expect("valid streaming input");
            if event.is_final {
                values.push(serde_json::from_str(&event.value.to_string()).unwrap());
            }
        }
    }
    for event in parser.finish().to_iter() {
        let event = event.expect("complete streaming input");
        if event.is_final {
            values.push(serde_json::from_str(&event.value.to_string()).unwrap());
        }
    }
    values
}

fn completed_strings(chunks: &[&str]) -> Vec<(String, String)> {
    let mut parser = JsonModemBuffers::string(
        ParserOptions::default().with_allow_multiple_json_values(true),
        BufferOptions::default(),
    );
    let mut strings = Vec::new();
    for chunk in chunks {
        for event in parser.feed(chunk).to_iter() {
            if let BufferedEvent::String {
                path,
                value,
                is_final: true,
                ..
            } = event.unwrap()
            {
                strings.push((format!("{path:?}"), value.unwrap().as_ref().to_owned()));
            }
        }
    }
    for event in parser.finish().to_iter() {
        event.unwrap();
    }
    strings
}

#[test]
fn every_character_boundary_preserves_values_and_strings() {
    for input in [
        r#"["a\n"]"#,
        r#"["\u00e9"]"#,
        r#"["\u20ac"]"#,
        r#"{"a":"\ud83d\ude00"}"#,
        r#"{"":""}"#,
        "{\"\u{e9}\":\"\u{20ac}\u{1f600}\"}",
        r#"{"a":[null],"a":{}}"#,
        "[[],{},[true]]",
    ] {
        let expected: serde_json::Value = serde_json::from_str(input).unwrap();
        let strings = completed_strings(&[input]);
        for split in 0..=input.len() {
            if input.is_char_boundary(split) {
                let chunks = [&input[..split], &input[split..]];
                assert_eq!(
                    final_values(&chunks),
                    std::slice::from_ref(&expected),
                    "split {split}: {input}"
                );
                assert_eq!(
                    completed_strings(&chunks),
                    strings,
                    "split {split}: {input}"
                );
            }
        }
        let chunks: Vec<_> = input
            .char_indices()
            .map(|(start, ch)| &input[start..start + ch.len_utf8()])
            .collect();
        assert_eq!(final_values(&chunks), [expected]);
        assert_eq!(completed_strings(&chunks), strings);
    }
}

#[test]
fn generated_container_growth_and_multiple_roots() {
    let cases: usize = std::env::var("JSONMODEM_SAFETY_CASES")
        .unwrap_or_else(|_| "32".into())
        .parse()
        .expect("positive case count");
    assert!(cases > 0);
    eprintln!("JSONMODEM_SAFETY_CASES={cases}; deterministic input indices=0..{cases}");
    for case in 0..cases {
        let mut map = serde_json::Map::new();
        for index in 0..(2 + case % 15) {
            map.insert(format!("k{index}"), serde_json::Value::Bool(index % 2 == 0));
        }
        map.insert(
            "nested".into(),
            serde_json::json!([{"text": format!("t{case}")}]),
        );
        let expected = serde_json::Value::Object(map);
        let input = format!("{expected} {expected}");
        let width = 1 + case % 17;
        let chunks: Vec<_> = input
            .as_bytes()
            .chunks(width)
            .map(|chunk| std::str::from_utf8(chunk).unwrap())
            .collect();
        assert_eq!(final_values(&chunks), [expected.clone(), expected]);
    }
}

#[test]
fn partial_values_survive_iterator_drop_and_finish() {
    let mut parser = JsonModemValues::with_options(
        ParserOptions::default(),
        ValuesOptions::default().with_partial(true),
    );
    {
        let mut events = parser.feed("{\"title\":\"hel");
        LendingIterator::next(&mut events).unwrap().unwrap();
    }
    assert!(matches!(parser.view_root(), Value::Object(_)));
    let events: Vec<_> = parser.feed("lo\"}").to_iter().map(Result::unwrap).collect();
    assert!(events.iter().any(|event| event.is_final));
    let finished: Vec<_> = parser.finish().to_iter().map(Result::unwrap).collect();
    assert!(finished.is_empty());
}

#[test]
fn error_and_early_drop_release_partial_containers() {
    for input in [r#"{"a":[true,{"text":"partial"#, r#"{"a":[true,?]}"#] {
        let mut values = JsonModemValues::new(ParserOptions::default());
        let results: Vec<_> = values.feed(input).to_iter().collect();
        let errors = results.iter().any(Result::is_err);
        let final_results: Vec<_> = values.finish().to_iter().collect();
        assert!(errors || final_results.iter().any(Result::is_err));

        let mut buffers =
            JsonModemBuffers::string(ParserOptions::default(), BufferOptions::default());
        let mut events = buffers.feed(input);
        assert!(events.next().is_some());
        drop(events);
        // Dropping a parser with unread input must release borrowed state.
        drop(buffers);
    }
}
