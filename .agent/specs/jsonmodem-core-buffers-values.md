```

# ChatGPT Codex Execution Spec (ExecSpec)

## Context

This repository contains a `jsonmodem` Rust crate that implements an incremental, event‑driven JSON streaming parser. The current core parser type is `StreamingParserImpl<V>` with a public alias `DefaultStreamingParser` (for `Value`). It yields `ParseEvent<V>` items and optionally builds string values and non‑scalar composite values (arrays/objects) depending on `ParserOptions` flags: `string_value_mode: StringValueMode::{None,Values,Prefixes}` and `non_scalar_values: NonScalarValueMode::{None,Roots,All}`. Value building is implemented by `event_stack::EventStack` + `value_zipper::{ValueBuilder, ValueZipper}`. A separate `StreamingValuesParserImpl` wrapper exists that collects complete values at chunk boundaries using the same builder. The library re‑exports these via `lib.rs` alongside internal modules like `parser.rs`, `event.rs`, `factory.rs`, `options.rs`, `value_zipper.rs`, and tests/benches.

The requested change: make `JsonModem` the core type focused purely on streaming parse events with minimal overhead (no value building, no string buffering). Then add two wrappers: `JsonModemBuffers` that buffers string values for parse events, and `JsonModemValues` that exposes a `jiter`‑equivalent interface by producing low‑overhead partial JSON value objects, leveraging the existing zipper/builder (#56 intent reflected by `value_zipper.rs`).

## Goal / Scope

- Introduce `JsonModem`: a minimal, fast streaming event parser with no string value buffering and no array/object value building.
- Introduce `JsonModemBuffers`: a wrapper over `JsonModem` that buffers string fragments to present string `value`s when appropriate.
- Introduce `JsonModemValues`: a wrapper that builds partial JSON value objects and yields them incrementally, equivalent to `jiter` behavior, reusing the existing `ValueBuilder`/`ValueZipper`.
- Refactor `ParseEvent` and options so the core does not include value‑building or string buffering behavior.
- Preserve existing performance characteristics; keep public exports stable via aliases where possible; add tests and docs.

Out of scope in this ExecSpec:
- Updating Python bindings (`crates/jsonmodem-py`) or external consumers; we will provide migration notes and type aliases to ease transition.
- Renaming internal modules unrelated to the three new types.

## Definitions

- JsonModem: The new core parser type, a thin façade over the lex/parse state machine that yields low‑overhead parse events (string fragments only, no composite value building).
- JsonModemBuffers: A stateful adapter that accumulates string fragments from `JsonModem` and emits events where `value` contains buffered content (final or prefix, per policy), while still forwarding fragments.
- JsonModemValues: A stateful adapter that consumes `JsonModem` events and builds partial JSON values using `ValueBuilder` with root/leaf progress exposure equivalent to `jiter`.
- ParseEventCore: The refined event enum (reusing `ParseEvent` file) where string events carry fragments and `ArrayEnd`/`ObjectEnd` events carry no built values.
- Partial JSON value object: A `Value` (or generic `V: JsonValue`) built incrementally via `ValueBuilder`/`ValueZipper`, not necessarily finished at chunk boundaries.

## File Map

crates/jsonmodem/src/event.rs    modify    Trim event payloads for core (no built non‑scalars; keep string fragment fields, remove core‑level string `value`)
crates/jsonmodem/src/options.rs  modify    Remove/deprecate core string/composite value emission; retain only parsing behavior flags; add deprecation notes
crates/jsonmodem/src/parser.rs   modify    Refactor to emit only core events; extract as `JsonModem`; remove EventStack usage from core path
crates/jsonmodem/src/event_stack.rs modify Keep for wrappers; gate usage to `JsonModemValues` only
crates/jsonmodem/src/streaming_values.rs modify Rename/refactor into `jsonmodem_values.rs`; adapt to consume `JsonModem` events
crates/jsonmodem/src/jsonmodem_buffers.rs create    Implement `JsonModemBuffers` wrapper and buffering policy
crates/jsonmodem/src/jsonmodem_values.rs  create    Implement `JsonModemValues` using `ValueBuilder`
crates/jsonmodem/src/lib.rs       modify    Export `JsonModem`, `JsonModemBuffers`, `JsonModemValues`; keep back‑compat aliases
crates/jsonmodem/src/tests/*.rs   modify/add Update and add tests for new types and behaviors
README.md                         modify    Update examples to use `JsonModem` and wrappers

## API & Data Structures

Note: This crate is no_std + alloc; no external dependencies are introduced or version‑pinned in this plan.

1) Events (core)
    enum ParseEvent {
        Null { path: Path },
        Boolean { path: Path, value: bool },
        Number { path: Path, value: f64 },
        String { path: Path, fragment: Str, is_final: bool },
        ArrayStart { path: Path },
        ArrayEnd { path: Path },
        ObjectBegin { path: Path },
        ObjectEnd { path: Path },
    }
    - Remove optional `value: Option<V::Str>` from String (core never fills it).
    - Remove `value: Option<V::Array/Object>` from ArrayEnd/ObjectEnd.
    - Keep generic form in code by constraining to built‑in `Value` types, or specialize to crate `Value`; wrappers may layer additional info without changing core enum.

2) Core parser
    struct JsonModem {
        // same internal lex/parse state as current StreamingParserImpl<Value>, minus EventStack/ValueBuilder
    }
    impl JsonModem {
        fn new(opts: ParserOptions) -> Self
        fn feed<'a>(&'a mut self, text: &str) -> JsonModemIter<'a>
        fn finish(self) -> JsonModemClosed
    }
    impl Iterator for JsonModemIter<'_> { type Item = Result<ParseEvent, ParserError>; }
    impl Iterator for JsonModemClosed { type Item = Result<ParseEvent, ParserError>; }

3) Parser options (core)
    struct ParserOptions {
        allow_unicode_whitespace: bool,
        allow_multiple_json_values: bool,
        #[cfg(any(test, feature = "fuzzing"))]
        panic_on_error: bool,
    }
    - Remove `string_value_mode` and `non_scalar_values` from core options.
    - Provide deprecated shims (behind `#[cfg(feature = "compat")]`) or type aliases for old names if needed.

