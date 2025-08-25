#![expect(missing_docs)]
#![expect(clippy::needless_raw_string_hashes)]

use core::fmt::Write;

use jsonmodem::{JsonModemValues, ParserOptions, ValuesOptions};

fn render_values(stream: &[&str], partial: bool) -> String {
    let mut vals = if partial {
        JsonModemValues::with_options(ParserOptions::default(), ValuesOptions { partial: true })
    } else {
        JsonModemValues::new(ParserOptions::default())
    };
    let mut out = String::new();
    for ch in stream {
        for sv in vals.feed(ch) {
            let sv = sv.expect("values error");
            writeln!(
                out,
                "{{\"index\":{},\"is_final\":{},\"value\":{:?}}}",
                sv.index, sv.is_final, sv.value
            )
            .unwrap();
        }
    }
    out
}

#[test]
fn snapshot_values_partial_modes() {
    let stream: [&str; 5] = [
        r#"{"k":[1"#,
        r#",2,{"#,
        r#""x":"y"}],"#,
        r#""s":"he"#,
        r#"llo"}"#,
    ];

    // Unrolled to satisfy insta inline snapshot rules
    insta::assert_snapshot!(render_values(&stream, false), @r#"{"index":0,"is_final":true,"value":Object({"k": Array([Number(1.0), Number(2.0), Object({"x": String("y")})]), "s": String("hello")})}"#);
    insta::assert_snapshot!(render_values(&stream, true),  @r#"
    {"index":0,"is_final":false,"value":Object({"k": Array([])})}
    {"index":0,"is_final":false,"value":Object({"k": Array([Number(1.0), Number(2.0), Object({})])})}
    {"index":0,"is_final":false,"value":Object({"k": Array([Number(1.0), Number(2.0), Object({"x": String("y")})])})}
    {"index":0,"is_final":false,"value":Object({"k": Array([Number(1.0), Number(2.0), Object({"x": String("y")})]), "s": String("he")})}
    {"index":0,"is_final":true,"value":Object({"k": Array([Number(1.0), Number(2.0), Object({"x": String("y")})]), "s": String("hello")})}
    "#);
}
