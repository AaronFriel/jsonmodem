```

# ChatGPT Codex Execution Spec (ExecSpec)

## Context

This repository exposes a three-layer JSON streaming stack:
- `JsonModem` (core): event-only parser. Fragment-only strings; no composite/"non-scalar" value building; bounded memory.
- `JsonModemBuffers` (adapter): coalesces string fragments per path and can attach a buffered `value` per `BufferStringMode` (None/Values/Prefixes). Today it does not emit composite values.
- `JsonModemValues` (adapter): incrementally builds partial values and yields `StreamingValue { index, value, is_final }` via an iterator.

Recent refactors removed the builder from the core (`StreamingParserImpl`) and concentrated building in adapters. There is an open design choice: Should any layer besides `JsonModemValues` hold a `ValueBuilder`? Specifically, do we:
1) Keep the core free of any builder and build only in adapters, or
2) Add an optional builder to the core (hidden), or
3) Add an optional builder to the buffers adapter to emit composite (“non-scalar”) values while preserving buffered string semantics?

This doc evaluates the trade‑offs and recommends a path that prioritizes user clarity, predictable memory, and layering.

## Goal / Scope

- Decide where a `ValueBuilder` should live: core vs. adapters.
- Determine whether `JsonModemBuffers` should optionally build composite values.
- Specify the user-facing knobs to control partial/complete emissions for `JsonModemValues` and composite emission for `JsonModemBuffers`.
- Keep the core event surface clean and memory-bounded by default.

Non-goals:
- Changing `ParseEvent` shape in the core.
- Introducing path-based policies or size limits (out of scope for this decision).

## Definitions

- Core: `StreamingParserImpl` behind `JsonModem` emitting `ParseEvent` only.
- Builder: `ValueBuilder<Value>` used to incrementally assemble JSON values.
- Non-scalar values: composite arrays/objects (as opposed to scalar null/bool/number/string). Emitting them requires holding state and sometimes cloning.

## Alternatives

Alt A — Core holds `Option<ValueBuilder>` (hidden/pub(crate))
- Core optionally maintains a builder. When enabled (via a hidden option), it can enrich certain events (e.g., ArrayEnd/ObjectEnd) with values or provide roots snapshots.
- Buffers/Values build on top as today, possibly reusing the core’s builder state.

Alt B — Core never holds a builder; only adapters build
- `JsonModem`: events only. No builder in `StreamingParserImpl`. Minimal, bounded memory.
- `JsonModemBuffers`: remains string-coalescing only, emits `BufferedEvent` with optional string values; optionally can build non-scalar values via its own internal builder if we decide.
- `JsonModemValues`: owns its `ValueBuilder` and emits partial/complete values.

Alt C — Core remains clean; both adapters can hold their own builders
- `JsonModemValues`: always holds a builder; add a `partial: bool` option to control partial root emissions (clone of current root vs only final).
- `JsonModemBuffers`: add `non_scalar_values: NonScalarValueMode` in `BufferOptions` and an internal `Option<ValueBuilder>` that is active when `!= None` to optionally emit composite values at container close events.

## Analysis

User mental model
- `JsonModem` users expect “just events” with minimal overhead and predictable memory use. A builder in the core—even if hidden—blurs expectations and complicates reasoning about memory.
- `JsonModemValues` users want values, not events. They want a builder and ergonomics for partial vs complete root emissions. A `partial: bool` fits this audience directly.
- `JsonModemBuffers` users want “events, but nicer strings”. Some may also want completed arrays/objects at emission points (array/object end), but others still want the event shape. Making composite emission opt-in at the adapter level aligns with the layer’s spirit while preserving core purity.

Performance & memory
- Builder in core (Alt A) imposes overhead—even if unused—by complicating code paths and risking accidental allocations. It also invites future creep (e.g., path policies), which hurts the core’s bounded footprint.
- Keeping builders only in adapters (Alt B/C) preserves core performance and memory bounds. Users who opt into adapters accept the added overhead.

API clarity
- Hiding a core builder behind `pub(crate)` (Alt A) creates a split-brain: core appears minimal, but logic becomes complex and harder to test/isolate. It also risks confusion if events are subtly enriched depending on hidden flags.
- Adapters explicitly owning builders (Alt C) is transparent: the adapter does more, so it rightfully owns state.

Testing & snapshots
- Keeping core event-only simplifies test snapshots and fuzzing.
- Emitting composite values at adapter boundaries (Buffers/Values) is natural to snapshot and reason about—particularly for education and demos.

Ergonomics
- `JsonModemValues(partial: bool)`: the partial flag lets users choose “dribble intermediate roots” vs “only emit finals”. It also maps well to streaming UI and batch processing use cases.
- `JsonModemBuffers(non_scalar_values: …)`: opt-in emission of arrays/objects at container close events expands the adapter’s utility without polluting the core. It can be implemented with a `ValueBuilder` that mutates alongside the buffering logic.

## Recommendation (Alt C with feature gating)

Adopt Alt C as the cross-language design, and gate the Rust adapters behind default Cargo features so bindings can opt out cleanly:

1) Core stays clean (unchanged)
   - No builder in `StreamingParserImpl` or `JsonModem`.
   - Rationale: Preserve the smallest, most portable FFI surface and predictable memory.

2) `JsonModemBuffers` (Rust) gains optional composite emission
   - Add `non_scalar_values: NonScalarValueMode` to `BufferOptions`.
   - Internally hold an `Option<ValueBuilder>` that is active when `non_scalar_values != None` to emit arrays/objects at container close events.
   - This aligns with Alt C: buffers remain event-first with string coalescing, and can optionally surface composites when users opt in.

3) `JsonModemValues` (Rust) remains the value-building adapter
   - Always owns a builder; add a `partial: bool` option to control intermediate root emission vs only finals.

4) Feature-gate adapters in Rust
   - Introduce Cargo features: `buffers` and `values`.
   - Enable both in `jsonmodem` by default (default-features include `buffers`, `values`).
   - Compile adapters conditionally so the public surface is hidden when features are disabled.

5) Python bindings strategy (jsonmodem-py)
   - Depend on `jsonmodem` with `default-features = false`, exposing only the core `JsonModem` event parser to Python.
   - Reimplement `JsonModemBuffers` and `JsonModemValues` in Python atop the Rust `JsonModem` stream (future execution specs will define this work in detail).

6) Documentation for users
   - `JsonModem`: events only; minimal overhead; stable FFI.
   - `JsonModemBuffers` (Rust, feature `buffers`): string coalescing plus optional composite emission controlled by `non_scalar_values`.
   - `JsonModemValues` (Rust, feature `values`): emits values; supports `partial` emissions.
   - Python: wrappers provide Buffers/Values behavior in Python space while using Rust for the core stream.

## User Experience Summary

- Want the fastest, smallest surface? Use `JsonModem`.
- Want to render strings incrementally and make prefix decisions but stay event-driven? Use `JsonModemBuffers` with `string_values = Prefixes` and, if desired, enable composite emission via `non_scalar_values`.
- Want to get values, not events? Use `JsonModemValues` and choose whether you need partial or finals.

## Milestones

1) JsonModemBuffers (Rust): Add optional composites
- Extend `BufferOptions` with `non_scalar_values`.
- Implement internal builder synced to buffering when enabled; emit composites at container close.
- Add tests and snapshots to cover both modes (off/on).

2) JsonModemValues (Rust): Add `partial` option
- Add `ValuesOptions { partial: bool }` and constructor `with_options`.
- Emit intermediate root snapshots when `partial = true`; document tradeoffs.

3) Feature-gate adapters
- Add Cargo features `buffers` and `values`; enable by default.
- Conditionally compile adapter modules and re-export items behind features.

4) Python bindings plan
- Make `jsonmodem-py` depend on `jsonmodem` with `default-features = false`.
- Defer Python-side Buffers/Values implementations to future execution specs; outline API parity and behavior alignment goals.

5) Docs & examples
- Expand README and examples to snapshot:
-   - Core events (JsonModem)
-   - Buffers (strings + optional composites)
-   - Values adapter with and without partial emissions (Rust) and note Python parity goals.

## Notes for the Implementer

- Keep the core free of any builder. Do not re-introduce `EventsOut` or similar sinks.
- In `JsonModemBuffers` (feature `buffers`), when composite emission is enabled:
-   - Track container starts/ends to keep the builder in sync.
-   - Emit composites via `BufferedEvent::{ArrayEnd,ObjectEnd}` with an optional `value` field (None when disabled, Some when allowed by policy and path).
-   - Avoid heavy cloning by only cloning root when required; do not deep-copy subtrees unless strictly needed.
- In `JsonModemValues` (feature `values`):
-   - Reserve builder capacity across feeds; expose iterator-based API.
-   - Define `partial` semantics precisely (emit at meaningful boundaries only; avoid spamming on every fragment unless explicitly desired).

## Decision

- Adopt Alt C: core remains event-only; adapters own their builders.
- Add optional composite emission to `JsonModemBuffers` via `non_scalar_values` and an internal builder.
- Add `partial` support to `JsonModemValues` for intermediate root emissions.
- Gate adapters behind `buffers` and `values` Cargo features (enabled by default). Bindings like `jsonmodem-py` depend on `jsonmodem` with `default-features = false` to expose only the core.
- Reimplement Buffers/Values in Python on top of the Rust core in future execution specs.

```

