//! Snapshot test that verifies the exact sequence of `ParseEvent`s emitted for
//! a moderately complex JSON input.  The test is particularly useful to catch
//! unintended behaviour changes when the parser implementation is modified.
#![cfg(not(miri))]
#![allow(clippy::too_many_lines)]

use alloc::{format, vec::Vec};

use crate::{
    backend::StdBackend,
    parser::{JsonModem, ParseEvent, ParserOptions},
};

type TestParser = JsonModem<StdBackend>;

#[test]
fn snapshot_complex_document() {
    let json = r#"{
        "users": [
            {"id": 1, "name": "Ada"},
            {"id": 2, "name": "Grace"}
        ],
        "meta": {"count": 2}
    }"#;

    let mut parser = TestParser::new(ParserOptions::default());
    let mut events: Vec<ParseEvent<'_, crate::Path, StdBackend>> = parser
        .feed(json)
        .to_iter()
        .collect::<Result<_, _>>()
        .expect("parser should not error on valid input");
    events.extend(
        parser
            .finish()
            .to_iter()
            .collect::<Result<Vec<_>, _>>()
            .expect("parser should not error while finishing"),
    );

    let formatted = format!("{events:#?}");
    insta::assert_snapshot!(formatted, @r###"
[
    ObjectBegin {
        path: [],
    },
    ArrayBegin {
        path: [
            Key(
                "users",
            ),
        ],
    },
    ObjectBegin {
        path: [
            Key(
                "users",
            ),
            Index(
                0,
            ),
        ],
    },
    Number {
        path: [
            Key(
                "users",
            ),
            Index(
                0,
            ),
            Key(
                "id",
            ),
        ],
        value: 1.0,
    },
    String {
        path: [
            Key(
                "users",
            ),
            Index(
                0,
            ),
            Key(
                "name",
            ),
        ],
        fragment: "Ada",
        is_initial: true,
        is_final: true,
    },
    ObjectEnd {
        path: [
            Key(
                "users",
            ),
            Index(
                0,
            ),
        ],
    },
    ObjectBegin {
        path: [
            Key(
                "users",
            ),
            Index(
                1,
            ),
        ],
    },
    Number {
        path: [
            Key(
                "users",
            ),
            Index(
                1,
            ),
            Key(
                "id",
            ),
        ],
        value: 2.0,
    },
    String {
        path: [
            Key(
                "users",
            ),
            Index(
                1,
            ),
            Key(
                "name",
            ),
        ],
        fragment: "Grace",
        is_initial: true,
        is_final: true,
    },
    ObjectEnd {
        path: [
            Key(
                "users",
            ),
            Index(
                1,
            ),
        ],
    },
    ArrayEnd {
        path: [
            Key(
                "users",
            ),
        ],
    },
    ObjectBegin {
        path: [
            Key(
                "meta",
            ),
        ],
    },
    Number {
        path: [
            Key(
                "meta",
            ),
            Key(
                "count",
            ),
        ],
        value: 2.0,
    },
    ObjectEnd {
        path: [
            Key(
                "meta",
            ),
        ],
    },
    ObjectEnd {
        path: [],
    },
]
"###);
}
