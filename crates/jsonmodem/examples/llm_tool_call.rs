//! Demonstrates how to react **immediately** to content-moderation feedback
//! while incrementally streaming a tool-call response from an LLM.
//!
//! In this scenario we have prompted the assistant with a *tool description*
//! that yields a JSON object describing a code snippet the model has generated
//! for us.  Besides the actual snippet the object contains a `moderation`
//! field so that the model (or an upstream service) can flag policy
//! violations early on.
//!
//! The relevant part of the schema looks roughly as follows (abridged):
//!
//! ```text
//! {
//!   "moderation": {
//!     "decision": "allow" | "block",
//!     "reason":   string | null
//!   }
//!   "filename":   string,
//!   "language":   string,
//!   "code":       string,
//! }
//! ```
//!
//! The example below streams a *single* JSON document but feeds it to the
//! parser in small, irregular chunks to mirror how OpenAI's `chat.completions`
//! (and similar) APIs deliver partial tokens.  Two things happen while the
//! payload arrives:
//!
//! 1. As soon as the `moderation.decision` string prefixes to `"block"` we
//!    abort processing and surface an error to the caller – **before** the full
//!    response has even finished.
//! 2. Each fragment of the `code` string is printed to `stdout` as soon as it
//!    becomes available so that a user interface could, for instance, render
//!    the snippet character-by-character.
//!
//! Run with
//!
//! ```bash
//! cargo run -p jsonmodem --example llm_tool_call
//! ```

#![expect(clippy::needless_raw_string_hashes)]
#![expect(clippy::doc_markdown)]

use jsonmodem::{
    BufferOptions, BufferStringMode, BufferedEvent, JsonModem, JsonModemBuffers, JsonModemValues,
    ParserOptions, path,
};

fn main() {
    // A *toy* assistant response streamed in ten tiny chunks.  The
    // `moderation` object comes *first* so that backend code can decide early
    // whether to continue or abort before the rest of the payload (including
    // the potentially expensive code snippet) arrives.
    // In real life this would come from the network.
    let simulated_stream: [&str; 10] = [
        // 0 – start of object, moderation key
        r#"{"moderation":{"decision":"al"#,
        // 1 – continue decision
        r#"lo"#,
        // 2 – finish decision & reason
        r#"w","reason":null},"#,
        // 3 – filename key/value
        r#""filename":"example.rs","#,
        // 4 – language key/value
        r#""language":"rust","#,
        // 5 – code key and opening quote
        r#""code":"use jsonmodem::{StreamingParser, "#,
        // 6 – more code
        r#"ParserOptions};\nfn main() {\n"#,
        // 7
        r#"    let _parser = StreamingParser::new(ParserOptions::default());\n"#,
        // 8
        r#"    println!(\"Hello from jsonmodem!\");\n}\n"#,
        // 9 – close code string and object
        r#""}"#,
    ];

    // Configure the parser so that every `ParseEvent::String` carries the full
    // *prefix* seen so far.  This enables super-low-latency decisions.
    let mut parser = JsonModemBuffers::new(
        ParserOptions::default(),
        BufferOptions {
            string_values: BufferStringMode::Prefixes,
        },
    );

    // Keep track whether we are currently inside the `code` field so that we
    // can stream it to the user.
    let mut in_code_field = false;

    for chunk in simulated_stream {
        // Drain all events currently available.
        for evt in parser.feed(chunk) {
            let evt = evt.expect("parser error");

            match evt {
                // -------------------------------- moderaton ---------------------------------
                BufferedEvent::String {
                    path,
                    value: Some(prefix),
                    is_final,
                    ..
                } if path == path!["moderation", "decision"] => {
                    if prefix.starts_with("block") {
                        eprintln!("🚨  Moderation blocked the content – aborting");
                        return;
                    }

                    if is_final {
                        println!("✅  Moderation decision: {prefix}");
                    }
                }

                // ---------------------------------- code ------------------------------------
                BufferedEvent::String {
                    path,
                    fragment,
                    is_final,
                    ..
                } if path == path!["code"] => {
                    // We only write the *new* fragment (not the whole prefix)
                    print!("{fragment}");
                    in_code_field = !is_final;
                }

                // When we reach the end of the JSON object we are done.
                BufferedEvent::ObjectEnd { path } if path.is_empty() => {
                    println!();
                }

                _ => {}
            }
        }
    }

    if in_code_field {
        // If the LLM stream ended unexpectedly we might not have seen the end
        // of the code field – handle however is appropriate for your app.
        eprintln!("⚠️  Stream ended before code snippet was complete");
    }

    // Compare the three layers on the same input to show output shapes.
    #[cfg(not(miri))]
    snapshot_three_layers(&simulated_stream);
}

#[cfg(not(miri))]
fn snapshot_three_layers(stream: &[&str]) {
    use core::fmt::Write;

    // A small input with nested structure and strings, split across chunks.
    // 1) Core: JsonModem events (fragment-only strings)
    let mut core = JsonModem::new(ParserOptions::default());
    let mut core_lines = String::new();
    for ch in stream {
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

    // 2) Buffers: JsonModemBuffers in Values mode (emit full string on final)
    let mut buf = JsonModemBuffers::new(
        ParserOptions::default(),
        BufferOptions {
            string_values: BufferStringMode::Values,
        },
    );
    let mut buf_lines = String::new();
    for ch in stream {
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

    // 3) Values: JsonModemValues emits completed roots
    let mut vals = JsonModemValues::new(ParserOptions::default());
    let mut val_lines = String::new();
    for ch in stream {
        for sv in vals.feed(ch) {
            let sv = sv.expect("values error");
            // Minimal, stable-ish representation
            writeln!(
                val_lines,
                "{{\"index\":{},\"is_final\":{},\"value\":{:?}}}",
                sv.index, sv.is_final, sv.value
            )
            .unwrap();
        }
    }

    let mut snapshot = String::new();
    snapshot.push_str("-- JsonModem (core) --\n");
    snapshot.push_str(&core_lines);
    snapshot.push_str("\n-- JsonModemBuffers (Values) --\n");
    snapshot.push_str(&buf_lines);
    snapshot.push_str("\n-- JsonModemValues --\n");
    snapshot.push_str(&val_lines);

    // Buffers in Prefixes mode to showcase both buffering policies
    let mut bufp = JsonModemBuffers::new(
        ParserOptions::default(),
        BufferOptions { string_values: BufferStringMode::Prefixes },
    );
    let mut bufp_lines = String::new();
    for ch in stream {
        for ev in bufp.feed(ch) {
            let ev = ev.expect("buffers error");
            #[cfg(feature = "serde")]
            {
                bufp_lines.push_str(&serde_json::to_string(&ev).unwrap());
                bufp_lines.push('
');
            }
            #[cfg(not(feature = "serde"))]
            {
                use core::fmt::Write;
                writeln!(bufp_lines, "{ev:?}").unwrap();
            }
        }
    }

    // Separate snapshots per parser/mode
    insta::assert_snapshot!("tool_call_core", core_lines);
    insta::assert_snapshot!("tool_call_buffers_values", buf_lines);
    insta::assert_snapshot!("tool_call_buffers_prefixes", bufp_lines);
    insta::assert_snapshot!("tool_call_values", val_lines);

}
