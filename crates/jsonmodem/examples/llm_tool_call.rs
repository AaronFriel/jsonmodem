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

#![allow(clippy::needless_raw_string_hashes)]
#![allow(clippy::doc_markdown)]

use jsonmodem::{ParseEvent, ParserOptions, StreamingParser, StringValueMode, path};

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
    let mut parser = StreamingParser::new(ParserOptions {
        string_value_mode: StringValueMode::Prefixes,
        ..ParserOptions::default()
    });

    // Keep track whether we are currently inside the `code` field so that we
    // can stream it to the user.
    let mut in_code_field = false;

    // Snapshot accumulator – we want one JSON line per `ParseEvent` so that
    // `cargo insta` can show meaningful diffs whenever the event stream
    // changes.
    let mut reference_value = String::from("\n");

    for chunk in simulated_stream {
        parser.feed(chunk);

        // Drain all events currently available.
        for evt in parser.by_ref() {
            let evt = evt.expect("parser error");

            // Record a serialised copy of each event for the snapshot.
            #[cfg(feature = "serde")]
            {
                reference_value.push_str(&serde_json::to_string(&evt).unwrap());
                reference_value.push('\n');
            }

            match evt {
                // -------------------------------- moderaton ---------------------------------
                ParseEvent::String {
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
                ParseEvent::String {
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
                ParseEvent::ObjectEnd { path, .. } if path.is_empty() => {
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

    // Finally, verify that the produced event stream stays stable.  Run
    // `cargo insta review` after the first execution to approve the snapshot.
    #[cfg(not(miri))]
    insta::assert_snapshot!(reference_value, @r#"
    {"kind":"ObjectBegin","path":[]}
    {"kind":"ObjectBegin","path":["moderation"]}
    {"kind":"String","path":["moderation","decision"],"value":"al","fragment":"al"}
    {"kind":"String","path":["moderation","decision"],"value":"allo","fragment":"lo"}
    {"kind":"String","path":["moderation","decision"],"value":"allow","fragment":"w","is_final":true}
    {"kind":"Null","path":["moderation","reason"]}
    {"kind":"ObjectEnd","path":["moderation"]}
    {"kind":"String","path":["filename"],"value":"example.rs","fragment":"example.rs","is_final":true}
    {"kind":"String","path":["language"],"value":"rust","fragment":"rust","is_final":true}
    {"kind":"String","path":["code"],"value":"use jsonmodem::{StreamingParser, ","fragment":"use jsonmodem::{StreamingParser, "}
    {"kind":"String","path":["code"],"value":"use jsonmodem::{StreamingParser, ParserOptions};\nfn main() {\n","fragment":"ParserOptions};\nfn main() {\n"}
    {"kind":"String","path":["code"],"value":"use jsonmodem::{StreamingParser, ParserOptions};\nfn main() {\n    let _parser = StreamingParser::new(ParserOptions::default());\n","fragment":"    let _parser = StreamingParser::new(ParserOptions::default());\n"}
    {"kind":"String","path":["code"],"value":"use jsonmodem::{StreamingParser, ParserOptions};\nfn main() {\n    let _parser = StreamingParser::new(ParserOptions::default());\n    println!(\"Hello from jsonmodem!\");\n}\n","fragment":"    println!(\"Hello from jsonmodem!\");\n}\n"}
    {"kind":"String","path":["code"],"value":"use jsonmodem::{StreamingParser, ParserOptions};\nfn main() {\n    let _parser = StreamingParser::new(ParserOptions::default());\n    println!(\"Hello from jsonmodem!\");\n}\n","fragment":"","is_final":true}
    {"kind":"ObjectEnd","path":[]}
    "#);
}
