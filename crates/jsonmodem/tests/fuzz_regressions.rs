#![cfg(debug_assertions)]

//! Regression tests that pin down fuzz-discovered panics caught by the
//! jsonmodem fuzz targets. Each case stores the reconstructed JSON payload plus
//! the fuzz mutator's flag byte and splitting seed so future runs reproduce the
//! same chunk boundaries. Keeping the payloads as readable JSON (rather than
//! raw byte dumps) makes it much easier to reason about the failing structure
//! when investigating or shrinking the repro.

use core::fmt::Write as _;

use jsonmodem::{
    BufferOptions, JsonModem, JsonModemBuffers, JsonModemValues, ParserOptions, ValuesOptions,
};

#[derive(Copy, Clone)]
enum Harness {
    Values,
    Buffers,
}

struct FuzzCase {
    name: &'static str,
    harness: Harness,
    flags: u8,
    split_seed: u32,
    payload: &'static str,
    description: &'static str,
}

fn parser_options(flags: u8) -> ParserOptions {
    ParserOptions::default()
        .with_allow_multiple_json_values(flags & 1 != 0)
        .with_allow_uppercase_u(flags & 2 != 0)
        .with_allow_unicode_whitespace(flags & 4 != 0)
        .with_panic_on_error(false)
}

fn values_options(flags: u8) -> ValuesOptions {
    ValuesOptions::default().with_partial(flags & 0x10 != 0)
}

fn chunks_for_case(case: &FuzzCase) -> (ParserOptions, Vec<String>) {
    let payload = case.payload.as_bytes();
    let split_seed = usize::try_from(case.split_seed).unwrap();

    let mut chunks = Vec::new();
    let mut start = 0;
    while start < payload.len() {
        let remaining = payload.len() - start;
        let mut size = (split_seed % remaining).saturating_add(1);
        while start + size < payload.len() && (payload[start + size] & 0xC0) == 0x80 {
            size += 1;
        }
        chunks.push(String::from_utf8_lossy(&payload[start..start + size]).into_owned());
        start += size;
    }

    (parser_options(case.flags), chunks)
}

fn consume_results<I, T, E>(iter: I)
where
    I: IntoIterator<Item = Result<T, E>>,
{
    for item in iter {
        match item {
            Ok(_) | Err(_) => {}
        }
    }
}

fn run_case(case: &FuzzCase) {
    let (options, chunks) = chunks_for_case(case);
    #[cfg(test)]
    {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&chunks.join("")) {
            eprintln!("{}: {}\n{json:#}", case.name, case.description);
        }
    }

    match case.harness {
        Harness::Values => {
            let mut parser = JsonModemValues::with_options(options, values_options(case.flags));
            for chunk in chunks {
                consume_results(parser.feed(&chunk));
            }
            consume_results(parser.finish());
        }
        Harness::Buffers => {
            let mut parser = JsonModemBuffers::new(options, BufferOptions::default());
            for chunk in chunks {
                consume_results(parser.feed(&chunk).to_iter());
            }
            consume_results(parser.finish().to_iter());
        }
    }
}

