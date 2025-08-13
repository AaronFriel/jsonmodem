use crate::{ParserOptions, StdBackend};
use crate::parser::JsonModem;

type DefaultJsonModem = JsonModem<StdBackend>;

#[test]
fn manual_number_events() {
    let mut parser = DefaultJsonModem::new(ParserOptions::default());
    let events: Vec<_> = parser
        .feed("123")
        .to_iter()
        .collect::<Result<_, _>>()
        .expect("parse failed");
    eprintln!("events = {:?}", events);
    assert!(!events.is_empty());
}
