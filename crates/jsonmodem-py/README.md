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

Build wheels for release:

```
maturin build -m crates/jsonmodem-py/Cargo.toml --release
```
