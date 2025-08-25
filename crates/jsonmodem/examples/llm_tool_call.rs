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
    BufferOptions, BufferedEvent, JsonModem, JsonModemBuffers, JsonModemValues, ParserOptions,
    StringBufferMode, path,
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
            string_buffer_mode: StringBufferMode::Prefixes,
            non_scalar_mode: jsonmodem::NonScalarMode::None,
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
                BufferedEvent::ObjectEnd { path, .. } if path.is_empty() => {
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
    // Snapshots are generated in tests (see below).
}

// Snapshot tests for the layers live in `tests/snapshots_layers.rs`.
