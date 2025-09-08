# Expose JsonModem Streaming Events to Python

This ExecPlan is a living document and must be maintained under the rules described in `PLANS.md`. The goal is to recreate the Python bindings currently visible in `git diff main..HEAD`, ensuring that future contributors can re-derive the same result from scratch.

## Purpose / Big Picture

Python users need direct access to the low-level streaming events produced by the Rust `jsonmodem` parser so they can build their own buffering and value assemblers in pure Python. After completing this plan, a developer can run `python -c "from jsonmodem import JsonModem"` and iterate `(kind, path, payload)` tuples coming straight from the Rust parser while the heavy parsing work executes outside the Python GIL. The Python package must expose only this event-level surface (no buffering helpers) and mirror the Rust semantics faithfully. Success is demonstrated by passing the pytest suite under `crates/jsonmodem-py/tests`, keeping `.agent/check.sh` green, and observing that the README examples produce the expected events.

## Progress

- [x] (2025-02-16 01:05Z) Reverse engineered HEAD vs. `main` to capture the completed Python binding surface.
- [x] (2025-02-16 01:10Z) Documented the contract for the Python binding, including the raw event tuple format and module layout.
- [x] (2025-02-16 01:20Z) Implemented the PyO3 module in `crates/jsonmodem-py/src/lib.rs`, covering `JsonModem`, `ParserOptions`, `DecodeMode`, iterators, and owned event conversion.
- [x] (2025-02-16 01:25Z) Added pytest coverage for events, chunked strings, multiple values, finish semantics, and syntax errors.
- [x] (2025-02-16 01:30Z) Updated documentation and tooling (`README.md`, `crates/jsonmodem-py/README.md`, `.agent/check.sh`, `.gitignore`) to reflect the new binding.
- [x] (2025-02-16 01:35Z) Validated with `scripts/setup-py.sh`, `.agent/check.sh`, and `scripts/check-py.sh`.
- [x] (2025-02-16 01:40Z) Captured decisions, surprises, and retrospective notes for future contributors.
- [x] (2025-02-16 02:20Z) Automate API documentation generation (pydoc + pdoc) and enrich Rust/PyO3 docstrings so Python docs read cleanly.

## Surprises & Discoveries

- Tracking string fragments required a `HashSet<Vec<OwnedPathComponent>>` keyed by the current JSON path so we can flip `is_initial` to false once a fragment follows the first chunk and drop the entry when `is_final` arrives. This avoids leaking state between simultaneous string fields and mirrors the semantics of `jsonmodem`’s buffered adapters.
- PyO3 tuple creation is cheaper when we intern the constant tag strings (`"key"`, `"index"`, and each event kind) once per interpreter rather than allocating new Python strings on every event conversion.
- `StreamingParser` still powers `JsonModem` internally; the binding exposes a friendlier name while preserving the zero-copy parser core. Releasing the GIL during `feed_chunks` while collecting owned events keeps Python iteration responsive.
- Generating docs with both `pydoc` and `pdoc` takes under a second; `pdoc` produces a richer layout (module index + type navigation) while `pydoc` remains a useful baseline. Both outputs land in `tmp/plans/python` so they stay out of version control.

## Decision Log

- (2025-02-16) Collect owned events in a `Vec<OwnedEvent>` while the GIL is released, then yield them lazily through a `PyEventIter`. Rationale: parsing and UTF-8 validation can run without the GIL, but converting directly to Python objects under `py.allow_threads` is unsafe; buffering in Rust ensures we only touch Python APIs while holding the GIL.
- (2025-02-16) Represent paths as tuples of `("key", str)` or `("index", int)` instead of custom Python classes. Rationale: tuples are ABI-stable, cheap to create, and easy for downstream code to pattern-match.
- (2025-02-16) Expose only the streaming API (`JsonModem`, `ParserOptions`, `DecodeMode`, and the custom exceptions). Rationale: higher-level buffering belongs in Python, keeping the Rust FFI surface minimal and stable.
- (2025-02-16) Note open questions about number precision (currently `f64`) and future higher-level adapters in the spec rather than expanding the binding prematurely.

