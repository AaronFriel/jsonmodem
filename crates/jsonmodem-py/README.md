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
call while preserving event order. Immutable bytes-backed input can be borrowed.
Mutable buffers and exporters whose storage cannot be verified as immutable are
copied before parsing, so Python callbacks cannot change text being parsed.

Use constructor options to shape the event stream without switching classes:

```python
JsonModem()                               # all events, decoded strings
JsonModem(paths="content")                # only matching paths
JsonModem(byte_views=True)                # byte-backed string payloads
JsonModem(paths="content", byte_views=True)
```

`paths` accepts a path string or a sequence of path strings. `*` matches one
object key or array index, for example `"items.*.metadata.etag"`. With
`byte_views=True`, `feed()` accepts immutable `bytes` or read-only contiguous
`memoryview` chunks, or an iterable of those chunks. Unescaped string fragments
that point into the current input are returned as `memoryview` objects; escaped
fragments fall back to `str`.

Direct memoryview inputs in this mode must be backed by `bytes`. Other accepted
read-only exporters are snapshotted into immutable bytes; their payload views
retain the snapshot rather than the original exporter. Parsing and payloads use
the same acquired storage even if an exporter returns different buffers on
successive requests.

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
    parser = JsonModem(paths="content", byte_views=True)
    batch = []
    async for chunk in request.stream():
        batch.append(chunk)
        if len(batch) < 64:
            continue
        for kind, path, payload in parser.feed(batch):
            if kind == "string":
                fragment = payload["fragment"]
                yield bytes(fragment) if payload["is_view"] else fragment.encode()
        batch.clear()

    if batch:
        for kind, path, payload in parser.feed(batch):
            if kind == "string":
                fragment = payload["fragment"]
                yield bytes(fragment) if payload["is_view"] else fragment.encode()
    for kind, path, payload in parser.finish():
        if kind == "string":
            fragment = payload["fragment"]
            yield bytes(fragment) if payload["is_view"] else fragment.encode()
```

The Python performance benchmarks are written around streams of fragments. The
fair `jiter` comparison reparses every cumulative prefix with
`partial_mode=True`; reassembled full-document decoder timings are kept as
competitor reference results only. `jsonmodem` does not expose a full-document
`loads()` API.

## Incremental values

`JsonModemValues` is for callers that want a read-only view of the current JSON
value instead of parser events. It yields `(index, view, path, is_final)`
tuples. `view` is the same reused root `JsonModemValueView` object on every
update, and `path` is a `PathView` pointing at the changed field.

```python
from jsonmodem import JsonModemValues


parser = JsonModemValues()
for index, view, path, is_final in parser.feed([b'{"message":"hel', b'lo"']):
    if path.endswith("message"):
        print(index, view["message"].snapshot(), is_final)
for index, view, path, is_final in parser.feed(b"}"):
    print(index, view.snapshot(), is_final)
```

`JsonModemValueView` exposes read-only operations such as `kind`, `snapshot()`,
`__getitem__()`, and `__len__()`. Python's type system does not have a general
`ReadOnly[T]` for arbitrary objects. Use read-only interfaces such as
`Mapping`/`Sequence` or protocol classes when annotating consumer code;
`typing.ReadOnly` is specific to `TypedDict` fields, and `Final` prevents
rebinding rather than mutation.

The root view is live. If `allow_multiple=True` and one `feed()` call contains
several complete root values, stored updates will all see the latest root by the
time the returned iterator is consumed. Feed and consume one root at a time when
historical root snapshots matter.

When a synchronous source already has many tiny fragments available, pass the
fragment iterable to `feed()` instead of calling `feed()` once per fragment.
This keeps the same API shape but avoids repeated Python-to-Rust call overhead:

```python
for index, view, path, is_final in parser.feed(chunks):
    ...
```

Build wheels for release:

```
maturin build -m crates/jsonmodem-py/Cargo.toml --release
```
