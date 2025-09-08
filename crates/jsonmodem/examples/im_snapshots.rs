#![allow(missing_docs)]

#[cfg(feature = "im")]
fn main() {
    use jsonmodem::{JsonModemIm, ParserOptions, ValuesOptions};

    let mut snapshots = JsonModemIm::with_options(
        ParserOptions::default(),
        ValuesOptions::default().with_partial(true),
    );

    let chunks = [
        "{",
        "\"title\":\"",
        "he",
        "llo\"",
        ",\"active\":",
        "true",
        ",\"limits\":[",
        "1,",
        "2",
        ",3],",
        "\"metadata\":{",
        "\"env\":\"",
        "pr",
        "od\"",
        "}}",
    ];

    for chunk in &chunks {
        for view in snapshots.feed(chunk) {
            println!(
                "partial index={} final={} value={}",
                view.index, view.is_final, view.value
            );
        }
    }

    for view in snapshots.finish() {
        println!(
            "finish index={} final={} value={}",
            view.index, view.is_final, view.value
        );
    }
}

#[cfg(not(feature = "im"))]
fn main() {
    eprintln!(
        "Enable the `im` feature to run this example:\n  cargo run -p jsonmodem --features im --example im_snapshots"
    );
}