4) String buffering wrapper
    enum BufferingMode { Values /* final only */, Prefixes /* emit growing prefix */ }
    struct JsonModemBuffers {
        modem: JsonModem,
        mode: BufferingMode,
        // current string buffers keyed by path; retained only while inside a string
        scratch: alloc::collections::BTreeMap<Path, Str>,
    }
    // Output iterator yields the same `ParseEvent` shape as core, but with an added `value: Option<Str>` in String events.
    // To avoid changing the core enum, the wrapper maps to a new adapter event type:
    enum BufferedEvent {
        String { path: Path, fragment: Str, value: Option<Str>, is_final: bool },
        // all other variants mirror ParseEvent exactly
        Null { path: Path },
        Boolean { path: Path, value: bool },
        Number { path: Path, value: f64 },
        ArrayStart { path: Path },
        ArrayEnd { path: Path },
        ObjectBegin { path: Path },
        ObjectEnd { path: Path },
    }

5) Values wrapper
    struct StreamingValue<V: JsonValue> { index: usize, value: V, is_final: bool }
    struct JsonModemValues<V: JsonValue = Value> {
        modem: JsonModem,
        builder: value_zipper::ValueBuilder<V>,
        index: usize,
        factory: StdValueFactory, // or generic over JsonValueFactory
    }
    impl<V: JsonValue> JsonModemValues<V> {
        fn new(opts: ParserOptions) -> Self // opts used only for JsonModem; composite building is internal
        fn feed(&mut self, chunk: &str) -> Result<Vec<StreamingValue<V>>, ParserError>
        fn finish(self) -> Result<Vec<StreamingValue<V>>, ParserError>
    }
    - Emission policy: push a finished `StreamingValue` when encountering primitive roots or end of a root container; for strings, emit `is_final=false` for mid‑string partials if a root string spans chunks.

## Algorithms / Control Flow

1) JsonModem (core)
    - Retain existing lexing and parse state machines (`LexState`, `ParseState`, `FrameStack`, `Buffer`, `UnicodeEscapeBuffer`).
    - Remove all interactions with `event_stack::EventStack` and `ValueBuilder` from the core event push path.
    - Produce events:
        - For strings: emit `ParseEvent::String { path, fragment, is_final }`, where `fragment` is the incremental text slice accumulated in the core `buffer` for that step; `is_final` is true when closing the string token or EOF finalizes it; do not provide a full `value`.
        - For arrays/objects: emit `ArrayStart`/`ArrayEnd` and `ObjectBegin`/`ObjectEnd` without any built values.
        - For primitives: unchanged (`Boolean { value }`, `Number { value }`, `Null`).
    - Preserve `allow_multiple_json_values` handling and path computation via `FrameStack`.

