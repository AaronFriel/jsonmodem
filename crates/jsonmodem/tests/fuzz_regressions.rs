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

#[ignore = "debug-only helper"]
#[test]
fn debug_events_for_buffers_case_three() {
    let case = FuzzCase {
        name: "buffers_case_three",
        harness: Harness::Buffers,
        flags: 30,
        split_seed: 1_017_481_773,
        payload: concat!(
            "\u{0009}\u{000D}\u{2001}\u{2006}\u{2001}\u{2029}\u{2004} {\"\":[],\"\\u0005\":\"yS\",
            \"\\\\\"\":{\"\":{}},\"K\":\"_\",\"gA\":[[],\"a\\\\\"\\u000b\"]},\"Y`\\u001c\":\"\"}]",
            "\u{202F}\u{2007}\u{2009}\u{2004}\u{000A}\u{2003}\u{1680}\u{2029}\u{2001}\u{2005}\u{000A}\u{202F}\u{202F}\u{2000}\u{1680}\u{2002}\u{2005}\u{2029}[]\u{3000}",
            "\u{205F}\u{1680}\u{000D}\u{FFFD}\u{2003}\u{1680}\u{1680}\u{FFFD}\u{0009}\u{000D}\u{202F}\u{2004}[]\u{2029}\u{1680}\u{2008}\u{0009}\u{2001}\u{2029}",
        ),
        description: "debug trace for buffers_case_three fuzz crash",
    };
    let (options, chunks) = chunks_for_case(&case);
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
fn debug_buffers_iter_for_case_three() {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    let case = FuzzCase {
        name: "buffers_case_three",
        harness: Harness::Buffers,
        flags: 30,
        split_seed: 1_017_481_773,
        payload: concat!(
            "\u{0009}\u{000D}\u{2001}\u{2006}\u{2001}\u{2029}\u{2004} {\"\":[],\"\\u0005\":\"yS\",
            \"\\\\\"\":{\"\":{}},\"K\":\"_\",\"gA\":[[],\"a\\\\\"\\u000b\"]},\"Y`\\u001c\":\"\"}]",
            "\u{202F}\u{2007}\u{2009}\u{2004}\u{000A}\u{2003}\u{1680}\u{2029}\u{2001}\u{2005}\u{000A}\u{202F}\u{202F}\u{2000}\u{1680}\u{2002}\u{2005}\u{2029}[]\u{3000}",
            "\u{205F}\u{1680}\u{000D}\u{FFFD}\u{2003}\u{1680}\u{1680}\u{FFFD}\u{0009}\u{000D}\u{202F}\u{2004}[]\u{2029}\u{1680}\u{2008}\u{0009}\u{2001}\u{2029}",
        ),
        description: "buffers iterator panic trace for buffers_case_three",
    };

    let (options, chunks) = chunks_for_case(&case);
    let mut parser = JsonModemBuffers::new(options, BufferOptions::default());

    for (i, chunk) in chunks.iter().enumerate() {
        println!("feed#{i} chunk='{chunk}'");
        let mut iter = parser.feed(chunk).to_iter();
        let mut event_idx = 0usize;
        loop {
            let next = catch_unwind(AssertUnwindSafe(|| iter.next()));
            match next {
                Ok(Some(Ok(evt))) => {
                    println!("  event#{event_idx}: {evt:?}");
                }
                Ok(Some(Err(err))) => {
                    println!("  event#{event_idx}: ERROR {err:?}");
                }
                Ok(None) => break,
                Err(_) => {
                    println!("  panic while decoding event#{event_idx}");
                    panic!("buffers iterator panic");
                }
            }
            event_idx += 1;
        }
    }

    println!("finish()...");
    let mut closed = parser.finish().to_iter();
    let mut event_idx = 0usize;
    loop {
        let next = catch_unwind(AssertUnwindSafe(|| closed.next()));
        match next {
            Ok(Some(Ok(evt))) => {
                println!("  finish event#{event_idx}: {evt:?}");
            }
            Ok(Some(Err(err))) => println!("  finish event#{event_idx}: ERROR {err:?}"),
            Ok(None) => break,
            Err(_) => {
                println!("  panic while decoding finish event#{event_idx}");
                panic!("buffers iterator finish panic");
            }
        }
        event_idx += 1;
    }
}

// CI regression: fuzz_jsonmodem_buffers crash-ec7fe34d57815d37
#[test]
fn fuzz_ci_buffers_ec7fe34d57815d37() {
    let bytes: &[u8] = &[
        44, 34, 107, 34, 58, 123, 34, 34, 58, 123, 34, 34, 58, 34, 34, 125, 44, 34, 92, 102, 34,
        58, 123, 125, 44, 34, 90, 34, 58, 34, 92, 117, 48, 48, 49, 57, 34, 125, 125, 44, 34, 37,
        56, 58, 123, 125, 44, 34, 39, 33, 83, 34, 58, 123, 34, 34, 58, 110, 117, 108, 108, 44, 34,
        92, 110, 94, 34, 58, 102, 97, 108, 115, 101, 125, 44, 34, 91, 34, 58, 91, 93, 44, 34, 110,
        68, 34, 58, 34, 34, 44, 34, 118, 38, 125, 34, 58, 34, 92, 117, 48, 48, 49, 102, 126, 236,
        170, 183, 218, 218, 218, 218, 218, 218, 48, 49, 99, 99, 197, 139, 34, 125, 44, 34, 111, 92,
        117, 48, 48, 49, 98, 34, 58, 34, 126, 34, 125, 44, 34, 34, 93, 32, 226, 128, 134, 226, 128,
        134,
    ];
    let text = String::from_utf8_lossy(bytes).into_owned();
    let options = ParserOptions::default()
        .with_allow_multiple_json_values(true)
        .with_allow_unicode_whitespace(true)
        .with_panic_on_error(false);
    let mut parser = JsonModemBuffers::new(options, BufferOptions::default());
    consume_results(parser.feed(&text).to_iter());
    consume_results(parser.finish().to_iter());
}

// CI regression: fuzz_jsonmodem_values crash-ba00f820dde9c446
#[test]
fn fuzz_ci_values_ba00f820dde9c446() {
    let bytes: &[u8] = &[
        6, 249, 7, 84, 43, 226, 128, 134, 116, 114, 117, 101, 226, 128, 131, 226, 128, 132, 226,
        128, 133, 226, 128, 168, 34, 52, 34, 226, 128, 168, 9, 226, 128, 130, 226, 128, 136, 226,
        128, 168, 226, 128, 132, 226, 128, 169, 226, 128, 128, 225, 154, 128, 225, 154, 128, 226,
        128, 137, 123, 125, 226, 128, 138, 226, 128, 134, 10, 32, 226, 128, 137, 226, 128, 129, 34,
        83, 34, 226, 128, 169, 226, 128, 130, 226, 128, 129, 226, 128, 130, 226, 128, 168, 9, 226,
        128, 136, 226, 128, 168, 123, 34, 34, 58, 123, 34, 34, 58, 52, 46, 55, 57, 56, 48, 54, 52,
        56, 49, 56, 57, 55, 57, 52, 51, 52, 101, 50, 50, 51, 44, 34, 36, 106, 46, 51, 34, 58, 91,
        34, 34, 44, 91, 34, 92, 117, 48, 48, 48, 48, 34, 44, 91, 123, 34, 34, 58, 123, 34, 34, 58,
        91, 34, 34, 44, 91, 34, 84, 34, 44, 123, 34, 34, 58, 91, 123, 125, 93, 44, 34, 86, 36, 36,
        36, 36, 36, 36, 36, 36, 36, 36, 36, 34, 58, 123, 34, 34, 58, 123, 34, 47, 34, 58, 91, 91,
        93, 44, 123, 125, 44, 34, 112, 34, 93, 44, 34, 49, 43, 34, 58, 34, 34, 125, 125, 44, 34,
        42, 90, 92, 117, 48, 48, 48, 101, 90, 69, 34, 58, 34, 34, 44, 34, 44, 101, 34, 58, 123, 34,
        59, 77, 92, 117, 48, 48, 48, 98, 34, 58, 50, 46, 51, 57, 56, 54, 53, 53, 52, 52, 53, 52,
        53, 48, 52, 50, 56, 51, 101, 49, 49, 55, 125, 125, 125, 44, 34, 73, 44, 34, 44, 91, 93, 93,
        44, 34, 71, 34, 44, 123, 125, 93, 125, 125, 93, 93, 93, 44, 34, 60, 116, 34, 58, 34, 34,
        125, 125, 226, 128, 133, 226, 128, 129,
    ];
    let text = String::from_utf8_lossy(bytes).into_owned();
    let options = ParserOptions::default()
        .with_allow_multiple_json_values(true)
        .with_allow_unicode_whitespace(true)
        .with_panic_on_error(false);
    let mut parser = JsonModemValues::with_options(options, ValuesOptions::default());
    consume_results(parser.feed(&text));
    consume_results(parser.finish());
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

#[test]
fn fuzz_candidate_from_user_buffers() {
    // Derived from the Provided FuzzerInput (flags expanded explicitly).
    let options = ParserOptions::default()
        .with_allow_multiple_json_values(false)
        .with_allow_uppercase_u(false)
        .with_allow_unicode_whitespace(false)
        .with_panic_on_error(false);
    let mut parser = JsonModemBuffers::new(options, BufferOptions::default());
    let chunks: &[&str] = &[
        "{{\"\\u0000\\u000f\":{\"\":{\"\":\"9~%\\r\"},\"\\u0007\\u0007\\u0007 #ɿ\":\"\"]\",\"\\u0016\":[[\"\",\"]\",[],{\"\":\"\"},{\"]e\":[true,6.6606492817934915e165,1.2201e5911614548592e185,\"'\\\":\\\"S\\\\u0019J\\\"},null,1.\"]},null,null,false]],\"\\u001dI\":{\"\\t\":{\"\\u0016\":[[{},[-9159615202055668737,[],null],null],true,[]],\"!\":{\"\":{\"\":{\"\":true},\",sU^\":[],\"k\\u0002\":\"\\u0005\",\"ю\":true}}}},\" \":{\"<\":null},\"\\\"\\b\":{},\"'M\":[[],-2.72817",
        "46600704903e-237,false],\",sU>\\u0013;\":5254487156537939167,\"=\":\".(\",\"G\":null,\"O\u{7f}\"",
        ":true,\"e,\\\"\":\"$*\",\"h\":true,\"",
        "i t\":null,\"i-\":null,\"k\\u0002\":\"\\u00",
        "05\",\"n$u^\":[],\"ϣ\":[[],{},7.484207260355667e251],\"\u{484}c3T\":\"\"},\"~M\":{}}}",
    ];
    for c in chunks {
        consume_results(parser.feed(c).to_iter());
    }
    consume_results(parser.finish().to_iter());
}

#[test]
fn fuzz_candidate_from_user_values() {
    let options = ParserOptions::default()
        .with_allow_multiple_json_values(false)
        .with_allow_uppercase_u(false)
        .with_allow_unicode_whitespace(false)
        .with_panic_on_error(false);
    let mut parser =
        JsonModemValues::with_options(options, ValuesOptions::default().with_partial(true));
    let chunks: &[&str] = &[
        "{{\"\\u0000\\u000f\":{\"\":{\"\":\"9~%\\r\"},\"\\u0007\\u0007\\u0007 #ɿ\":\"\"]\",\"\\u0016\":[[\"\",\"]\",[],{\"\":\"\"},{\"]e\":[true,6.6606492817934915e165,1.2201e5911614548592e185,\"'\\\":\\\"S\\\\u0019J\\\"},null,1.\"]},null,null,false]],\"\\u001dI\":{\"\\t\":{\"\\u0016\":[[{},[-9159615202055668737,[],null],null],true,[]],\"!\":{\"\":{\"\":{\"\":true},\",sU^\":[],\"k\\u0002\":\"\\u0005\",\"ю\":true}}}},\" \":{\"<\":null},\"\\\"\\b\":{},\"'M\":[[],-2.72817",
        "46600704903e-237,false],\",sU>\\u0013;\":5254487156537939167,\"=\":\".(\",\"G\":null,\"O\u{7f}\"",
        ":true,\"e,\\\"\":\"$*\",\"h\":true,\"",
        "i t\":null,\"i-\":null,\"k\\u0002\":\"\\u00",
        "05\",\"n$u^\":[],\"ϣ\":[[],{},7.484207260355667e251],\"\u{484}c3T\":\"\"},\"~M\":{}}}",
    ];
    for c in chunks {
        consume_results(parser.feed(c));
    }
    consume_results(parser.finish());
}

// Repro for fuzz-discovered abort in buffers harness (see fuzz artifact
// crash-057e8a9e64884aa3)
#[test]
fn fuzz_ci_buffers_057e8a9e64884aa3() {
    // Flags from debug print: allow_multiple_json_values=true, uppercase_u=false,
    // unicode_ws=false, partial_values=true. For buffers harness, only the first
    // matters.
    let mut parser = JsonModemBuffers::new(parser_options(1), BufferOptions::default());
    let chunks: &[&str] = &[
        "{\"\":[[null,\"\\u001bV\\u001d\",9144662900591180799,{},{\",sb\":[[{\"\":{\"\":{\"\":{\"\":{\"\":{\"\":{\"\":\"xN'\",\"$Y\":{\"\":true,\"94_\":{\"\":{\"k\":13459907907603343736}}},\"/ۦҷ\\n\\u0007\":{\"\":null,\"\\u001dI\":{\"\\t\":{\"!\":{\"\":{\"\":{\"\":{\"\":{\"\":{}},\"\\u0002\\u0002\\u0002\\u0002\":{}-,\"f\\u0007Z7\":true},\"\u{200a}\u{2001}{}\":[[[{}],true],117664469269",
        "997844{37,[],[],[{}\\\\,[]]]}}}}},\" \":{\"<\":null},\"=\":\".(",
        "\",{\"O\u{7f}\":true,\"h\"",
        ":{\"\":true}}}}}}}}}]]}]]}",
    ];
    for chunk in chunks {
        consume_results(parser.feed(chunk).to_iter());
    }
    consume_results(parser.finish().to_iter());
}
