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
chunk. Fragment mode also accepts an iterable for compatibility. New code that
already has several HTTP, WebSocket, or LLM fragments available should prefer
`feed_many(chunks)` so the caller-selected application update is explicit.
Bytes-like chunks are read through Python's buffer protocol for the duration of
the call. Byte chunks may end in the middle of a UTF-8 character; jsonmodem
carries the incomplete trailing bytes into the next byte or memoryview input and
raises `TypeError` from `finish()` if the stream ends before that character is
completed. Invalid bytes inside a chunk still raise immediately.

Use constructor options to shape the event stream without switching classes:

```python
JsonModem()                               # all events, decoded strings
JsonModem(paths="content")                # only matching paths
JsonModem(paths="content", string_events="per_feed")
JsonModem(byte_views=True)                # byte-backed string payloads
JsonModem(paths="content", byte_views=True)
```

`paths` accepts a path string or a sequence of path strings. `*` matches one
object key or array index, for example `"items.*.metadata.etag"`. With
`byte_views=True`, `feed()` accepts immutable `bytes` or read-only contiguous
`memoryview` chunks, or an iterable of those chunks. Unescaped string fragments
that point into the current input are returned as `memoryview` objects; escaped
fragments fall back to `str`. Byte-view payloads include:

- `payload_kind == "raw_source"` and `ownership == "borrowed"` for a borrowed
  source span.
- `payload_kind == "decoded_text"` and `ownership == "owned"` for an allocated
  decoded fallback.

Borrowed spans keep their source buffer alive for as long as the returned
`memoryview` is retained. A tiny selected field inside a large `bytes` object can
therefore retain the entire source object. A read-only `memoryview` input is
accepted only when it is backed by immutable `bytes`. If a string fragment is
decoded from bytes carried across a UTF-8 chunk boundary, byte-view mode returns
owned decoded text for that fragment because no single caller-owned buffer
contains the complete valid span.

`string_events="per_feed"` compacts selected string fragments within one
`feed()` or `feed_many()` call before building Python event tuples, path views,
or `StringPayload` objects. Concatenating the compacted deltas produces the same
decoded text as concatenating fragment-mode events. This mode is intended for
selected streaming fields:

```python
from jsonmodem import JsonModem

parser = JsonModem(paths=["message", "items.*.content"], string_events="per_feed")

for kind, path, payload in parser.feed_many([b'{"message":"hel', b'lo"}']):
    if kind == "string":
        print(path, payload.fragment, payload.is_final)
```

With `byte_views=True`, per-feed compaction returns owned decoded text for the
compacted payload. JsonModem does not claim a borrowed or zero-copy payload for
escaped strings or strings that span several input buffers.

Cumulative-prefix sources should be adapted before calling `JsonModem`: keep the
previous accepted length, verify that the new prefix is append-only when the
source cannot prove it, and pass only the newly appended bytes to `feed()` or
`feed_many()`. Silent parser-state reuse after earlier input changes is
incorrect.

The Python package intentionally does not expose a cumulative-prefix parser
class. A full-prefix source has an application-level append-only contract, so
the caller should decide whether to trust that contract or compare the previous
prefix before calling `feed()` with only the appended bytes:

```python
from jsonmodem import JsonModem

parser = JsonModem(paths="message", string_events="per_feed")
accepted = b""

def update_prefix(prefix: bytes):
    global accepted
    if not prefix.startswith(accepted):
        raise ValueError("JSON prefix was rewritten; reset the parser")
    appended = prefix[len(accepted):]
    accepted = prefix
    return parser.feed(appended)
```

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
        for kind, path, payload in parser.feed_many(batch):
            if kind == "string":
                fragment = payload["fragment"]
                yield bytes(fragment) if payload["is_view"] else fragment.encode()
        batch.clear()

    if batch:
        for kind, path, payload in parser.feed_many(batch):
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
value instead of parser events. Use `update(chunks)` for the low-notification
path: it consumes one chunk or an iterable of chunks, mutates the parser-owned
value tree, and returns the same reused `JsonModemValueView` object without
creating one Python tuple per mutation. Call `snapshot()` explicitly when you
want ordinary Python containers and strings.

```python
from jsonmodem import JsonModemValues


parser = JsonModemValues()
view = parser.update([b'{"message":"hel', b'lo"'])
assert view is parser.view()
print(view["message"].snapshot())
parser.finish(changed_paths=False)
```

Passing `changed_paths=True` returns a summary dict with `view` and
`changed_paths`. Duplicate object keys are reported in parser event order, and
the mapping-like view uses last-write-wins semantics for duplicate keys.
`reset()` preserves the view object but returns it to the empty state; retained
views therefore observe later updates by design.

For callers that need a notification for each mutation, `feed()` and no-argument
`finish()` yield `(index, view, path, is_final)` tuples. `view` is still the same
reused root `JsonModemValueView` object on every update, and `path` is a
`PathView` pointing at the changed field.

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

## Completed subtrees

`JsonModemCompletedSubtrees` emits owned Python values when selected objects or
arrays close. This is for record-style streams where callers want each item from
a large array before the root document is complete:

```python
from jsonmodem import JsonModemCompletedSubtrees


parser = JsonModemCompletedSubtrees(paths="items.*")
for path, value, released in parser.feed_many(chunks):
    process(path.as_tuple(), value)
for path, value, released in parser.finish():
    process(path.as_tuple(), value)
```

Values are emitted in document order. Incomplete final subtrees are not emitted
as complete. The emitted value is owned by Python and remains valid after the
parser releases its internal copy.

`release_after_emit=True` removes emitted object fields and replaces emitted
array entries with `null` placeholders in the current implementation.
If overlapping selected paths are configured, a completed child is not released
before a selected ancestor can emit its original value; the emitted child's
`released` flag is `False` in that case.
`retained_state()` reports the remaining native value-tree accounting, including
`released_array_entries_retained_as_null`. This means completed subtree
extraction can reduce retained payload bytes, but jsonmodem does not yet claim
constant parser-retained memory for large arrays.

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