2) JsonModemBuffers
    - Maintain a map `scratch: BTreeMap<Path, Str>` keyed by current string paths; insert on first string fragment for that path.
    - On `ParseEvent::String` from core:
        - Append fragment to `scratch[path]`.
        - If `mode == Values` and `is_final == true`: emit `BufferedEvent::String { value: Some(full) }` and remove entry.
        - If `mode == Prefixes`: emit `BufferedEvent::String { value: Some(current_prefix.clone()) }`; on final, also remove entry.
        - Always forward `fragment` and `is_final` unchanged for incremental UI rendering use cases.
    - On container begin/end events and other scalar events: forward as‑is; clear any stray string buffers when the path is popped (defensive cleanup in case of malformed streams).

3) JsonModemValues
    - Consume `JsonModem` events; map them into `ValueBuilder` mutations:
        - Null/Boolean/Number: `builder.set(path.last(), built_value, &mut factory)`.
        - String: `builder.mutate_with` to ensure a string exists at the leaf and append fragment; when `is_final && path.is_empty()` push a `StreamingValue { is_final: true }`.
        - ArrayStart/ObjectBegin: `builder.enter_with(path.last(), ...)` to create containers.
        - ArrayEnd/ObjectEnd: if at root (`path.is_empty()`), extract the finished root (`builder.into_value()` or clone via `read_root`/`pop`) and push a `StreamingValue { is_final: true }` and reset the builder.
    - Maintain `index` monotonically increasing across emitted values (multi‑root streams).
    - `finish`: drain any remaining core events, then if a partial root exists (`builder.read_root().is_some()`), emit it once with `is_final=false`.

Determinism & tie‑breakers:
- For overlapping string fragments on the same path (should not happen in valid JSON), the last fragment wins; scratch entry is cleared only on `is_final` for that exact path.
- For `allow_multiple_json_values=true`, root boundaries are driven by empty `path` in ArrayEnd/ObjectEnd and primitive events; indices increment per finished root only.

## Tests

Add new tests and adapt existing ones. Commands reflect `.agent/AGENTS.md` flows with `bench-fast`/`test-fast` features enabled where relevant.

- Core JsonModem
    - Emits fragments only: split a string across feeds and assert a sequence of `String { fragment, is_final }` without any full `value` fields.
    - Root boundaries for primitives and containers: assert correct `path` sequences and event ordering for arrays/objects.
    - Multiple values: feed `1 2 [3]` with `allow_multiple_json_values=true` and assert root transitions.

- JsonModemBuffers (Values mode)
    - Single string value in one chunk: final event includes `value=Some("abc")` and `is_final=true`.
    - Split string across feeds: intermediate fragments have `value=None` (Values mode), final has `value=Some("abcdef")`.
    - Prefixes mode: every fragment emits `value=Some(prefix)` growing until final.

- JsonModemValues
    - Primitive roots: feed `1 2` across chunks and assert two `StreamingValue { 1.0, 2.0, is_final=true }` with indices 0,1.
    - Root string across chunks: first chunk emits `is_final=false` partial only at `finish`, or no emission until final depending on policy; ensure final produces `is_final=true` and partial emission behavior is documented.
    - Nested arrays/objects across chunks: assert correct emission at container ends and that builder resets for next root.
    - Property names and paths: verify `Path` components match keys/indices for nested structures.

Failure paths
- Invalid JSON mid‑string: ensure `ParserError` surfaces from the core and wrappers propagate it without partial mutation state leaks (buffers cleared).
- Unfinished string at EOF: `JsonModem` must mark `is_final=true` and close; wrappers emit consistent finalization.

## Milestones

### Milestone 1: Define Core Events and Type Aliases
    Scope
        - Modify `event.rs` to remove composite value payloads and string `value` in core `ParseEvent`.
        - Add back‑compat aliases or serde gates as needed for tests; keep JSON snapshot stability for non‑value fields.
    Commands
        - cargo build --all --workspace --features bench-fast --features test-fast
        - cargo test -p jsonmodem --features bench-fast --features test-fast
        - cargo clippy --workspace --all-targets -- -D warnings
    Acceptance
        - Crate compiles; existing tests that do not depend on built values pass or are updated.
    Idempotence
        - Re‑running does not change generated code; only source edits persist.
    Rollback/Fallback
        - If tests require legacy fields, add a `#[cfg(feature = "compat")]` adapter enum and migrate incrementally.

