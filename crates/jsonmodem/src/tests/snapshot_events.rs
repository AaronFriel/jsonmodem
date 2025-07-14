//! Snapshot test that verifies the exact sequence of `ParseEvent`s emitted for
//! a moderately complex JSON input.  The test is particularly useful to catch
//! unintended behaviour changes when the parser implementation is modified.

use alloc::vec::Vec;

// Enable the `yaml` feature for a more human-readable snapshot format.
use insta::assert_yaml_snapshot;

use crate::{ParseEvent, ParserOptions, StreamingParser};

#[test]
fn snapshot_complex_document() {
    let json = r#"{
        "users": [
            {"id": 1, "name": "Ada"},
            {"id": 2, "name": "Grace"}
        ],
        "meta": {"count": 2}
    }"#;

    let mut parser = StreamingParser::new(ParserOptions::default());
    parser.feed(json);

    let events: Vec<ParseEvent> = parser
        .finish()
        .collect::<Result<_, _>>()
        .expect("parser should not error on valid input");

    // Inline snapshot taken from a known-good run via `cargo insta review`.
    assert_yaml_snapshot!(events, @r"
    - kind: ObjectBegin
      path: []
    - kind: ArrayStart
      path:
        - users
    - kind: ObjectBegin
      path:
        - users
        - 0
    - kind: Number
      path:
        - users
        - 0
        - id
      value: 1
    - kind: String
      path:
        - users
        - 0
        - name
      fragment: Ada
      is_final: true
    - kind: ObjectEnd
      path:
        - users
        - 0
    - kind: ObjectBegin
      path:
        - users
        - 1
    - kind: Number
      path:
        - users
        - 1
        - id
      value: 2
    - kind: String
      path:
        - users
        - 1
        - name
      fragment: Grace
      is_final: true
    - kind: ObjectEnd
      path:
        - users
        - 1
    - kind: ArrayEnd
      path:
        - users
    - kind: ObjectBegin
      path:
        - meta
    - kind: Number
      path:
        - meta
        - count
      value: 2
    - kind: ObjectEnd
      path:
        - meta
    - kind: ObjectEnd
      path: []
    ");
}
