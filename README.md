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

**Streaming‑JSON benchmark (time *per chunk*)**

* 10 kB JSON streamed in 100 / 1 000 / 5 000 pieces.
* **Implementations**

  * `jsonmodem::StreamingParser` – single‑pass state machine
  * `parse_partial_json` – Rust port of [vercel/ai](https://github.com/vercel/ai)'s JSON fixing with
    `serde_json`
  * [`partial_json_fixer`](https://crates.io/crates/partial-json-fixer) - Rust crate

**Result:** `jsonmodem::StreamingParser` is **8 – 77 × faster per chunk** because it never rebuilds
  or re‑parses the buffer.

| chunks | jsonmodem | parse_partial_json | partial_json_fixer | speed-up\* |
| -----: | --------------: | -------------------: | -------------------: | --------------------------: |
|    100 |     **1.22 µs** |             10.46 µs |              9.93 µs |                   **× 8.2** |
|  1 000 |     **0.22 µs** |             10.54 µs |              9.88 µs |                  **× 44.6** |
|  5 000 |     **0.13 µs** |             10.51 µs |              9.80 µs |                  **× 77.0** |


\* Versus the fastest helper (`partial_json_fixer`). Benchmarked with Criterion.

---

## 🔭 Roadmap

| Target              | Status     | Notes                          |
| ------------------- | ---------- | ------------------------------ |
| Rust crate          | ✅ released |                                |
| **Python** bindings | 🛠 next    | `pyo3`, published to PyPI      |
| **Node‑API** module | ⏩ queued   | Native addon for TS/JS         |
| **WASM** build      | ⏩ queued   | < 30 kB gz for browsers & Deno |

---

## 🤝 Contributing

Issues and PRs—especially fuzz corpora and non‑Rust bindings—are very welcome. A `CONTRIBUTING.md`
will land before the first non‑Rust release.

---

## 📝 License

MIT or Apache 2 © 2025 Aaron Friel
