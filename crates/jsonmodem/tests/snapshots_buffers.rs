#![expect(missing_docs)]

mod common;

use jsonmodem::{BufferOptions, JsonModemBuffers, NonScalarMode, ParserOptions, StringBufferMode};

use crate::common::STREAM;

fn render_buffers(stream: &[&str], sm: StringBufferMode, nsm: NonScalarMode) -> String {
    let mut buf = JsonModemBuffers::new(
        ParserOptions::default(),
        BufferOptions {
            string_buffer_mode: sm,
            non_scalar_mode: nsm,
        },
    );
    let mut out = String::new();
    for ch in stream {
        for ev in buf.feed(ch) {
            let ev = ev.expect("buffers error");
            #[cfg(feature = "serde")]
            {
                out.push_str(&serde_json::to_string(&ev).unwrap());
                out.push('\n');
            }
            #[cfg(not(feature = "serde"))]
            {
                use core::fmt::Write;
                writeln!(out, "{ev:?}").unwrap();
            }
        }
    }
    out
}

#[test]
#[allow(clippy::too_many_lines)]
fn snapshot_buffers_permutations() {
    insta::assert_snapshot!(render_buffers(&STREAM, StringBufferMode::None,     NonScalarMode::None),   @r#"
    ObjectBegin { path: [] }
    ObjectBegin { path: [Key("moderation")] }
    String { path: [Key("moderation"), Key("decision")], fragment: "al", value: None, is_final: false }
    String { path: [Key("moderation"), Key("decision")], fragment: "lo", value: None, is_final: false }
    String { path: [Key("moderation"), Key("decision")], fragment: "w", value: None, is_final: true }
    Null { path: [Key("moderation"), Key("reason")] }
    ObjectEnd { path: [Key("moderation")], value: None }
    ObjectBegin { path: [Key("request")] }
    String { path: [Key("request"), Key("filename")], fragment: "example.rs", value: None, is_final: true }
    String { path: [Key("request"), Key("language")], fragment: "rust", value: None, is_final: true }
    ObjectBegin { path: [Key("request"), Key("options")] }
    String { path: [Key("request"), Key("options"), Key("opt_level")], fragment: "2", value: None, is_final: true }
    ArrayStart { path: [Key("request"), Key("options"), Key("features")] }
    String { path: [Key("request"), Key("options"), Key("features"), Index(0)], fragment: "serde", value: None, is_final: true }
    String { path: [Key("request"), Key("options"), Key("features"), Index(1)], fragment: "tokio", value: None, is_final: true }
    ArrayEnd { path: [Key("request"), Key("options"), Key("features")], value: None }
    ObjectEnd { path: [Key("request"), Key("options")], value: None }
    ObjectEnd { path: [Key("request")], value: None }
    ArrayStart { path: [Key("snippets")] }
    String { path: [Key("snippets"), Index(0)], fragment: "fn main() {}", value: None, is_final: true }
    String { path: [Key("snippets"), Index(1)], fragment: "println!(\"hi\")", value: None, is_final: true }
    ArrayEnd { path: [Key("snippets")], value: None }
    ArrayStart { path: [Key("entities")] }
    ObjectBegin { path: [Key("entities"), Index(0)] }
    String { path: [Key("entities"), Index(0), Key("type")], fragment: "function", value: None, is_final: true }
    String { path: [Key("entities"), Index(0), Key("name")], fragment: "main", value: None, is_final: true }
    ObjectEnd { path: [Key("entities"), Index(0)], value: None }
    ObjectBegin { path: [Key("entities"), Index(1)] }
    String { path: [Key("entities"), Index(1), Key("type")], fragment: "macro", value: None, is_final: true }
    String { path: [Key("entities"), Index(1), Key("name")], fragment: "println", value: None, is_final: true }
    ObjectEnd { path: [Key("entities"), Index(1)], value: None }
    ArrayEnd { path: [Key("entities")], value: None }
    ArrayStart { path: [Key("matrix")] }
    ArrayStart { path: [Key("matrix"), Index(0)] }
    String { path: [Key("matrix"), Index(0), Index(0)], fragment: "a", value: None, is_final: true }
    ArrayEnd { path: [Key("matrix"), Index(0)], value: None }
    ArrayEnd { path: [Key("matrix")], value: None }
    ArrayStart { path: [Key("mixed")] }
    String { path: [Key("mixed"), Index(0)], fragment: "s", value: None, is_final: true }
    ObjectBegin { path: [Key("mixed"), Index(1)] }
    String { path: [Key("mixed"), Index(1), Key("k")], fragment: "v", value: None, is_final: true }
    ObjectEnd { path: [Key("mixed"), Index(1)], value: None }
    String { path: [Key("mixed"), Index(2)], fragment: "t", value: None, is_final: true }
    ArrayStart { path: [Key("mixed"), Index(3)] }
    String { path: [Key("mixed"), Index(3), Index(0)], fragment: "u", value: None, is_final: true }
    ArrayEnd { path: [Key("mixed"), Index(3)], value: None }
    String { path: [Key("mixed"), Index(4)], fragment: "end", value: None, is_final: true }
    ArrayEnd { path: [Key("mixed")], value: None }
    ObjectBegin { path: [Key("trailing")] }
    String { path: [Key("trailing"), Key("status")], fragment: "ok", value: None, is_final: true }
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

    insta::assert_snapshot!(render_buffers(&STREAM, StringBufferMode::None,     NonScalarMode::Roots),  @r#"
    ObjectBegin { path: [] }
    ObjectBegin { path: [Key("moderation")] }
    String { path: [Key("moderation"), Key("decision")], fragment: "al", value: None, is_final: false }
    String { path: [Key("moderation"), Key("decision")], fragment: "lo", value: None, is_final: false }
    String { path: [Key("moderation"), Key("decision")], fragment: "w", value: None, is_final: true }
    Null { path: [Key("moderation"), Key("reason")] }
    ObjectEnd { path: [Key("moderation")], value: None }
    ObjectBegin { path: [Key("request")] }
    String { path: [Key("request"), Key("filename")], fragment: "example.rs", value: None, is_final: true }
    String { path: [Key("request"), Key("language")], fragment: "rust", value: None, is_final: true }
    ObjectBegin { path: [Key("request"), Key("options")] }
    String { path: [Key("request"), Key("options"), Key("opt_level")], fragment: "2", value: None, is_final: true }
    ArrayStart { path: [Key("request"), Key("options"), Key("features")] }
    String { path: [Key("request"), Key("options"), Key("features"), Index(0)], fragment: "serde", value: None, is_final: true }
    String { path: [Key("request"), Key("options"), Key("features"), Index(1)], fragment: "tokio", value: None, is_final: true }
    ArrayEnd { path: [Key("request"), Key("options"), Key("features")], value: None }
    ObjectEnd { path: [Key("request"), Key("options")], value: None }
    ObjectEnd { path: [Key("request")], value: None }
    ArrayStart { path: [Key("snippets")] }
    String { path: [Key("snippets"), Index(0)], fragment: "fn main() {}", value: None, is_final: true }
    String { path: [Key("snippets"), Index(1)], fragment: "println!(\"hi\")", value: None, is_final: true }
    ArrayEnd { path: [Key("snippets")], value: None }
    ArrayStart { path: [Key("entities")] }
    ObjectBegin { path: [Key("entities"), Index(0)] }
    String { path: [Key("entities"), Index(0), Key("type")], fragment: "function", value: None, is_final: true }
    String { path: [Key("entities"), Index(0), Key("name")], fragment: "main", value: None, is_final: true }
    ObjectEnd { path: [Key("entities"), Index(0)], value: None }
    ObjectBegin { path: [Key("entities"), Index(1)] }
    String { path: [Key("entities"), Index(1), Key("type")], fragment: "macro", value: None, is_final: true }
    String { path: [Key("entities"), Index(1), Key("name")], fragment: "println", value: None, is_final: true }
    ObjectEnd { path: [Key("entities"), Index(1)], value: None }
    ArrayEnd { path: [Key("entities")], value: None }
    ArrayStart { path: [Key("matrix")] }
    ArrayStart { path: [Key("matrix"), Index(0)] }
    String { path: [Key("matrix"), Index(0), Index(0)], fragment: "a", value: None, is_final: true }
    ArrayEnd { path: [Key("matrix"), Index(0)], value: None }
    ArrayEnd { path: [Key("matrix")], value: None }
    ArrayStart { path: [Key("mixed")] }
    String { path: [Key("mixed"), Index(0)], fragment: "s", value: None, is_final: true }
    ObjectBegin { path: [Key("mixed"), Index(1)] }
    String { path: [Key("mixed"), Index(1), Key("k")], fragment: "v", value: None, is_final: true }
    ObjectEnd { path: [Key("mixed"), Index(1)], value: None }
    String { path: [Key("mixed"), Index(2)], fragment: "t", value: None, is_final: true }
    ArrayStart { path: [Key("mixed"), Index(3)] }
    String { path: [Key("mixed"), Index(3), Index(0)], fragment: "u", value: None, is_final: true }
    ArrayEnd { path: [Key("mixed"), Index(3)], value: None }
    String { path: [Key("mixed"), Index(4)], fragment: "end", value: None, is_final: true }
    ArrayEnd { path: [Key("mixed")], value: None }
    ObjectBegin { path: [Key("trailing")] }
    String { path: [Key("trailing"), Key("status")], fragment: "ok", value: None, is_final: true }
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

    insta::assert_snapshot!(render_buffers(&STREAM, StringBufferMode::None,     NonScalarMode::All),    @r#"
    ObjectBegin { path: [] }
    ObjectBegin { path: [Key("moderation")] }
    String { path: [Key("moderation"), Key("decision")], fragment: "al", value: None, is_final: false }
    String { path: [Key("moderation"), Key("decision")], fragment: "lo", value: None, is_final: false }
    String { path: [Key("moderation"), Key("decision")], fragment: "w", value: None, is_final: true }
    Null { path: [Key("moderation"), Key("reason")] }
    ObjectEnd { path: [Key("moderation")], value: None }
    ObjectBegin { path: [Key("request")] }
    String { path: [Key("request"), Key("filename")], fragment: "example.rs", value: None, is_final: true }
    String { path: [Key("request"), Key("language")], fragment: "rust", value: None, is_final: true }
    ObjectBegin { path: [Key("request"), Key("options")] }
    String { path: [Key("request"), Key("options"), Key("opt_level")], fragment: "2", value: None, is_final: true }
    ArrayStart { path: [Key("request"), Key("options"), Key("features")] }
    String { path: [Key("request"), Key("options"), Key("features"), Index(0)], fragment: "serde", value: None, is_final: true }
    String { path: [Key("request"), Key("options"), Key("features"), Index(1)], fragment: "tokio", value: None, is_final: true }
    ArrayEnd { path: [Key("request"), Key("options"), Key("features")], value: None }
    ObjectEnd { path: [Key("request"), Key("options")], value: None }
    ObjectEnd { path: [Key("request")], value: None }
    ArrayStart { path: [Key("snippets")] }
    String { path: [Key("snippets"), Index(0)], fragment: "fn main() {}", value: None, is_final: true }
    String { path: [Key("snippets"), Index(1)], fragment: "println!(\"hi\")", value: None, is_final: true }
    ArrayEnd { path: [Key("snippets")], value: None }
    ArrayStart { path: [Key("entities")] }
    ObjectBegin { path: [Key("entities"), Index(0)] }
    String { path: [Key("entities"), Index(0), Key("type")], fragment: "function", value: None, is_final: true }
    String { path: [Key("entities"), Index(0), Key("name")], fragment: "main", value: None, is_final: true }
    ObjectEnd { path: [Key("entities"), Index(0)], value: None }
    ObjectBegin { path: [Key("entities"), Index(1)] }
    String { path: [Key("entities"), Index(1), Key("type")], fragment: "macro", value: None, is_final: true }
    String { path: [Key("entities"), Index(1), Key("name")], fragment: "println", value: None, is_final: true }
    ObjectEnd { path: [Key("entities"), Index(1)], value: None }
    ArrayEnd { path: [Key("entities")], value: None }
    ArrayStart { path: [Key("matrix")] }
    ArrayStart { path: [Key("matrix"), Index(0)] }
    String { path: [Key("matrix"), Index(0), Index(0)], fragment: "a", value: None, is_final: true }
    ArrayEnd { path: [Key("matrix"), Index(0)], value: None }
    ArrayEnd { path: [Key("matrix")], value: None }
    ArrayStart { path: [Key("mixed")] }
    String { path: [Key("mixed"), Index(0)], fragment: "s", value: None, is_final: true }
    ObjectBegin { path: [Key("mixed"), Index(1)] }
    String { path: [Key("mixed"), Index(1), Key("k")], fragment: "v", value: None, is_final: true }
    ObjectEnd { path: [Key("mixed"), Index(1)], value: None }
    String { path: [Key("mixed"), Index(2)], fragment: "t", value: None, is_final: true }
    ArrayStart { path: [Key("mixed"), Index(3)] }
    String { path: [Key("mixed"), Index(3), Index(0)], fragment: "u", value: None, is_final: true }
    ArrayEnd { path: [Key("mixed"), Index(3)], value: None }
    String { path: [Key("mixed"), Index(4)], fragment: "end", value: None, is_final: true }
    ArrayEnd { path: [Key("mixed")], value: None }
    ObjectBegin { path: [Key("trailing")] }
    String { path: [Key("trailing"), Key("status")], fragment: "ok", value: None, is_final: true }
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

    insta::assert_snapshot!(render_buffers(&STREAM, StringBufferMode::Values,   NonScalarMode::None),   @r#"
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

    insta::assert_snapshot!(render_buffers(&STREAM, StringBufferMode::Values,   NonScalarMode::Roots),  @r#"
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

    insta::assert_snapshot!(render_buffers(&STREAM, StringBufferMode::Values,   NonScalarMode::All),    @r#"
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

    insta::assert_snapshot!(render_buffers(&STREAM, StringBufferMode::Prefixes, NonScalarMode::None),   @r#"
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

    insta::assert_snapshot!(render_buffers(&STREAM, StringBufferMode::Prefixes, NonScalarMode::Roots),  @r#"
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

    insta::assert_snapshot!(render_buffers(&STREAM, StringBufferMode::Prefixes, NonScalarMode::All),    @r#"
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
}
