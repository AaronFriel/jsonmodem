# JsonModemBuffers — Rewrite Plan

This document specifies a ground-up rewrite of `JsonModemBuffers` that:

- Removes the broken/overly complex state machine in `src/jsonmodem_buffers.rs`.
- Moves non-scalar value building state (`ValueBuilder<Value>`) into the parent
  `JsonModemBuffers` struct rather than the iterator.
- Inlines the old `EventStack::push` logic into the iterator’s `next()`
  implementation (no intermediate `Vec<ParseEvent>` queue).
- Keeps the public surface area and emitted `BufferedEvent` semantics stable for
  existing tests.

The rewrite follows the proven behavior of the original `EventStack::push`
implementation provided in the prompt and adapts it to the `Value` type and
`BufferedEvent` outputs.

## Goals

- Correctly coalesce string fragments by JSON path, emitting one
  `BufferedEvent::String` per step, with the fragment and an optional buffered
  value according to `BufferStringMode`.
- Maintain a `ValueBuilder<Value>` across feeds inside `JsonModemBuffers` and
  use it to build arrays/objects so container ends can optionally include a
  composite `Value` according to `NonScalarValueMode`.
- Emit non-string events exactly one-per-`next()`; if a pending string exists,
  flush it first and then emit the non-string on a subsequent `next()` call (no
  double emission).
- Persist incomplete string buffers across feeds (iterator `Drop` writes back
  to the parent’s scratch state).
- Keep memory/allocations minimal, preserving the zero-copy and fragment-first
  approach of `JsonModem`.

## Non-Goals

- Changing the external `BufferedEvent` enum or option types.
- Changing `JsonModem` core event semantics.
- Building arbitrary composite scalar values (only objects/arrays via builder).

## Public API (unchanged)

- `JsonModemBuffers::new(options: ParserOptions, opts: BufferOptions) -> Self`
- `JsonModemBuffers::feed<'a>(&'a mut self, chunk: &str) -> JsonModemBuffersIter<'a>`
- `JsonModemBuffers::collect(&mut self, chunk: &str) -> Result<Vec<BufferedEvent>, ParserError>`
- `JsonModemBuffers::finish(self) -> Result<Vec<BufferedEvent>, ParserError>`

`BufferedEvent` stays as-is.

## State Ownership and Invariants

Parent `JsonModemBuffers` holds long-lived state:

- `modem: JsonModem`
- `opts: BufferOptions`
- `scratch: Option<(Path, Str)>` — pending string buffer persisted across feeds
  when a string is not yet final at the end of an iterator.
- `builder: Option<ValueBuilder<Value>>` — exists iff `opts.non_scalar_values != None`.
- `factory: StdValueFactory` — shared factory for `ValueBuilder` operations.

Iterator `JsonModemBuffersIter<'a>` is a thin, short-lived view over `modem.feed`:

- `inner: JsonModemIter<'a>`
- Transient pending-string state to flush/emit one event at a time:
  - `pending_path: Option<Path>`
  - `pending_buf: Str`
  - `pending_last_fragment: Str`
  - `pending_final: bool`
- `stash_non_string: Option<BufferedEvent>` — one non-string event delayed until
  the next `next()` call, used when we must flush a string first.
- `emitted_pending_on_eof: bool` — ensures at most one emission of leftover
  string at iterator end.

Critically, the iterator NO LONGER owns a `ValueBuilder` or factory. It always
updates the parent’s `builder` so container-building state spans multiple feeds.

## Event Mapping and Buffering Rules

### Strings

We coalesce fragments by path. For each incoming `ParseEvent::String { path, fragment, is_final }`:

- If `pending_path` is `None` or equals `path`:
  - Append `fragment` to `pending_buf`.
  - Update `pending_last_fragment` to `fragment` and `pending_final |= is_final`.
- If `pending_path` is Some but for a different `path`:
  - Flush the previous string as `BufferedEvent::String`:
    - `fragment`: the last fragment we saw for the previous path (`pending_last_fragment`).
    - `value`:
      - `None` when `BufferStringMode::None`.
      - `Some(prefix)` when `BufferStringMode::Values` AND the previous string was final.
      - `Some(prefix)` when `BufferStringMode::Prefixes` AND either the previous string is final or we are flushing due to a path switch (prefix emission allowed every flush in Prefixes mode).
    - `is_final`: `pending_final` of the previous string.
  - Then start a new pending buffer for the new `path` with the current `fragment`.

On iterator end (`inner.next()` == `None`):

- If `pending_path` exists and we have not yet emitted it for EOF:
  - Emit one `BufferedEvent::String` with:
    - `fragment = pending_last_fragment`
    - `is_final = pending_final`
    - `value` per `BufferStringMode`:
      - `None` for `None`
      - `Some(full)` for `Values` only if final
      - `Some(full)` for `Prefixes` if final; otherwise `None` on EOF (there is
        no new fragment to justify a prefix emission; matches current tests)
  - Set `emitted_pending_on_eof = true` and return it.
- If no pending string (or already emitted), return `None`.

Iterator `Drop` persists an incomplete string across feeds:

- If `pending_path.is_some()` and `pending_final == false`, write
  `self.parent.scratch = Some((pending_path, pending_buf))`.

### Non-String Events

Map `ParseEvent` to `BufferedEvent`:

- `Null { path }` → `BufferedEvent::Null { path }`
- `Boolean { path, value }` → `BufferedEvent::Boolean { path, value }`
- `Number { path, value }` → `BufferedEvent::Number { path, value }`
- `ArrayStart { path }` → `BufferedEvent::ArrayStart { path }`
- `ObjectBegin { path }` → `BufferedEvent::ObjectBegin { path }`
- `ArrayEnd { path }` → `BufferedEvent::ArrayEnd { path, value: maybe_value }`
- `ObjectEnd { path }` → `BufferedEvent::ObjectEnd { path, value: maybe_value }`

