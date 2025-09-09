#![no_main]

mod parser_common;

use jsonmodem::JsonModemValues;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: parser_common::FuzzerInput| {
    let mut parser = JsonModemValues::with_options(
        parser_common::parser_options(input.flags),
        parser_common::values_options(input.flags),
    );

    for chunk in &input.chunks {
        parser_common::consume_results(parser.feed(chunk));
    }

    parser_common::consume_results(parser.finish());
});
