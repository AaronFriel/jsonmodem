#![expect(missing_docs)]
#![expect(clippy::needless_raw_string_hashes)]

use core::fmt::Write;

use insta::{assert_snapshot, assert_yaml_snapshot};
use jsonmodem::{
    BufferOptions, JsonModem, JsonModemBuffers, JsonModemValues, NonScalarMode, ParserOptions,
    StringBufferMode,
};

mod common;

#[test]
#[allow(clippy::too_many_lines)]
fn snapshot_three_layers_llm_stream() {
    let stream = common::STREAM;

    // print the joined JSON object for reference, wrapping at column 80
    let mut stream_joined = String::new();
    for (i, ch) in stream.iter().flat_map(|f| f.chars()).enumerate() {
        if i % 80 == 0 && i > 0 {
            stream_joined.push('\n');
        }
        stream_joined.push(ch);
    }

    assert_snapshot!(stream_joined, @r#"
    {"moderation":{"decision":"allow","reason":null},"request":{"filename":"example.
    rs","language":"rust","options":{"opt_level":"2","features":["serde","tokio"]}},
    "snippets":["fn main() {}","println!(\"hi\")"],"entities":[{"type":"function","n
    ame":"main"},{"type":"macro","name":"println"}],"matrix":[["a"]],"mixed":["s",{"
    k":"v"},"t",["u"],"end"],"trailing":{"status":"ok"},"object_in_array_last":[{"a"
    :1}],"nested_objects":{"outer":{"inner":1}}}
    "#);

    // Core
    let mut core = JsonModem::new(ParserOptions::default());
    let mut core_lines = String::new();
    for ch in &stream {
        for ev in core.feed(ch) {
            let ev = ev.expect("core error");
            #[cfg(feature = "serde")]
            {
                core_lines.push_str(&serde_json::to_string(&ev).unwrap());
                core_lines.push('\n');
            }
            #[cfg(not(feature = "serde"))]
            {
                writeln!(core_lines, "{ev:?}").unwrap();
            }
        }
    }

    // Buffers (Values)
    let mut buf = JsonModemBuffers::new(
        ParserOptions::default(),
        BufferOptions {
            string_buffer_mode: StringBufferMode::Values,
            non_scalar_mode: NonScalarMode::None,
        },
    );
    let mut buf_lines = String::new();
    for ch in &stream {
        for ev in buf.feed(ch) {
            let ev = ev.expect("buffers error");
            #[cfg(feature = "serde")]
            {
                buf_lines.push_str(&serde_json::to_string(&ev).unwrap());
                buf_lines.push('\n');
            }
            #[cfg(not(feature = "serde"))]
            {
                writeln!(buf_lines, "{ev:?}").unwrap();
            }
        }
    }

    // Buffers (Prefixes)
    let mut bufp = JsonModemBuffers::new(
        ParserOptions::default(),
        BufferOptions {
            string_buffer_mode: StringBufferMode::Prefixes,
            non_scalar_mode: NonScalarMode::None,
        },
    );
    let mut bufp_lines = String::new();
    for ch in &stream {
        for ev in bufp.feed(ch) {
            let ev = ev.expect("buffers error");
            #[cfg(feature = "serde")]
            {
                bufp_lines.push_str(&serde_json::to_string(&ev).unwrap());
                bufp_lines.push('\n');
            }
            #[cfg(not(feature = "serde"))]
            {
                writeln!(bufp_lines, "{ev:?}").unwrap();
            }
        }
    }

    // Values (finals only)
    let mut vals = JsonModemValues::new(ParserOptions::default());
    let mut val_lines = String::new();
    for ch in &stream {
        for sv in vals.feed(ch) {
            let sv = sv.expect("values error");
            writeln!(
                val_lines,
                "{{\"index\":{},\"is_final\":{},\"value\":{:?}}}",
                sv.index, sv.is_final, sv.value
            )
            .unwrap();
        }
    }

    // Inline snapshots; run `cargo insta test` then `cargo insta review` to
    // populate
    insta::assert_snapshot!(core_lines, @r#"
    ObjectBegin { path: [] }
    ObjectBegin { path: [Key("moderation")] }
    String { path: [Key("moderation"), Key("decision")], value: None, fragment: "al", is_final: false }
    String { path: [Key("moderation"), Key("decision")], value: None, fragment: "lo", is_final: false }
    String { path: [Key("moderation"), Key("decision")], value: None, fragment: "w", is_final: true }
    Null { path: [Key("moderation"), Key("reason")] }
    ObjectEnd { path: [Key("moderation")], value: None }
    ObjectBegin { path: [Key("request")] }
    String { path: [Key("request"), Key("filename")], value: None, fragment: "example.rs", is_final: true }
    String { path: [Key("request"), Key("language")], value: None, fragment: "rust", is_final: true }
    ObjectBegin { path: [Key("request"), Key("options")] }
    String { path: [Key("request"), Key("options"), Key("opt_level")], value: None, fragment: "2", is_final: true }
    ArrayStart { path: [Key("request"), Key("options"), Key("features")] }
    String { path: [Key("request"), Key("options"), Key("features"), Index(0)], value: None, fragment: "serde", is_final: true }
    String { path: [Key("request"), Key("options"), Key("features"), Index(1)], value: None, fragment: "tokio", is_final: true }
    ArrayEnd { path: [Key("request"), Key("options"), Key("features")], value: None }
    ObjectEnd { path: [Key("request"), Key("options")], value: None }
    ObjectEnd { path: [Key("request")], value: None }
    ArrayStart { path: [Key("snippets")] }
    String { path: [Key("snippets"), Index(0)], value: None, fragment: "fn main() {}", is_final: true }
    String { path: [Key("snippets"), Index(1)], value: None, fragment: "println!(\"hi\")", is_final: true }
    ArrayEnd { path: [Key("snippets")], value: None }
    ArrayStart { path: [Key("entities")] }
    ObjectBegin { path: [Key("entities"), Index(0)] }
    String { path: [Key("entities"), Index(0), Key("type")], value: None, fragment: "function", is_final: true }
    String { path: [Key("entities"), Index(0), Key("name")], value: None, fragment: "main", is_final: true }
    ObjectEnd { path: [Key("entities"), Index(0)], value: None }
    ObjectBegin { path: [Key("entities"), Index(1)] }
    String { path: [Key("entities"), Index(1), Key("type")], value: None, fragment: "macro", is_final: true }
    String { path: [Key("entities"), Index(1), Key("name")], value: None, fragment: "println", is_final: true }
    ObjectEnd { path: [Key("entities"), Index(1)], value: None }
    ArrayEnd { path: [Key("entities")], value: None }
    ArrayStart { path: [Key("matrix")] }
    ArrayStart { path: [Key("matrix"), Index(0)] }
    String { path: [Key("matrix"), Index(0), Index(0)], value: None, fragment: "a", is_final: true }
    ArrayEnd { path: [Key("matrix"), Index(0)], value: None }
    ArrayEnd { path: [Key("matrix")], value: None }
    ArrayStart { path: [Key("mixed")] }
    String { path: [Key("mixed"), Index(0)], value: None, fragment: "s", is_final: true }
    ObjectBegin { path: [Key("mixed"), Index(1)] }
    String { path: [Key("mixed"), Index(1), Key("k")], value: None, fragment: "v", is_final: true }
    ObjectEnd { path: [Key("mixed"), Index(1)], value: None }
    String { path: [Key("mixed"), Index(2)], value: None, fragment: "t", is_final: true }
    ArrayStart { path: [Key("mixed"), Index(3)] }
    String { path: [Key("mixed"), Index(3), Index(0)], value: None, fragment: "u", is_final: true }
    ArrayEnd { path: [Key("mixed"), Index(3)], value: None }
    String { path: [Key("mixed"), Index(4)], value: None, fragment: "end", is_final: true }
    ArrayEnd { path: [Key("mixed")], value: None }
    ObjectBegin { path: [Key("trailing")] }
    String { path: [Key("trailing"), Key("status")], value: None, fragment: "ok", is_final: true }
    ObjectEnd { path: [Key("trailing")], value: None }
    ArrayStart { path: [Key("object_in_array_last")] }
    ObjectBegin { path: [Key("object_in_array_last"), Index(0)] }
    Number { path: [Key("object_in_array_last"), Index(0), Key("a")], value: 1.0 }
    ObjectEnd { path: [Key("object_in_array_last"), Index(0)], value: None }
    ArrayEnd { path: [Key("object_in_array_last")], value: None }
    ObjectBegin { path: [Key("nested_objects")] }
    ObjectBegin { path: [Key("nested_objects"), Key("outer")] }
    Number { path: [Key("nested_objects"), Key("outer"), Key("inner")], value: 1.0 }
    ObjectEnd { path: [Key("nested_objects"), Key("outer")], value: None }
    ObjectEnd { path: [Key("nested_objects")], value: None }
    ObjectEnd { path: [], value: None }
    "#);
    insta::assert_snapshot!(buf_lines,  @r#"
    ObjectBegin { path: [] }
    ObjectBegin { path: [Key("moderation")] }
    String { path: [Key("moderation"), Key("decision")], fragment: "al", value: Some("al"), is_final: false }
    String { path: [Key("moderation"), Key("decision")], fragment: "lo", value: Some("allo"), is_final: false }
    String { path: [Key("moderation"), Key("decision")], fragment: "w", value: Some("allow"), is_final: true }
    Null { path: [Key("moderation"), Key("reason")] }
    ObjectEnd { path: [Key("moderation")], value: None }
    ObjectBegin { path: [Key("request")] }
    String { path: [Key("request"), Key("filename")], fragment: "example.rs", value: Some("example.rs"), is_final: true }
    String { path: [Key("request"), Key("language")], fragment: "rust", value: Some("rust"), is_final: true }
    ObjectBegin { path: [Key("request"), Key("options")] }
    String { path: [Key("request"), Key("options"), Key("opt_level")], fragment: "2", value: Some("2"), is_final: true }
    ArrayStart { path: [Key("request"), Key("options"), Key("features")] }
    String { path: [Key("request"), Key("options"), Key("features"), Index(0)], fragment: "serde", value: Some("serde"), is_final: true }
    String { path: [Key("request"), Key("options"), Key("features"), Index(1)], fragment: "tokio", value: Some("tokio"), is_final: true }
    ArrayEnd { path: [Key("request"), Key("options"), Key("features")], value: None }
    ObjectEnd { path: [Key("request"), Key("options")], value: None }
    ObjectEnd { path: [Key("request")], value: None }
    ArrayStart { path: [Key("snippets")] }
    String { path: [Key("snippets"), Index(0)], fragment: "fn main() {}", value: Some("fn main() {}"), is_final: true }
    String { path: [Key("snippets"), Index(1)], fragment: "println!(\"hi\")", value: Some("println!(\"hi\")"), is_final: true }
    ArrayEnd { path: [Key("snippets")], value: None }
    ArrayStart { path: [Key("entities")] }
    ObjectBegin { path: [Key("entities"), Index(0)] }
    String { path: [Key("entities"), Index(0), Key("type")], fragment: "function", value: Some("function"), is_final: true }
    String { path: [Key("entities"), Index(0), Key("name")], fragment: "main", value: Some("main"), is_final: true }
    ObjectEnd { path: [Key("entities"), Index(0)], value: None }
    ObjectBegin { path: [Key("entities"), Index(1)] }
    String { path: [Key("entities"), Index(1), Key("type")], fragment: "macro", value: Some("macro"), is_final: true }
    String { path: [Key("entities"), Index(1), Key("name")], fragment: "println", value: Some("println"), is_final: true }
    ObjectEnd { path: [Key("entities"), Index(1)], value: None }
    ArrayEnd { path: [Key("entities")], value: None }
    ArrayStart { path: [Key("matrix")] }
    ArrayStart { path: [Key("matrix"), Index(0)] }
    String { path: [Key("matrix"), Index(0), Index(0)], fragment: "a", value: Some("a"), is_final: true }
    ArrayEnd { path: [Key("matrix"), Index(0)], value: None }
    ArrayEnd { path: [Key("matrix")], value: None }
    ArrayStart { path: [Key("mixed")] }
    String { path: [Key("mixed"), Index(0)], fragment: "s", value: Some("s"), is_final: true }
    ObjectBegin { path: [Key("mixed"), Index(1)] }
    String { path: [Key("mixed"), Index(1), Key("k")], fragment: "v", value: Some("v"), is_final: true }
    ObjectEnd { path: [Key("mixed"), Index(1)], value: None }
    String { path: [Key("mixed"), Index(2)], fragment: "t", value: Some("t"), is_final: true }
    ArrayStart { path: [Key("mixed"), Index(3)] }
    String { path: [Key("mixed"), Index(3), Index(0)], fragment: "u", value: Some("u"), is_final: true }
    ArrayEnd { path: [Key("mixed"), Index(3)], value: None }
    String { path: [Key("mixed"), Index(4)], fragment: "end", value: Some("end"), is_final: true }
    ArrayEnd { path: [Key("mixed")], value: None }
    ObjectBegin { path: [Key("trailing")] }
    String { path: [Key("trailing"), Key("status")], fragment: "ok", value: Some("ok"), is_final: true }
    ObjectEnd { path: [Key("trailing")], value: None }
    ArrayStart { path: [Key("object_in_array_last")] }
    ObjectBegin { path: [Key("object_in_array_last"), Index(0)] }
    Number { path: [Key("object_in_array_last"), Index(0), Key("a")], value: 1.0 }
    ObjectEnd { path: [Key("object_in_array_last"), Index(0)], value: None }
    ArrayEnd { path: [Key("object_in_array_last")], value: None }
    ObjectBegin { path: [Key("nested_objects")] }
    ObjectBegin { path: [Key("nested_objects"), Key("outer")] }
    Number { path: [Key("nested_objects"), Key("outer"), Key("inner")], value: 1.0 }
    ObjectEnd { path: [Key("nested_objects"), Key("outer")], value: None }
    ObjectEnd { path: [Key("nested_objects")], value: None }
    ObjectEnd { path: [], value: None }
    "#);
    insta::assert_snapshot!(bufp_lines, @r#"
    ObjectBegin { path: [] }
    ObjectBegin { path: [Key("moderation")] }
    String { path: [Key("moderation"), Key("decision")], fragment: "al", value: None, is_final: false }
    String { path: [Key("moderation"), Key("decision")], fragment: "lo", value: None, is_final: false }
    String { path: [Key("moderation"), Key("decision")], fragment: "w", value: Some("allow"), is_final: true }
    Null { path: [Key("moderation"), Key("reason")] }
    ObjectEnd { path: [Key("moderation")], value: None }
    ObjectBegin { path: [Key("request")] }
    String { path: [Key("request"), Key("filename")], fragment: "example.rs", value: Some("example.rs"), is_final: true }
    String { path: [Key("request"), Key("language")], fragment: "rust", value: Some("rust"), is_final: true }
    ObjectBegin { path: [Key("request"), Key("options")] }
    String { path: [Key("request"), Key("options"), Key("opt_level")], fragment: "2", value: Some("2"), is_final: true }
    ArrayStart { path: [Key("request"), Key("options"), Key("features")] }
    String { path: [Key("request"), Key("options"), Key("features"), Index(0)], fragment: "serde", value: Some("serde"), is_final: true }
    String { path: [Key("request"), Key("options"), Key("features"), Index(1)], fragment: "tokio", value: Some("tokio"), is_final: true }
    ArrayEnd { path: [Key("request"), Key("options"), Key("features")], value: None }
    ObjectEnd { path: [Key("request"), Key("options")], value: None }
    ObjectEnd { path: [Key("request")], value: None }
    ArrayStart { path: [Key("snippets")] }
    String { path: [Key("snippets"), Index(0)], fragment: "fn main() {}", value: Some("fn main() {}"), is_final: true }
    String { path: [Key("snippets"), Index(1)], fragment: "println!(\"hi\")", value: Some("println!(\"hi\")"), is_final: true }
    ArrayEnd { path: [Key("snippets")], value: None }
    ArrayStart { path: [Key("entities")] }
    ObjectBegin { path: [Key("entities"), Index(0)] }
    String { path: [Key("entities"), Index(0), Key("type")], fragment: "function", value: Some("function"), is_final: true }
    String { path: [Key("entities"), Index(0), Key("name")], fragment: "main", value: Some("main"), is_final: true }
    ObjectEnd { path: [Key("entities"), Index(0)], value: None }
    ObjectBegin { path: [Key("entities"), Index(1)] }
    String { path: [Key("entities"), Index(1), Key("type")], fragment: "macro", value: Some("macro"), is_final: true }
    String { path: [Key("entities"), Index(1), Key("name")], fragment: "println", value: Some("println"), is_final: true }
    ObjectEnd { path: [Key("entities"), Index(1)], value: None }
    ArrayEnd { path: [Key("entities")], value: None }
    ArrayStart { path: [Key("matrix")] }
    ArrayStart { path: [Key("matrix"), Index(0)] }
    String { path: [Key("matrix"), Index(0), Index(0)], fragment: "a", value: Some("a"), is_final: true }
    ArrayEnd { path: [Key("matrix"), Index(0)], value: None }
    ArrayEnd { path: [Key("matrix")], value: None }
    ArrayStart { path: [Key("mixed")] }
    String { path: [Key("mixed"), Index(0)], fragment: "s", value: Some("s"), is_final: true }
    ObjectBegin { path: [Key("mixed"), Index(1)] }
    String { path: [Key("mixed"), Index(1), Key("k")], fragment: "v", value: Some("v"), is_final: true }
    ObjectEnd { path: [Key("mixed"), Index(1)], value: None }
    String { path: [Key("mixed"), Index(2)], fragment: "t", value: Some("t"), is_final: true }
    ArrayStart { path: [Key("mixed"), Index(3)] }
    String { path: [Key("mixed"), Index(3), Index(0)], fragment: "u", value: Some("u"), is_final: true }
    ArrayEnd { path: [Key("mixed"), Index(3)], value: None }
    String { path: [Key("mixed"), Index(4)], fragment: "end", value: Some("end"), is_final: true }
    ArrayEnd { path: [Key("mixed")], value: None }
    ObjectBegin { path: [Key("trailing")] }
    String { path: [Key("trailing"), Key("status")], fragment: "ok", value: Some("ok"), is_final: true }
    ObjectEnd { path: [Key("trailing")], value: None }
    ArrayStart { path: [Key("object_in_array_last")] }
    ObjectBegin { path: [Key("object_in_array_last"), Index(0)] }
    Number { path: [Key("object_in_array_last"), Index(0), Key("a")], value: 1.0 }
    ObjectEnd { path: [Key("object_in_array_last"), Index(0)], value: None }
    ArrayEnd { path: [Key("object_in_array_last")], value: None }
    ObjectBegin { path: [Key("nested_objects")] }
    ObjectBegin { path: [Key("nested_objects"), Key("outer")] }
    Number { path: [Key("nested_objects"), Key("outer"), Key("inner")], value: 1.0 }
    ObjectEnd { path: [Key("nested_objects"), Key("outer")], value: None }
    ObjectEnd { path: [Key("nested_objects")], value: None }
    ObjectEnd { path: [], value: None }
    "#);
    insta::assert_snapshot!(val_lines,  @r#"{"index":0,"is_final":true,"value":Object({"entities": Array([Object({"name": String("main"), "type": String("function")}), Object({"name": String("println"), "type": String("macro")})]), "matrix": Array([Array([String("a")])]), "mixed": Array([String("s"), Object({"k": String("v")}), String("t"), Array([String("u")]), String("end")]), "moderation": Object({"decision": String("allow"), "reason": Null}), "nested_objects": Object({"outer": Object({"inner": Number(1.0)})}), "object_in_array_last": Array([Object({"a": Number(1.0)})]), "request": Object({"filename": String("example.rs"), "language": String("rust"), "options": Object({"features": Array([String("serde"), String("tokio")]), "opt_level": String("2")})}), "snippets": Array([String("fn main() {}"), String("println!(\"hi\")")]), "trailing": Object({"status": String("ok")})})}"#);
}