Where `maybe_value` is computed from the parent’s `builder` according to
`NonScalarValueMode` and the ported `EventStack::push` logic (see below).

If a non-string event arrives while `pending_path.is_some()`, we must flush the
string first. We emit one `BufferedEvent::String` now, and stash the non-string
event (already decorated with any composite `value`) into `stash_non_string`. On
the next call to `next()`, we return the stashed non-string event.

## ValueBuilder Integration (ported from EventStack::push)

We adapt the original, generic `EventStack::push` behavior to our concrete
`Value`/`StdValueFactory` types. The parent holds the `ValueBuilder<Value>`.

For each `ParseEvent` the iterator sees, update the parent’s `builder` when it
is `Some(_)`:

- Scalars
  - `Null { path }` → `builder.set(path.last(), build_from_null(new_null()))`
  - `Boolean { path, value }` → `builder.set(path.last(), build_from_bool(value))`
  - `Number { path, value }` → `builder.set(path.last(), build_from_num(value))`
  - `String { path, fragment, .. }` → `builder.mutate_with(path.last(),
    || build_from_str(new_string("")), |v| push fragment into inner string)`
- Container starts
  - `ObjectBegin { path }` → `builder.enter_with(path.last(), build_from_object(new_object()))`
  - `ArrayStart { path }` → `builder.enter_with(path.last(), build_from_array(new_array()))`
- Container ends
  - `ArrayEnd { path }`
    - If `path.is_empty()`: take the builder’s root, convert to array and use it.
    - Else: `builder.pop()` must return the just-closed array value.
  - `ObjectEnd { path }` mirrors `ArrayEnd` but for objects.

Attach composite values per `NonScalarValueMode`:

- `None`: never attach composite values; still keep `builder` in sync.
- `Roots`: attach only when `path.is_empty()`.
- `All`: attach at all container ends (root or nested).

The attachments are cloned `Value`s so the builder’s internal state remains
valid for subsequent events.

## Iterator Algorithm (pseudocode)

```
loop:
  if let Some(e) = stash_non_string.take():
    return e

  match inner.next():
    None =>
      if pending_path.is_some() and !emitted_pending_on_eof:
        emitted_pending_on_eof = true
        return emit_string_from_pending(per BufferStringMode)
      else:
        return None

    Some(Err(e)) => return Err(e)

    Some(Ok(ev)) =>
      // Always keep builder in sync first
      let maybe_composite = apply_builder_on_parent(ev, NonScalarValueMode)

      match ev:
        String { path, fragment, is_final, .. } =>
          if pending_path.is_none() or pending_path == Some(path):
            append_to_pending(path, fragment, is_final)
            continue
          else:
            let flushed = flush_pending_to_string(BufferStringMode)
            // prepare new pending
            init_pending(path, fragment, is_final)
            return Ok(flushed)

        other_non_string =>
          if pending_path.is_some():
            let flushed = flush_pending_to_string(BufferStringMode)
            // map + decorate other with maybe_composite
            stash_non_string = Some(map_parse_to_buffered(other_non_string, maybe_composite))
            return Ok(flushed)
          else:
            return Ok(map_parse_to_buffered(other_non_string, maybe_composite))
```

`apply_builder_on_parent` is a direct translation of the old `EventStack::push`
behavior for `ValueBuilder`, capturing and returning an optional
`(Path, Value)` only for container end events when policy allows.

## Scratch Handling Across Feeds

On `feed()`:

- Create an iterator with empty pending state.
- If the parent has `scratch = Some((path, buf))`, seed iterator’s pending
  string with this state, and clear parent scratch so the iterator exclusively
  owns it until drop.

On iterator `Drop`:

- If a string is pending and not final, write back to parent’s `scratch`.

This preserves the cross-feed coalescing guarantee for long strings split across
chunks.

## Error Propagation

- Parsing errors from `inner.next()` are returned immediately and do not change
  buffering or builder state.

## Complexity and Allocation Notes

- String coalescing appends to a single `Str` buffer per path; we avoid emitting
  intermediate fragments unless forced to by a path switch or non-string event.
- No intermediate `Vec<ParseEvent>` is kept in memory.
- `ValueBuilder` clones only at container ends when policy requires an attached
  value.

## Migration of Current File

- Remove iterator-owned `builder` and `factory`; add them to `JsonModemBuffers`.
- Remove `maybe_apply_builder` and fold its logic into `next()` using the
  parent’s `builder`.
- Keep `stash_non_string`, pending-string state, and iterator `Drop` behavior,
  simplified to reflect the single-source-of-truth parent builder and scratch.
- Simplify `collect` and `finish` to drive `feed()` and reuse the same
  `push`-within-`next` mechanics, relying on iterator behavior rather than
  bespoke duplication.

## Test Expectations

The rewrite must satisfy existing tests in:

- `src/tests/adapters.rs`
- `src/tests/buffers_regressions.rs`

Key behaviors validated:

- Coalesced string buffering per path; `fragment` is the most recent fragment
  for that emission; `value` follows `BufferStringMode`.
- Incomplete strings are carried across feeds and completed later.
- Container end events carry a `value` only per `NonScalarValueMode`.
- No double-emission in a single `next()`; if we flush a string, the following
  non-string is stashed for the next call.

## Rationale

This design removes accidental complexity, aligns strictly with the known-good
`EventStack::push` semantics for composite values, and keeps the buffering
adapter minimal, predictable, and testable. The parent-owned builder ensures
containers can be built across feeds, while the iterator remains a simple glue
layer that translates parser events into buffered outputs.