### Milestone 2: Extract JsonModem Core
    Scope
        - Refactor `parser.rs` to `JsonModem` (keep old alias `StreamingParser` for back‑compat).
        - Remove `EventStack`/`ValueBuilder` integration from the core push path; retain path tracking and tokenization.
        - Trim `ParserOptions` to core fields; deprecate removed flags.
    Commands
        - cargo build/test/clippy (as above)
    Acceptance
        - Size of `JsonModem` remains small (comparable to current), tests for primitive and structural events pass.
    Idempotence
        - Safe to re‑run.
    Rollback/Fallback
        - Keep a temporary feature flag to compile old code paths if unexpected regressions block progress.

### Milestone 3: Implement JsonModemBuffers
    Scope
        - Create `jsonmodem_buffers.rs` adapter implementing `BufferingMode::{Values,Prefixes}` and outputting `BufferedEvent`.
        - Add tests for both modes and multi‑feed strings.
    Commands
        - cargo test -p jsonmodem --features bench-fast --features test-fast -- tests::buffers
    Acceptance
        - Buffering behavior matches `StringValueMode` legacy semantics.
    Idempotence
        - Stateless aside from internal map; re‑runs stable.
    Rollback/Fallback
        - Fall back to `Values` only if `Prefixes` introduces complexity.

### Milestone 4: Implement JsonModemValues
    Scope
        - Create `jsonmodem_values.rs` using `ValueBuilder` to construct partial values from core events.
        - Replace existing `StreamingValuesParserImpl` with a thin shim or re‑export.
        - Tests mirroring `StreamingValuesParser` expectations and `jiter` parity scenarios.
    Commands
        - cargo test -p jsonmodem --features bench-fast --features test-fast -- tests::values
    Acceptance
        - Produces `StreamingValue<V>` lists per feed/finish; indices increment, `is_final` semantics match spec.
    Idempotence
        - Stable across re‑runs.
    Rollback/Fallback
        - Keep old adapter behind a feature if necessary until parity validated.

### Milestone 5: Public Exports & Docs
    Scope
        - Update `lib.rs` to export `JsonModem`, `JsonModemBuffers`, `JsonModemValues` and maintain compatibility aliases.
        - Refresh README examples to use `JsonModem` + wrapper guidance.
    Commands
        - cargo doc -p jsonmodem --no-deps
        - cargo test
    Acceptance
        - Docs build; examples compile; CI checks pass.
    Idempotence
        - Docs remain stable.
    Rollback/Fallback
        - Keep README note about legacy names if needed.

## Notes for the Implementer

- Keep allocations minimal in core: continue reusing the internal `Buffer` for lexing; do not clone strings in the core path. Fragments can be constructed from the internal buffer as owned `Str` to avoid lifetimes that outlive `feed` iterations.
- `Path` handling must remain consistent with existing tests (keys vs indices); wrappers must key buffers by the full `Path` snapshot to avoid accidental collisions.
- Ensure `finish()` behavior completes any unterminated primitives/strings consistent with current logic; wrappers should act on `is_final=true` only.
- Error reporting: surface `ParserError` directly from core; wrappers should stop and return the error without emitting additional items; clear internal state on error.
- Back‑compat: provide `type StreamingParser = JsonModem;` and `type StreamingValuesParser = JsonModemValues;` to reduce downstream churn; consider a `compat` feature to re‑enable legacy `StringValueMode` on the wrapper layer only.

## Next Work

- Update Python bindings to expose the three layers (`JsonModem`, `JsonModemBuffers`, `JsonModemValues`) with ergonomic Pythonic APIs.
- Add benches contrasting core vs wrappers; include `comparison` feature variants.
- Explore borrowing string fragments to avoid intermediate `Str` allocations if safe lifetimes can be guaranteed.

## Handoff (Required if Partial)

If progress is blocked or time runs out, return a Short Handoff ExecSpec limited to the remaining milestones, embedding:
- The exact compiler/test outputs (exit codes and stderr excerpts).
- A concise `git status` and `git diff --patch` showing which files changed.
- A trimmed set of API/Algorithm/Test deltas necessary to complete the next milestone.

```

