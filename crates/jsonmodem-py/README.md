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
`(kind, path, payload)` triple. The outer event is a normal Python tuple, so
immediate unpacking is fast. `path` is a lightweight `PathView`, and string
payloads are lightweight `StringPayload` objects with `.fragment`,
`.is_initial`, and `.is_final` attributes.

`feed()` accepts one `str`, `bytes`, `bytearray`, or contiguous `memoryview`
chunk, or an iterable of those chunk types. Passing an iterable is the preferred
way to process many small HTTP or LLM fragments because it uses one Python/Rust
call while preserving event order. Bytes-like chunks are read through Python's
buffer protocol for the duration of the call.

```python
from jsonmodem import JsonModem, ParserOptions

parser = JsonModem(ParserOptions(allow_multiple=True))
for kind, path, payload in parser.feed('{"x": 1} {"y": 2}'):
    print(kind, path, payload)
for kind, path, payload in parser.finish():
    print(kind, path, payload)
```

FastAPI exposes Starlette's request object, and Starlette's
`Request.stream()` yields byte chunks without storing the whole body in memory:

```python
from fastapi import Request
from jsonmodem import JsonModem


async def stream_json(request: Request):
    parser = JsonModem()
    batch = []
    async for chunk in request.stream():
        batch.append(chunk)
        if len(batch) < 64:
            continue
        for kind, path, payload in parser.feed(batch):
            if kind == "string" and path.endswith("content"):
                yield payload.fragment
        batch.clear()

    if batch:
        for kind, path, payload in parser.feed(batch):
            if kind == "string" and path.endswith("content"):
                yield payload.fragment
    for kind, path, payload in parser.finish():
        if kind == "string" and path.endswith("content"):
            yield payload.fragment
```

The Python performance benchmarks are written around streams of fragments. The
fair `jiter` comparison reparses every cumulative prefix with
`partial_mode=True`; reassembled full-document `loads()` timings are kept as
reference results only.

Build wheels for release:

```
maturin build -m crates/jsonmodem-py/Cargo.toml --release
```
