#![expect(missing_docs)]

mod common;

use jsonmodem::{BufferOptions, JsonModemBuffers, ParserOptions};

use crate::common::STREAM;

fn render_string_buffers(stream: &[&str]) -> String {
  let mut buf = JsonModemBuffers::string(ParserOptions::default(), BufferOptions::default());
  let mut out = String::new();
  for ch in stream {
    for ev in buf.feed(ch).to_iter() {
      let ev = ev.expect("buffers error");
      use core::fmt::Write;
      writeln!(out, "{ev:?}").unwrap();
    }
  }
  out
}

#[test]
fn snapshot_string_prefixes_default() {
  let rendered = render_string_buffers(&STREAM);
  assert!(rendered.contains("ObjectBegin { path: [] }"));
  assert!(rendered.contains("String { path: [Key(\"request\"), Key(\"filename\")]"));
  assert!(rendered.contains("ArrayEnd { path: [Key(\"snippets\")]"));
}
