use crate::{StreamingParser, ParserOptions};
#[test]
fn manual_number(){
 let mut p=StreamingParser::new(ParserOptions::default());
 let ev=p.parse_incremental("123").unwrap();
 println!("events len {}", ev.len());
 assert!(!ev.is_empty());
}
