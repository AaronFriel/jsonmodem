#![cfg(feature = "im")]
#![allow(missing_docs)]
//! Sanity test for the feature-gated `JsonModemIm` adapter.

use jsonmodem::{JsonModemIm, ParserOptions, ValuesOptions};
use serde_json::Value as SerdeValue;

#[test]
fn jsonmodem_im_snapshots_are_stable() {
    let mut adapter = JsonModemIm::with_options(
        ParserOptions::default(),
        ValuesOptions::default().with_partial(true),
    );

    let mut collected = Vec::new();

    for chunk in &["{\"title\":\"he", "llo\",\"nums\":[1", ",2,3]}"] {
        for snapshot in adapter.feed(chunk) {
            collected.push(snapshot);
        }
    }

    for snapshot in adapter.finish() {
        collected.push(snapshot);
    }

    let final_snapshot = collected
        .into_iter()
        .rev()
        .find(|snapshot| snapshot.is_final)
        .expect("expected a final snapshot from JsonModemIm");

    assert_eq!(
        serde_json::from_str::<SerdeValue>(&final_snapshot.value.to_string()).unwrap(),
        serde_json::json!({"title": "hello", "nums": [1, 2, 3]})
    );
}
