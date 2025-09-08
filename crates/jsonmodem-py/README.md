# jsonmodem (Python)

Python bindings for the Rust `jsonmodem` crate, packaged with
`maturin`/`pyo3` as a mixed project. The published Python package
is named `jsonmodem` and contains a native extension module
`jsonmodem._jsonmodem`.

Quickstart (dev):

```
python -m pip install maturin
maturin develop -m crates/jsonmodem-py/Cargo.toml --release
python -c "import jsonmodem, sys; print(jsonmodem.__version__)"
```

## Streaming usage

`JsonModem` exposes the streaming events from the Rust parser. Each event is a
`(kind, path, payload)` triple: `kind` is a short string, `path` is a tuple of
`("key" | "index", value)` components, and `payload` holds scalar data when
present (string fragments provide `{ "fragment", "is_initial", "is_final" }`).

```python
from jsonmodem import JsonModem, ParserOptions

parser = JsonModem(ParserOptions(allow_multiple=True))
for kind, path, payload in parser.feed('{"x": 1} {"y": 2}'):
    print(kind, path, payload)
for kind, path, payload in parser.finish():
    print(kind, path, payload)
```

Build wheels for release:

```
maturin build -m crates/jsonmodem-py/Cargo.toml --release
```
