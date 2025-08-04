use jsonmodem::ParserOptions;
use jsonmodem_py::{PyFactory, PyStreamingParser};

#[test]
fn streaming_parser_smoke() {
    let mut parser = PyStreamingParser::new(ParserOptions::default());
    for ev in parser.feed_with(PyFactory, "{\"a\":[1,2]}") {
        ev.unwrap();
    }
    for ev in parser.finish_with(PyFactory) {
        ev.unwrap();
    }
}
