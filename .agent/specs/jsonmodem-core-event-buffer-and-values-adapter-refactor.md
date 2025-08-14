```

# ChatGPT Codex Execution Spec (ExecSpec)

## Context

The `jsonmodem` crate has been refactored into three layers:
- `JsonModem`: low-overhead streaming event parser that emits fragment-only strings and no composite values (`non_scalar_values = None`).
- `JsonModemBuffers`: adapter that coalesces string fragments per-path and optionally attaches a buffered full/prefix value in `BufferedEvent::String { value: Option<Str> }`.
- `JsonModemValues`: adapter that produces low-overhead partial values using `ValueBuilder/ValueZipper` and emits `StreamingValue { index, value, is_final }`.

Internally, the parser (`StreamingParserImpl<V>`) introduced an `EventsOut` abstraction to route events either into a `Vec` or an `EventStack` when building composites. With the new layered design, the core should not juggle multiple sinks: it should collect events in a single internal buffer, and adapters should build higher-level artifacts on top. Likewise, `JsonModemValues` should mirror the `JsonModemBuffers` pattern by keeping a `ValueBuilder` and a small output queue instead of relying on parser-internal routing.

## Goal / Scope

- Remove `EventsOut` from the core: `StreamingParserImpl<V>` maintains an internal `Vec<ParseEvent<V>>` buffer for events, drained by iterators returned from `feed()`/`finish()`.
- Refactor `JsonModemValues` to use an internal `ValueBuilder` and a per-feed output buffer (queue), analogous to `JsonModemBuffers` string coalescing and output semantics.
- Preserve public APIs and existing behavior; no path-based policies or size/overflow controls are introduced.
- Update tests/benches/docs accordingly; keep performance and memory characteristics stable.

Out of scope:
- Any path include/exclude rules; any buffer size limits or overflow policies.
- Changes to `ParseEvent` shape or `JsonModemBuffers` behavior.

## Definitions

- EventsOut (to remove): internal routing mechanism in the parser used to push events into either a flat vector or an `EventStack`.
- Event Buffer (core): `Vec<ParseEvent<V>>` owned by `StreamingParserImpl<V>` accumulating events during a `feed()`; its iterator drains and clears it.
- Values Output Queue: `Vec<StreamingValue<V>>` inside `JsonModemValues` holding emitted items during a `feed()`; cleared between feeds to reuse capacity.
- Root completion: a primitive at empty path, or `ArrayEnd/ObjectEnd` with empty path; triggers a `StreamingValue { is_final: true }` emission.

## File Map

crates/jsonmodem/src/parser.rs           modify   Remove `EventsOut`; add internal event buffer; adapt iterators to drain/clear it.
crates/jsonmodem/src/event_stack.rs      modify   Drop `EventsOut` usage; retain only what’s needed for tests/utilities (if any).
crates/jsonmodem/src/jsonmodem_values.rs modify   Use internal `ValueBuilder` + output queue; no parser-internal routing.
crates/jsonmodem/src/lib.rs              modify   Remove any re-exports tied to `EventsOut` (if present); keep public API stable.
crates/jsonmodem/src/tests/*.rs          modify   Add iterator drain tests; ensure values adapter parity; remove `EventsOut` assumptions.
benches/*                                modify   Update internals if they referenced `EventsOut`; ensure they build.
README.md                                modify   Briefly note internal simplification (no breaking changes).

## API & Data Structures

Public signatures remain unchanged; below are internal structures for clarity.

1) Parser internals
    struct StreamingParserImpl<V: JsonValue> {
        // existing lex/parse state ...
        events_out: alloc::vec::Vec<ParseEvent<V>>, // replaces EventsOut
    }

    pub struct JsonModemIter<'a, V: JsonValue> {
        parser: &'a mut StreamingParserImpl<V>,
        cursor: usize,
    }

    impl<V: JsonValue> StreamingParserImpl<V> {
        pub fn new(options: ParserOptions) -> Self;
        pub fn feed<'a>(&'a mut self, chunk: &str) -> JsonModemIter<'a, V>;
        pub fn finish(self) -> ClosedStreamingParser<StdValueFactory>;
    }

2) Values adapter internals
    pub struct JsonModemValues<V: JsonValue = Value> {
        modem: JsonModem,
        builder: value_zipper::ValueBuilder<V>,
        out: alloc::vec::Vec<StreamingValue<V>>, // reused per feed
        index: usize,
        factory: StdValueFactory,
    }

    impl<V: JsonValue> JsonModemValues<V> {
        pub fn new(options: ParserOptions) -> Self;
        pub fn feed(&mut self, chunk: &str) -> Result<alloc::vec::Vec<StreamingValue<V>>, ParserError>;
        pub fn finish(self) -> Result<alloc::vec::Vec<StreamingValue<V>>, ParserError>;
    }

## Algorithms / Control Flow

1) Parser event buffer (replace EventsOut)
    - Initialize `events_out` empty (reserve a small capacity, e.g., 16).
    - During tokenization, push `ParseEvent<V>` values directly into `events_out` in encounter order.
    - `feed(chunk)` returns `JsonModemIter { parser: self, cursor: 0 }` after tokenizing `chunk`.
    - `JsonModemIter::next()`:
        - If `cursor < events_out.len()`, return a clone of `events_out[cursor]` and increment `cursor`.
        - On first `None`, call `events_out.clear()` to reuse capacity and ensure no stale events remain.
    - `finish()` finalizes syntax, pushes any last events, then returns a closed iterator that drains the remaining buffer exactly once.

2) JsonModemValues as adapter with internal queue
    - For each event from `modem.feed(chunk)`:
        - Apply it to `builder` (append string fragments, set primitives, open/close containers).
        - On root completion, clone or take the built root via the builder/factory and push `StreamingValue { index: self.index, value, is_final: true }` into `out`; then increment `index` and reset the builder’s root context.
        - For mid-string or mid-container events at non-root paths, do not emit.
    - At the end of `feed`, return `core::mem::take(&mut out)`.
    - `finish()` drains remaining events via `modem.finish()` and repeats the same logic; return the final `out`.

Edge handling:
- Ensure primitive roots that occur back-to-back (e.g., `1 2 3`) each emit once.
- A top-level string split across chunks must emit only once on final fragment.
- Reuse `out` allocation across feeds.

## Tests

- Iterator drain semantics:
  - A single `feed` producing N events yields them in order; a second iteration over the same iterator yields none.
  - After the iterator completes, the next `feed` on new input yields only new events (buffer cleared).

- Finish semantics:
  - Pending events before `finish()` are observed exactly once by the closed iterator.

- Values adapter parity:
  - Compare `JsonModemValues` emissions to `event::test_util::reconstruct_values` for arrays, objects, primitives, nested structures, and multi-root streams.
  - Verify top-level streaming string emits only once on final fragment.

- Regression coverage:
  - Existing adapter tests (buffers coalescing, core fragment-only behavior) remain green.
  - Benches compile.

## Milestones

### Milestone 1: Replace EventsOut with Event Buffer in Parser
- Scope:
  - Remove `EventsOut` and introduce `events_out: Vec<ParseEvent<V>>` in `StreamingParserImpl`.
  - Update all event emission sites to push into `events_out`.
  - Implement iterator drain/clear behavior for `feed` and `finish`.
- Commands:
  - cargo test -p jsonmodem
  - cargo clippy -p jsonmodem -- -D warnings
- Acceptance:
  - All existing tests pass with identical event sequences.
  - No new clippy warnings.

### Milestone 2: Refactor JsonModemValues to Adapter Pattern
- Scope:
  - Maintain an internal `ValueBuilder` and `out` queue in `jsonmodem_values.rs`.
  - Emit only on root completion; align with reconstruction helper results.
- Commands:
  - cargo test -p jsonmodem crates/jsonmodem/src/tests/adapters.rs::jsonmodem_values_*
  - cargo test -p jsonmodem
- Acceptance:
  - Parity with reconstruction across representative inputs.
  - No regressions in existing adapter tests.

### Milestone 3: Cleanup, Benches, and Docs
- Scope:
  - Remove dead `EventsOut` code paths and references.
  - Update README to note internal simplification (no API changes).
  - Ensure benches compile; investigate any performance deltas.
- Commands:
  - cargo bench -p jsonmodem --no-run
  - AGENT_CHECK_MIRI_DISABLE=true .agent/check.sh
- Acceptance:
  - Benches build; check passes locally (modulo external tools like actionlint).

## Notes for the Implementer

- Maintain `no_std + alloc` and avoid `std`.
- Reuse allocations by clearing vectors; avoid per-iteration allocations in iterators.
- Keep iterator lifetimes/borrows simple: drain by index and clear on completion.
- Do not change public enums or adapter APIs; this is an internal cleanup.

## Next Work

- Consider exposing a step-wise API that yields after a fixed number of events.
- Explore zero-copy string/number fragments backed by input slices where safe.

```
