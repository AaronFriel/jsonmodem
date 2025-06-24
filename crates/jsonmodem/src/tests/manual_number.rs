use crate::{StreamingParser, ParserOptions};

#[test]
fn manual_number_events() {
    let mut p = StreamingParser::new(ParserOptions::default());
    let events = p.parse_incremental("123").expect("parse failed");
    eprintln!("events = {:?}", events);
    assert!(!events.is_empty());
}