#[ignore = "debug-only helper"]
#[test]
fn debug_events_for_values_case_three() {
    let case = FuzzCase {
        name: "values_case_three",
        harness: Harness::Values,
        flags: 22,
        split_seed: 3_180_124_912,
        payload: concat!(
            "\u{2003}\u{2002}\u{2002}\u{2004}\u{2005}\u{2008}\u{3000}{\"\":\"\"}\u{2029}",
            "\u{2005}\u{202F}\u{2009}\u{2001}{\"\\u0014\":\"\"}\u{2007}\u{200A}\t\u{2008}",
            " \u{200A}\t\r\u{2028}{\"\":[[{\";\\r\":{\"\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}",
            "\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\":\"\",\"",
            "\u{7B6D}6\\u001f)\":{}}],\"*\"]]+\"\\u0007n\\u001b\\n\":{},\"\\u000bA",
            "\u{007F}\":\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\",\"<n\":\"\u{FFFD}\u{FFFD}",
            "\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\\u0004\\f\"}}]],\"Kc\\r",
            "\u{01C3}/\":[\"\"]}\u{2004}\u{2028}"
        ),
        description: "debug print of core parser events for values_case_three",
    };
    let (mut options, chunks) = chunks_for_case(&case);
    options = options
        .with_allow_multiple_json_values(true)
        .with_panic_on_error(false);
    let mut parser: JsonModem<jsonmodem::StdBackend> = JsonModem::new(options);
    let mut out = String::new();
    let _ = writeln!(&mut out, "chunks = {}", chunks.len());
    for (i, chunk) in chunks.iter().enumerate() {
        let _ = writeln!(&mut out, "feed#{i} chunk='{chunk}'");
        for evt in parser.feed(chunk).to_iter() {
            match evt {
                Ok(e) => {
                    let _ = writeln!(&mut out, "feed#{i} -> {e:?}");
                }
                Err(e) => {
                    let _ = writeln!(&mut out, "feed#{i} -> ERROR: {e:?}");
                }
            }
        }
    }
    out.push_str("finish()...\n");
    for evt in parser.finish().to_iter() {
        match evt {
            Ok(e) => {
                let _ = writeln!(&mut out, "finish -> {e:?}");
            }
            Err(e) => {
                let _ = writeln!(&mut out, "finish -> ERROR: {e:?}");
            }
        }
    }
    println!("{out}");
}

#[ignore = "debug-only helper"]
#[test]
fn debug_events_for_buffers_case_two() {
    let case = FuzzCase {
        name: "buffers_case_two",
        harness: Harness::Buffers,
        flags: 6,
        split_seed: 1_875_414_582,
        payload: concat!(
            "\u{2006}\u{2000}\u{2028}{\"<\\b(\":[{},\"<Ob(Q\":false}\u{2028}\u{2005}",
            "\u{2005}\u{2002}\":[{}\u{FFFD}\u{FFFD}\r.\u{FFFD}U\u{0000}}\u{0006}[[[[[[[[[",
            "[[[[[[[[[[[[[\u{000B}\"u\u{FFFD}\u{029D}\u{0300}\u{FFFD}\u{2003}"
        ),
        description: "debug events for buffers_case_two",
    };
    let (mut options, chunks) = chunks_for_case(&case);
    options = options.with_allow_multiple_json_values(true);
    let mut parser: JsonModem<jsonmodem::StdBackend> = JsonModem::new(options);
    let mut out = String::new();
    let _ = writeln!(&mut out, "chunks = {}", chunks.len());
    for (i, chunk) in chunks.iter().enumerate() {
        let _ = writeln!(&mut out, "feed#{i} chunk='{chunk}'");
        for evt in parser.feed(chunk).to_iter() {
            match evt {
                Ok(e) => {
                    let _ = writeln!(&mut out, "feed#{i} -> {e:?}");
                }
                Err(e) => {
                    let _ = writeln!(&mut out, "feed#{i} -> ERROR: {e:?}");
                }
            }
        }
    }
    out.push_str("finish()...\n");
    for evt in parser.finish().to_iter() {
        match evt {
            Ok(e) => {
                let _ = writeln!(&mut out, "finish -> {e:?}");
            }
            Err(e) => {
                let _ = writeln!(&mut out, "finish -> ERROR: {e:?}");
            }
        }
    }
    println!("{out}");
}

#[test]
fn fuzz_transition_regression_values_case_one() {
    run_case(&FuzzCase {
        name: "values_case_one",
        harness: Harness::Values,
        flags: 5,
        split_seed: 2_401_737_659,
        payload: "\u{202F}\u{3000}\u{3000} {\"g\":{\"\":{},\"M\":\"x\"}}\u{2028}\u{200A}",
        description: "multi-root stream that triggered a depth transition panic after finishing a root",
    });
}