## Outcomes & Retrospective

The binding matches the spec, passes the new pytest suite, and integrates with existing tooling. README examples give working guidance, and the spec captures remaining open questions around numeric precision and optional adapters. Future work can build Python-side utilities on top of the stable raw event tuples without modifying the Rust module.

## Context and Orientation

The repository hosts the Rust parser in `crates/jsonmodem/` and the Python extension in `crates/jsonmodem-py/`. The Python package is built with PyO3 and maturin; it ships an ABI3-compatible extension `jsonmodem._jsonmodem` and a thin Python package wrapper under `crates/jsonmodem-py/python/jsonmodem`. Tooling scripts live at the repository root (`.agent/check.sh`, `scripts/setup-py.sh`, `scripts/check-py.sh`). This ExecPlan now holds the full contract for the binding—there is no separate spec file—so every requirement must remain up to date here.

**Binding Contract (authoritative summary)**

*Public API*

    from jsonmodem import JsonModem, ParserOptions, DecodeMode, JsonModemSyntaxError, JsonModemStateError

`JsonModem` accepts `ParserOptions` (defaults below), yields iterators of `(kind, path, payload)` tuples from `feed(chunk: str)` and `finish()`, and exposes `is_finished`. `ParserOptions` exposes:

* `allow_unicode_whitespace: bool` – recognise JSON5-style whitespace between values (default `False`).
* `allow_multiple: bool` – permit multiple root values in one stream (default `False`).
* `decode_mode: DecodeMode` – one of `StrictUnicode`, `SurrogatePreserving`, `ReplaceInvalid` (default `StrictUnicode`).
* `allow_uppercase_u: bool` – accept `\UXXXX` escapes (default `False`).
* `as_dict()` helper returning a Python dict snapshot.

`DecodeMode` is an enum-like class providing `.name` and `.value` and the three singleton attributes above.

*Events*

* Each event is a tuple `(kind, path, payload)`.
* `kind` is one of `"null"`, `"bool"`, `"number"`, `"string"`, `"array_begin"`, `"array_end"`, `"object_begin"`, `"object_end"`.
* `path` is a tuple of components, where each component is `("key", str)` or `("index", int)` describing the JSON path to the event.
* `payload` is:
  * `None` for nulls and structural begin/end markers;
  * `bool` for booleans;
  * `float` for numbers (mirrors Rust’s `f64`);
  * a dict `{"fragment": str, "is_initial": bool, "is_final": bool}` for strings (fragments arrive as chunks).

Strings are reported incrementally: `is_initial` is `True` on the first fragment for a given path, `is_final` on the last. Paths let callers rebuild structure or correlate fragments.

*Errors & State*

* `JsonModemSyntaxError` is raised lazily from iterators when invalid JSON is detected; it carries `.line` and `.column` populated from the Rust parser.
* `JsonModemStateError` is raised when `feed()` is called after `finish()` or `finish()` is called twice.
* With `allow_multiple=False`, trailing data after a parsed value triggers `JsonModemSyntaxError` at the appropriate boundary.

*Threading*

* Rust parsing occurs inside `py.allow_threads`, collecting owned `Vec<OwnedEvent>` values; conversion to Python objects happens inside the iterator to ensure GIL safety.
* We do not opt into Python’s free-threaded mode; the extension relies on the GIL for object creation.

*Non-goals*

* No Rust exposure of JsonModemBuffers/JsonModemValues—higher-level helpers should be pure Python.
* No additional parser toggles beyond the four listed above.