## TODO (Alt C Implementation Checklist)

- [x] Core kept event-only (`JsonModem` forces `non_scalar_values = None`).
- [ ] Gate adapters behind Cargo features `buffers` and `values` (default-on).
- [x] Gate adapters behind Cargo features `buffers` and `values` (default-on).
- [ ] Add `non_scalar_values: NonScalarValueMode` to `BufferOptions`.
- [x] Add `non_scalar_values: NonScalarValueMode` to `BufferOptions`.
- [ ] In `JsonModemBuffers`, optionally emit composite values at container close using internal `ValueBuilder` when `non_scalar_values != None`.
- [x] In `JsonModemBuffers`, optionally emit composite values at container close using internal `ValueBuilder` when `non_scalar_values != None`.
- [ ] Extend `BufferedEvent::{ArrayEnd,ObjectEnd}` to carry optional composite `value`.
- [x] Extend `BufferedEvent::{ArrayEnd,ObjectEnd}` to carry optional composite `value`.
- [ ] Add `partial: bool` option to `JsonModemValues` to control intermediate root emissions.
- [x] Add `partial: bool` option to `JsonModemValues` to control intermediate root emissions.
- [x] Remove `ParserOptions.non_scalar_values`; core no longer exposes composite emission.
- [ ] Update README and examples to reflect features and adapter behavior.

```
