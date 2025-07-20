use alloc::vec;

use crate::{produce_chunks, produce_prefixes};

#[test]
fn produce_helpers_example() {
    let payload = "[\"foo\",\"bar\"]";
    let chunks = produce_chunks(payload, 5);
    assert_eq!(chunks, vec!["[\"f", "oo\"", ",\"b", "ar\"", "]"]);
    let prefixes = produce_prefixes(payload, 5);
    assert_eq!(
        prefixes,
        vec![
            "[\"f",
            "[\"foo\"",
            "[\"foo\",\"b",
            "[\"foo\",\"bar\"",
            "[\"foo\",\"bar\"]",
        ]
    );
}

#[test]
fn produce_helpers_multibyte() {
    let payload = "[\"f😊o\",\"b🚀r\"]";
    let parts = 5;
    let chunks = produce_chunks(payload, parts);
    let mut idx = 0;
    for chunk in &chunks {
        idx += chunk.len();
        assert!(payload.is_char_boundary(idx));
    }
    assert_eq!(chunks.concat(), payload);

    let prefixes = produce_prefixes(payload, parts);
    for prefix in &prefixes {
        idx = prefix.len();
        assert!(payload.is_char_boundary(idx));
        assert_eq!(&payload[..idx], *prefix);
    }
    assert_eq!(prefixes.last().unwrap(), &payload);
}
