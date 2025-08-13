#![cfg(debug_assertions)]

//! Regression tests that pin down fuzz-discovered panics inside the debug
//! transition checker.  Each case stores the reconstructed JSON payload plus
//! the fuzz mutator's flag byte and splitting seed so future runs reproduce
//! the same chunk boundaries.  Keeping the payloads as readable JSON (rather
//! than raw byte dumps) makes it much easier to reason about the failing
//! structure when investigating or shrinking the repro.

use jsonmodem::{BufferOptions, JsonModemBuffers, JsonModemValues, ParserOptions, ValuesOptions};

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
    let mut options = ParserOptions::default();
    options.allow_multiple_json_values = flags & 1 != 0;
    options.allow_uppercase_u = flags & 2 != 0;
    options.allow_unicode_whitespace = flags & 4 != 0;
    options.panic_on_error = false;
    options
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
            let mut parser = JsonModemValues::with_options(options, ValuesOptions::default());
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

#[test]
fn fuzz_transition_regression_values_case_one() {
    run_case(&FuzzCase {
        name: "values_case_one",
        harness: Harness::Values,
        flags: 5,
        split_seed: 2_401_737_659,
        payload: "\u{202F}\u{3000}\u{3000} {\"g\":{\"\":{},\"M\":\"x\"}}\u{2028}\u{200A}",
        description:
            "multi-root stream that triggered a depth transition panic after finishing a root",
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
        description:
            "values adapter panic when encountering control escapes and whitespace heavy chunking",
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
        description:
            "buffers adapter panic after closing an object and immediately opening an array at root",
    });
}
