#![no_main]

mod parser_common;

use jsonmodem::{JsonModem, StdBackend};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: parser_common::FuzzerInput| {
    let mut parser: JsonModem<StdBackend> =
        JsonModem::new(parser_common::parser_options(input.flags));

    for chunk in &input.chunks {
        parser_common::consume_results(parser.feed(chunk).to_iter());
    }

    parser_common::consume_results(parser.finish().to_iter());
});
