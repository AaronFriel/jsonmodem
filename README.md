# jsonmodem

Incremental and online JSON parser for Rust (**no_std** compatible).

`jsonmodem` lets you consume a JSON document **while it is still being
produced**, slice-by-slice, without first buffering the entire payload in
memory.  The crate is a port of the original
TypeScript reference implementation published as the **fn-stream** package.

---

## Why incremental parsing?

* Stream structured responses from large language models (LLMs) with almost
  zero latency.
* Operate on arbitrarily large JSON documents in memory-constrained
  environments (embedded, WASM, …).
* Utilize partial values as soon as the information you need becomes available.

---

## Quick tour of the repository

```text
workspace/
│
├─ crates/jsonmodem/   # This crate
├─ fuzz/               # libFuzzer/AFL harness & corpus
└─ fn-stream/          # Original TypeScript implementation (kept for reference)
```

---

## Getting started

Add the dependency:

```toml
[dependencies]
jsonmodem = "0.1"
```

Parse a streaming payload:

```rust
use jsonmodem::{StreamingParser, ParserOptions, ParseEvent};

let mut parser = StreamingParser::new(ParserOptions::default());

// Feed input in small, possibly irregular chunks.
parser.feed("{\"hello\": [true, null, 3.14]");
parser.feed(" }");

// Drain the remaining events once the stream is finished.
for evt in parser.finish() {
    let evt = evt?; // Result<ParseEvent, ParserError>
    println!("{evt:?}");
}
```

`ParseEvent` gives you exactly what changed – start/end of object or array,
scalar value emitted, etc.  Higher-level helpers such as `ValueBuilder` are
provided if you eventually want to materialise the full JSON value.

---

## Building & testing

```bash
# run unit tests & property-based tests
cargo test -p jsonmodem

# (optional) fuzz – requires nightly & cargo-fuzz
cargo fuzz run foo
```

---

## Contributing

Bug reports, ideas and pull requests are very welcome!  Please open an issue
first for larger changes so we can discuss the design up-front.

1. Follow the style of the surrounding code.
2. Keep dependencies to a minimum.
3. Run `cargo fmt` and `cargo test` before submitting.

---

## License

Dual licensed under either of:

* Apache-2.0 – see `LICENSE-APACHE`
* MIT – see `LICENSE-MIT`

You may choose the license that best suits your needs.
