#![no_main]

mod parser_common;

use jsonmodem::{JsonModemBuffers, StdBackend};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Some(input) = parser_common::prepare_input(data) {
        let mut parser = JsonModemBuffers::new(
            parser_common::parser_options(input.flags),
            parser_common::buffer_options(input.flags),
        );

        for chunk in &input.chunks {
            parser_common::consume_results(parser.feed(chunk).to_iter());
        }

        parser_common::consume_results(parser.finish().to_iter());
    }
});