*Examples*

    >>> from jsonmodem import JsonModem, ParserOptions, DecodeMode
    >>> modem = JsonModem(ParserOptions(allow_multiple=True))
    >>> list(modem.feed('{"a": 1} {"b": 2}'))
    [('object_begin', (), None), ('string', (('key', 'a'),), {'fragment': 'a', 'is_initial': True, 'is_final': True}), ('number', (('key', 'a'),), 1.0)]
    >>> list(modem.finish())
    [('object_end', (), None)]

## Implementation Strategy

1. Reiterate the contract inside this ExecPlan. Summarise the streaming API, raw tuple layout, and error semantics (using the binding summary above). Embed examples for single-value and multi-value parsing, explain path tagging, and document limitations (no buffers/values, `f64` numbers, UTF-8 decode modes).

2. Scaffold the PyO3 module in `crates/jsonmodem-py/src/lib.rs`. Define:
   - `DecodeMode` enum mirroring Rust’s decode modes with conversion helpers.
   - `ParserOptions` dataclass that maps to `jsonmodem::ParserOptions`.
   - Exception types `JsonModemSyntaxError` and `JsonModemStateError`.
   - `PyJsonModem` class storing `StreamingParser` plus an `Option<HashSet<Path>>` for fragment tracking and a bool `is_finished`. `feed` and `finish` must return `PyEventIter` objects and guard against invalid state transitions.
   Release the GIL when calling into `StreamingParser`, but keep Python object construction inside `PyEventIter::__next__`.

3. Implement owned event conversion utilities:
   - `OwnedPathComponent`, `OwnedPayload`, `OwnedEventKind`, and `OwnedEvent`.
   - `OwnedEvent::from_parse_event` converts each `ParseEvent`, tracks string start/end, and clones path components into owned representations.
   - `build_path_tuple` and `build_payload` transform owned data into Python tuples/dicts using interned tag strings stored in an `InternedStrings` helper struct.
   - `OwnedParserError` stores message, line, and column for syntax error propagation.

4. Build the iterator wrapper. `PyEventIter` should hold a `Vec<EventRecord>` and an index, implement `__iter__` returning self, and `__next__` that yields `(kind, path, payload)` tuples or raises `JsonModemSyntaxError` / `StopIteration`. Convert Rust errors into Python exceptions with detailed context.

5. Extend tests under `crates/jsonmodem-py/tests`:
   - `test_events_simple.py` exercises each event kind and ensures path tagging works.
   - `test_multiple_values.py` covers `allow_multiple=True` vs. `False`.
   - `test_strings_fragmentation.py` verifies fragment ordering and flags across chunked feeds.
   - `test_finish_semantics.py` confirms state errors when misusing `feed`/`finish`.
   - `test_errors.py` checks syntax errors include line/column.
   Use the repository’s managed virtualenv by running `scripts/setup-py.sh` first.

6. Update documentation and tooling:
   - Add Python artefacts to `.gitignore`.
   - Exclude `jsonmodem-fuzz` and `jsonmodem-py` from `.agent/check.sh` so workspace checks remain fast.
   - Refresh `README.md` with the new API naming and timings table; update `crates/jsonmodem-py/README.md` with quickstart instructions and examples.

7. Validate iteratively:
   - Run `scripts/setup-py.sh` once per environment to configure the virtualenv.
   - Execute `./.agent/check.sh` after Rust changes.
   - Execute `scripts/check-py.sh` after Python/Rust binding updates.

8. Record discoveries and open questions directly in this ExecPlan, especially around potential future adapters or numeric precision enhancements, so future contributors know the boundaries of the current design.

9. Build documentation automation:
   - Extend `.agent/check-py.sh` to generate both built-in `pydoc` HTML and richer `pdoc` output into a git-ignored staging directory (`tmp/plans/python`).
   - Add doc comments (`///`) to the Rust `#[pyclass]` and `#[pymethods]` items so PyO3 exposes helpful Python docstrings.
   - Record viewing instructions in `docs/py/api-docs.md` so contributors know where the generated HTML lives.
   - Capture which doc style renders best and note any follow-up tasks in the `Surprises & Discoveries` section.
