# JsonModemBuffers Rewrite Spec

Status: draft

Owner: runtime parsing layer

Scope: `crates/jsonmodem/src/jsonmodem_buffers.rs` and its iterator

## Motivation

`JsonModemBuffers` has accreted complexity over time: the current iterator owns
multiple “pending_*” fields and even an internal builder. This made behavior
hard to reason about (e.g., incorrect `ObjectEnd` values for
`NonScalarValueMode::All`, string value semantics drifting, EOF flushes that
don’t correspond to core events).

We will rewrite (not refactor) `JsonModemBuffers` and its iterator to:

- Restore a simple, predictable 1:1 mapping: one buffered event per core event.
- Put state where it belongs (adapter, not iterator).
- Make string buffering and non-scalar value emission correct and easy to test.
- Keep the public API the same.

## Non‑Goals

- Changing the public surface (types, constructors, module names).
- Changing the `JsonModem` core event shapes.
- FFI changes or Python-specific optimizations.
- Path‑policy or size‑limit features.

## Terminology

- Core: `JsonModem` and `ParseEvent`.
- Adapter: `JsonModemBuffers` mapping core events to `BufferedEvent`.
- String coalescing: accumulating fragments of the same string in a scratch
  buffer keyed by path.
- Builder: `ValueBuilder<Value>` used only to provide composite values for
  `ArrayEnd`/`ObjectEnd` when requested.

## User‑Visible Semantics (unchanged API, clarified behavior)

`JsonModemBuffers` maps each core event to exactly one `BufferedEvent`:

- Strings
  - `fragment`: always the core fragment for that event.
  - `value` depends on `BufferStringMode`:
    - `None`: always `None`.
    - `Values`: `Some(coalesced_string_so_far)` on every string event.
    - `Prefixes`: `None` for non‑final fragments; `Some(coalesced_full_string)` on the final fragment only.
  - `is_final`: copied from the core event.
  - The adapter maintains a single `scratch: Option<(Path, String)>` for the
    currently coalescing string. `scratch` is cleared on `is_final` or when
    a closing container event proves the scratch path is no longer in scope.

- Containers (`ArrayEnd`/`ObjectEnd`)
  - `NonScalarValueMode::None`: `value: None`.
  - `NonScalarValueMode::Roots`: `value: Some(root_value)` only when `path` is
    empty.
  - `NonScalarValueMode::All`: `value: Some(value)` for nested and root (uses
    the adapter’s builder to capture a clone of the current leaf/root value).

- All other events (`Null`, `Boolean`, `Number`, `ArrayStart`, `ObjectBegin`)
  - Mapped 1:1 with their data carried through unchanged.

## Chunk Boundaries

- The iterator must not emit any events that are not driven by core events.
- If `inner.next()` returns `None`, the iterator returns `None` — no flushing.
- Coalesced string state is maintained at the adapter level across feeds, but
  no “extra” flush events occur at boundaries.

## Design

### Where State Lives

- `JsonModemBuffers` (adapter):
  - `scratch: Option<(Path, String)>` — coalesced string buffer for the current
    string under construction.
  - `builder: Option<ValueBuilder<Value>>` — present only when
    `non_scalar_values != None` to produce composite container values.
  - `factory: StdValueFactory` — backing for the builder.

- `JsonModemBuffersIter<'a>` (iterator):
  - `inner: JsonModemIter<'a>` — the core event source.
  - `parent: *mut JsonModemBuffers` — to update adapter state.
  - No pending_* fields, no EOF flags, no stashing.

### Mapping Algorithm

For each `ParseEvent` from `inner.next()`:

1) Update builder (if present):
   - Scalars: `set` on the builder at `path.last()`.
   - Strings: `mutate_with` to append the fragment to a leaf string.
   - Container starts: `enter_with` a new array/object.
   - Container ends: according to `NonScalarValueMode`:
     - `None`: pop or read root but don’t surface (`value = None`).
     - `Roots`: if `path.is_empty()`, set `composite = Some(root.clone())`, else `pop`.
     - `All`: if `path.is_empty()`, `composite = Some(root.clone())`; else `pop` and capture the leaf value into `composite`.

2) String buffering:
   - If this event is `String { path, fragment, is_final, .. }`:
     - If `scratch` exists with same `path`, append; otherwise set `scratch = (path.clone(), fragment.clone())`.
     - Compute `value` by `BufferStringMode` (see Semantics above).
     - If `is_final`, clear `scratch`.

3) Container close events:
   - Before emitting, clear `scratch` if it belongs to a deeper path than the
     closing container.
   - Emit `BufferedEvent::{ArrayEnd,ObjectEnd}` with `value = composite` as
     determined by step (1).

4) Emit the buffered event exactly once per core event.

### Error Handling & Performance

- All builder updates return `Result`; during mapping we treat builder errors
  as impossible for valid core inputs and `ok()` the results (consistent with
  existing builder usage). If needed, strict error propagation can be revisited.
- Strings: Values mode clones the coalesced buffer; prefixes mode clones on
  final only. The common case keeps scratch small by clearing at `is_final`.

## Migration Strategy

1) Move `builder` and `factory` from the iterator into `JsonModemBuffers`.
2) Replace the iterator’s custom state machine with the simple per‑event mapper.
3) Keep the `collect()` and `finish()` plumbing by delegating to the iterator.
4) Remove deprecated iterator state: `pending_path`, `pending_buf`, `pending_final`,
   `pending_last_fragment`, `stash_non_string`, `emitted_pending_on_eof`.

## Tests & Snapshots

- Unit/regression tests:
  - Values mode: two‐feed string produces `Some("he")` then `Some("hello")`.
  - Prefixes mode: multi‐fragment string yields `None, None, Some("allow")`.
  - NonScalar Roots: root `ObjectEnd` carries `Some(root)`.
  - NonScalar All: nested and root close events carry composite values.

- Integration snapshots (inline, no `.snap` files):
  - `layers`: core events, buffers (Values/Prefixes), values adapter output.
  - `buffers`: unrolled permutations by `BufferStringMode × NonScalarValueMode`.

- Snapshot process:
  - Update with `cargo insta test` and review with `cargo insta review`.
  - Use inline snapshots `@""`; avoid named snapshots and loops in assertions.

## Acceptance Criteria

- Iterator has no pending/EOF/stash fields; adapter owns `scratch` and builder.
- 1:1 mapping: the number of buffered events equals the number of core events.
- All string and non‑scalar modes pass the updated inline snapshots.
- The simple rules for Values/Prefixes are enforced and covered by tests.

## Risks

- Any remaining reliance on the old iterator state could cause subtle behavior
  differences. Keep the rewrite surgical and update only `jsonmodem_buffers.rs`
  mapping logic; ensure other modules (tests/examples) compile unchanged.

## Timeline

- Phase 1: Move state to adapter; implement per‑event mapping; compile/run tests.
- Phase 2: Tighten snapshots and add missing regression cases if discovered.

