#![allow(missing_docs)]
#![expect(clippy::needless_raw_string_hashes)]

pub const ORIGINAL: &str = r#"
{
    "moderation": {
        "decision": "allow",
        "reason": null
    },
    "request": {
        "filename": "example.rs",
        "language": "rust",
        "options": {
            "opt_level": "2",
            "features": [
                "serde",
                "tokio"
            ]
        }
    },
    "snippets": [
        "fn main() {}",
        "println!(\"hi\")"
    ],
    "entities": [
        {
            "type": "function",
            "name": "main"
        },
        {
            "type": "macro",
            "name": "println"
        }
    ],
    "matrix": [
        [
            "a"
        ]
    ],
    "mixed": [
        "s",
        {
            "k": "v"
        },
        "t",
        [
            "u"
        ],
        "end"
    ],
    "trailing": {
        "status": "ok"
    },
    "object_in_array_last": [
        {
            "a": 1
        }
    ],
    "nested_objects": {
        "outer": {
            "inner": 1
        }
    }
}"#;

// This stream simulates a structured tool-call response with nested
// objects/arrays. It intentionally cuts chunks on transition seams to
// exercise the streaming parser.
#[rustfmt::skip]
pub const STREAM: [&str; 22] = [
    r#"{"moderation":{"decision":"al"#,                                       // (building "allow")
    r#"lo"#,                                                                  // (building "allow")
    r#"w","reason":null},""#,                                                 // ends with '},"'  object end -> new string (next chunk starts the key)

    r#"request":{"filename":"example.rs""#,                                   // ... "request":{"filename":"example.rs"
    r#","language":"rust","#,                                                 // starts with '","'  string -> new string (within same object)

    r#""options":{"opt_level":"2""#,                                           // ... "options":{"opt_level":"2"
    r#","features":["serde""#,                                                // starts with ',"'  string -> new string (to next array element)
    r#","tokio"]}"#,                                                          // ends with '"]}'  string -> array end, then array end -> object end

    r#"}"#,                                                                   // across-boundary '}}'  object end (options) -> object end (request)
    r#","snippets":["#,                                                       // across-boundary '},"'  object end -> new string (top-level key)

    r#""fn main() {}","#,                                                     // ends with '","'  string -> new string (next array string)
    r#""println!(\"hi\")"]"#,                                                 // ends with '"]'   string -> array end

    r#","entities":[{"type":"function","name":"main"},{"type":"macro","name":"println"}]"#, // ends with '}]'  object end -> array end
    r#","matrix":[["a"]]"#,                                                   // ends with ']]'   array end -> array end (inner, then outer)

    r#","mixed":["s",{"k":"v"}"#,                                             // ends with '}'    (sets up next '},"' in following chunk)
    r#","t""#,                                                                // across-boundary '},"'  object end -> new string (within array)
    r#",["u"]"#,                                                              // (inner array element)
    r#","end"]"#,                                                             // across-boundary '],'  array end -> new string (within array), ends with '"]' (string -> array end)

    r#","trailing":{"status":"ok"}"#,                                         // ends with '"}'   string -> object end
    r#","object_in_array_last":[{"a":1}]"#,                                   // ends with '}]'   object end -> array end
    r#","nested_objects":{"outer":{"inner":1}}"#,                             // ends with '}}'   object end -> object end
    r#"}"#,                                                                   // closes the top-level object
];

#[test]
#[allow(clippy::too_many_lines)]
fn assert_stream_example() {
    let streamed = STREAM.join("");

    let value: serde_json::Value = serde_json::from_str(ORIGINAL).unwrap();
    let original = serde_json::to_string(&value).unwrap();

    assert_eq!(streamed, original);
}