#[test]
fn fuzz_transition_regression_values_case_two() {
    run_case(&FuzzCase {
        name: "values_case_two",
        harness: Harness::Values,
        flags: 30,
        split_seed: 3_910_507_698,
        payload: concat!(
            "\u{200A} {\"\":{\"\":{},\"\\u000C\\u0001\":\"*\"}}",
            "\u{1680}\u{2009}\n",
            "\u{1680}\u{202F}\u{2004}\u{2005}{\"\\u0015\\u034F:\\u0001m/\\u0010\":{}}",
            "\u{2004}\u{2007}\n",
            "\u{2000}\u{2029}"
        ),
        description: "values adapter panic when encountering control escapes and whitespace heavy chunking",
    });
}

#[test]
fn fuzz_regression_values_case_three() {
    run_case(&FuzzCase {
        name: "values_case_three",
        harness: Harness::Values,
        flags: 22,
        split_seed: 3_180_124_912,
        payload: concat!(
            "\u{2003}\u{2002}\u{2002}\u{2004}\u{2005}\u{2008}\u{3000}{\"\":\"\"}\u{2029}",
            "\u{2005}\u{202F}\u{2009}\u{2001}{\"\\u0014\":\"\"}\u{2007}\u{200A}\t\u{2008}",
            " \u{200A}\t\r\u{2028}{\"\":[[{\";\\r\":{\"\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}",
            "\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\":\"\",\"",
            "\u{7B6D}6\\u001f)\":{}}],\"*\"]]+\"\\u0007n\\u001b\\n\":{},\"\\u000bA",
            "\u{007F}\":\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\",\"<n\":\"\u{FFFD}\u{FFFD}",
            "\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\\u0004\\f\"}}]],\"Kc\\r",
            "\u{01C3}/\":[\"\"]}\u{2004}\u{2028}"
        ),
        description: concat!(
            "values adapter panic while emitting partial snapshots that combine heavy ",
            "unicode whitespace and lossy UTF-8 replacements",
        ),
    });
}

#[test]
fn sanity_values_multi_roots_simple() {
    run_case(&FuzzCase {
        name: "sanity_values_multi_roots_simple",
        harness: Harness::Values,
        flags: 1 | 4, // allow multiple + unicode whitespace
        split_seed: 1,
        payload: "{}[]",
        description: "sanity check for multi-root object followed by array",
    });
}

#[test]
fn fuzz_transition_regression_buffers_case_one() {
    run_case(&FuzzCase {
        name: "buffers_case_one",
        harness: Harness::Buffers,
        flags: 6,
        split_seed: 1_491_973_754,
        payload: concat!(
            "\u{2001}\u{205F} \r\r\u{2001}\u{2001}\r",
            "{\"\":{\"\":{},\"ȯ\\u0010\":[[],[],{\"u\\u0019\\u0007p\\ti\":\"\"}],\"ތ\":[\"\"]},",
            "\"\\u001ao\":[{}],\"8\":\"\"}",
            "\u{2000}\u{2001}"
        ),
        description: "buffers adapter panic after closing an object and immediately opening an array at root",
    });
}

#[test]
fn fuzz_regression_buffers_case_two() {
    run_case(&FuzzCase {
        name: "buffers_case_two",
        harness: Harness::Buffers,
        flags: 6,
        split_seed: 1_875_414_582,
        payload: concat!(
            "\u{2006}\u{2000}\u{2028}{\"<\\b(\":[{},\"<Ob(Q\":false}\u{2028}\u{2005}",
            "\u{2005}\u{2002}\":[{}\u{FFFD}\u{FFFD}\r.\u{FFFD}U\u{0000}}\u{0006}[[[[[[[[[",
            "[[[[[[[[[[[[[\u{000B}\"u\u{FFFD}\u{029D}\u{0300}\u{FFFD}\u{2003}"
        ),
        description: concat!(
            "buffers adapter panic triggered by lossy bytes followed by a dense ",
            "bracket burst at the root",
        ),
    });
}
