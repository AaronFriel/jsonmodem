# jsonmodem
*Incremental, event‑driven **streaming JSON** parser for Rust* 🚀


Parse → filter → act **while the bytes are still in flight**.

[![Crates.io](https://img.shields.io/crates/v/jsonmodem)](https://crates.io/crates/jsonmodem)
[![Docs.rs](https://img.shields.io/docsrs/jsonmodem)](https://docs.rs/jsonmodem)
![Tests](https://github.com/aaronfriel/jsonmodem/actions/workflows/test.yml/badge.svg?branch=main)
![Fuzzing](https://github.com/aaronfriel/jsonmodem/actions/workflows/fuzz.yml/badge.svg?branch=main)
![Miri](https://github.com/aaronfriel/jsonmodem/actions/workflows/miri.yml/badge.svg?branch=main) [![MSRV
1.85](https://img.shields.io/badge/MSRV-1.85-blue)](#msrv)

---

## ✨ Why jsonmodem?

* **Linear performance, bounded memory** – work grows with bytes received; peak usage is limited to
  the largest in‑flight fragment when default options are used.
* **LLM‑ready** – handles multi‑kilobyte tool calls without the quadratic “buffer, patch, re‑parse”
  dance.
* **First‑class moderation hooks** – inspect or cancel as soon as a sentinel field appears.
* **Hardened core** – QuickCheck property tests, `cargo‑fuzz` (via `libafl_libfuzzer`), and Miri
  runs to verify safety.

---

## 📦 Installation

```bash
cargo add jsonmodem
````

*(Python, Node‑API, and WASM bindings are on the roadmap.)*

---

## 🧪 Quick start – reacting to moderation while streaming code


The full runnable program lives at
[`examples/llm_tool_call.rs`](crates/jsonmodem/examples/llm_tool_call.rs).

```rust
use jsonmodem::{
    StreamingParser, ParserOptions, StringValueMode, ParseEvent
};

let mut parser = StreamingParser::new(ParserOptions {
    // Emit string *prefixes* so we can act on partial values
    string_value_mode: StringValueMode::Prefixes,
    ..Default::default()
});

for chunk in llm_stream() {           // ← bytes from the model
    parser.feed(&chunk);

    for ev in &mut parser {
        match ev? {
            // 1️⃣ Abort early if the model flags a policy violation
            ParseEvent::String { path, value: Some(prefix), .. }
                if path == path!["moderation", "decision"]
                   && prefix.starts_with("block") =>
            {
                return Err("content blocked".into());
            }

            // 2️⃣ Forward code fragments to the UI immediately
            ParseEvent::String { path, fragment, .. }
                if path == path!["code"] =>
            {
                ui_write(fragment);   // render incrementally
            }

            _ => {}
        }
    }
}
```

*Result*: harmful output is rejected **before** the document finishes, while valid code streams to
the user with minimal latency.

---

## 📊 Performance
---
**Streaming‑JSON benchmark (time *per chunk*)**

* 16 KiB JSON streamed in 100 / 1 000 / 5 000 pieces (the `response_large.json` file).
* **Implementations**

  * `jsonmodem::StreamingParser` – single-pass state machine. `NonScalarValueMode` controls memory usage and is rendered in the table below:
    * `None` (the `jsonmodem` column) – no objects or arrays are emitted. This is the default and recommended for streaming servers.
    * `Roots` – root objects and arrays are buffered and emitted as parse events.
    * `All` – every object and array is emitted as a parse event.
  * `Values` - uses `jsonmodem::StreamingValuesParser`, yields parsed values each chunk parsed.
  * `parse_partial_json` – Rust port of [vercel/ai](https://github.com/vercel/ai)'s JSON fixing with `serde_json`.
  * `fix_json_parse` – helper from Vercel AI's library.
  * `jiter` – partial JSON parser (`jiter_partial` and `jiter_partial_owned`). The *owned* variant is closer to real Python usage because borrowed strings must be materialized as [`str`](https://peps.python.org/pep-0393/).

These implementations produce different outputs: `jsonmodem::StreamingValuesParser` (below as "Values"), `parse_partial_json`, `fix_json_parse`, and `jiter` emit a value for each chunk fed to the parser, while `jsonmodem::StreamingParser` has modes that produce parse events.


The first four columns use `jsonmodem`, respectively:

* Default: ``jsonmodem::StreamingParser` with `NonScalarValueMode` of `None`. Parse events are emitted for every null, boolean, numeric, and string chunk parsed.
* Roots: The above, and the fully parsed JSON value is also built in memory and emitted as a parse event with an empty path on completely parsing a value.
* All: Will also emit all array and object values at every depth.
* Values: Uses `jsonmodem::StreamingValuesParser`, which instead of producing parse events on iteration, produces whole values. 

| chunks | Default | Roots | All | Values | `parse_partial_json` | `fix_json_parse` | `jiter_partial` | `jiter_partial_owned` |
| -----: | ----------: | -----------: | ---------: | -------------: | -----------------: | -------------: | ------------: | -------------------: |
|    100 |     115 μs |      141 μs |     144 μs |        673 μs |            5.35 ms |         3.92 ms |      1.05 ms |              1.75 ms |
|  1 000 |     211 μs |      257 μs |     271 μs |        5.16 ms |            50.4 ms |         36.9 ms |      9.93 ms |              15.9 ms |
|  5 000 |     589 μs |      667 μs |     730 μs |        22.9 ms |             222 ms |          164 ms |       42.3 ms |              67.1 ms |

Benchmarked with Criterion. Lower is faster.

## 🔭 Roadmap

| Target              | Status      | Notes                       |
| ------------------- | ----------- | --------------------------- |
| Rust crate          | ✅ released |                             |
| **Python** bindings | 🛠 next      | `pyo3`, published to PyPI  |
| **Node‑API** module | ⏩ queued   | Native addon for TS/JS      |
| **WASM** build      | ⏩ queued   | For browsers and more       |

---

## 🤝 Contributing

Issues and PRs—especially fuzz corpora and non‑Rust bindings—are very welcome. A `CONTRIBUTING.md`
will land before the first non‑Rust release.

---

## 📝 License

MIT or Apache 2 © 2025 Aaron Friel
