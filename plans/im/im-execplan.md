# Deliver an Immutable JsonModem Backend

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must be kept up to date as work proceeds.

Refer to `PLANS.md` in the repository root for the governing standards. This document is authored and maintained in accordance with those requirements.

## Purpose / Big Picture

JsonModem currently exposes only `StdBackend`, which materialises values using `String`, `Vec`, and `BTreeMap`. After completing this plan, contributors will be able to construct immutable JSON trees that reuse structure instead of cloning, by instantiating `JsonModem`, `JsonModemBuffers`, or `JsonModemValues` with a new `ImBackend`. The API surface will match the snippet in `docs/im/prompt.md`, and users can observe success by feeding chunked JSON into the immutable adapters and verifying the resulting values share structure, serialise correctly, and pass the existing adapters’ tests. The immutable backend will compile everywhere the crate runs today and will be exercised by benches and tests alongside the standard backend. Along the way, we will prototype a `JsonModemIm` interface that proves immutable roots can be exposed through ordinary iterators, reducing reliance on lending lifetimes for callers that prefer a simpler ownership model.

## Progress

- [x] (2025-02-14 00:00Z) Authored initial ExecPlan describing how to recreate the immutable backend from HEAD~1..HEAD.
- [x] (2025-02-14 02:15Z) Ran EcoString and rpds spikes (`cargo test --test im_spikes`) documenting copy-on-write semantics and persistent container mutations.
- [x] (2025-02-14 02:35Z) Prototyped a `JsonModemIm` adapter with stored immutable snapshots (`cargo test --features im --test im_adapter`).
- [x] (2025-02-14 02:45Z) Verified dependency scaffolding, upstream mirrors (`tmp/upstream/{ecow,rpds}`), and updated `docs/im/im-execplan.md` with spike findings.
- [x] (2025-02-14 02:50Z) Confirmed immutable backend modules (`value`, `value_zipper`, `value_applicator`, `mod`) satisfy trait integrations and doc coverage.
- [x] (2025-02-14 03:00Z) Exercised adapters/tests and Criterion benches (`cargo test --test jsonmodem_values`, `JSONMODEM_BENCH_FAST=1 cargo bench --bench streaming_json_medium`).
- [x] (2025-02-14 03:10Z) Finalised documentation and validation (`.agent/check.sh` clean, spec updated with spike/prototype findings).
- [x] (2025-02-14 03:20Z) Productised `JsonModemIm` behind the `im` feature (opt-in for consumers, enabled in `.agent/check.sh`).

## Surprises & Discoveries

- `ecow::EcoString` clones may immediately allocate separate inline buffers; pointer equality is not a reliable signal of sharing, but copy-on-write semantics still ensure independent mutation once we call `push_str`. Demonstrated in `crates/jsonmodem/tests/im_spikes.rs:9`.
- `rpds::VectorSync::push_back_mut` mutates a clone in place without affecting earlier clones, confirming we can maintain persistent prefixes when appending array items. Verified by `vector_sync_structural_sharing` in `crates/jsonmodem/tests/im_spikes.rs:32`.
- Storing `ImBackend` snapshots in a Vec enables a plain iterator over `&Value` without lending lifetimes; structural sharing keeps clones cheap and serde equality verifies stability (`crates/jsonmodem/tests/im_adapter.rs:38`).
- Criterion benches show the IM variants working end-to-end, albeit with expected overhead versus the std backend (e.g., `streaming_json_medium/jsonmodem_values_im` ≈ 160 µs vs. `jsonmodem_values` ≈ 72 µs with `JSONMODEM_BENCH_FAST=1`).

## Decision Log

- (2025-02-14) Exposed the `im_value` module at the crate root (`lib.rs`) so downstream prototypes and documentation can reference the immutable `Value` type directly without relying on private modules.
- (2025-02-14) Introduced the `im` Cargo feature, enabling `JsonModemIm` for local checks while keeping the crate’s default feature set empty for downstream consumers.

## Outcomes & Retrospective

- Immutable backend work is validated by green `.agent/check.sh`, targeted spikes, and benches comparing Std/IM paths. The prototype `JsonModemIm` demonstrates a viable direction for non-lending iteration, and documentation now captures empirical findings and open performance gaps.

## Context and Orientation

The repository hosts the `jsonmodem` crate in `crates/jsonmodem/`. Streaming adapters live under `crates/jsonmodem/src/`, with the standard backend in `backend/std/`. The new immutable backend will reside at `crates/jsonmodem/src/backend/im/`, parallel to the standard backend, so agents must mirror the existing structure when adding modules.

JsonModem pipelines parse events through contexts (`PathCtx`, `EventCtx`, `BuilderCtx`, etc.). `JsonModemBuffers` coalesces fragments, and `JsonModemValues` builds composite values by delegating to a `RootedBufferAssembler`. The immutable backend must implement these contexts and assemblers so that existing adapters continue to function unchanged from the caller’s perspective. Persistent data structures will come from two crates:

* `ecow::EcoString` provides a clone-on-write string with inline small-string storage. It is Send + Sync and allows zero-copy cloning, which is essential for string fragments observed multiple times.
* `rpds::VectorSync<T>` and `rpds::RedBlackTreeMapSync<K, V>` implement persistent (functional) vector and map types that provide structural sharing across clones and mutations. They allow pushing or updating elements without copying the entire collection.

The plan must also capture number handling. We will begin with `f64` to match existing behaviour, but the design must make it explicit how contributors could later swap to a more expressive number type under a feature flag. String decode modes (`StrictUnicode` versus `ReplaceInvalid`) require explicit surface APIs so callers can choose whether to lossy-convert invalid UTF-8 sequences.

## Plan of Work

Start with hands-on spikes. Build a scratch test module that exercises `ecow::EcoString` mutations, clone-on-write semantics, and interactions with reference types so we can prove that appending fragments or cloning values does not invalidate previously returned references. Create a companion spike that uses `rpds::VectorSync` and `rpds::RedBlackTreeMapSync` to simulate zipper operations: push array frames, insert object keys, mutate existing entries, and observe how `*_mut` APIs behave. These experiments should include assertions that verify sharing (for example, comparing pointer addresses or lengths before and after `push_back_mut`) and should be summarised in `Surprises & Discoveries` with exact file locations. Treat the spikes as executable documentation: they can live under `crates/jsonmodem/tests/` or an `experiments/` directory guarded by `#[cfg(test)]`.

With the behaviour understood, expand the spike into a prototype adapter. Implement a lightweight `JsonModemIm` proof-of-concept—either as a stand-alone module or an integration test—that feeds synthetic parse events into an immutable value, retains the root, and exposes a standard iterator returning `&Value`. Use a fake or simplified backend if necessary to sidestep parser complexity. The goal is to confirm that persistent data structures allow us to return references without lending lifetimes, and to reveal any borrow-checker constraints before we touch production code. Record the outcome in the plan: either document the path to a production-ready adapter or note why the prototype fails and what blockers remain.

Promote the validated design into a feature-gated production API. Create a `jsonmodem_im` module that is compiled when the `im` Cargo feature is enabled, implement a snapshotting `JsonModemIm` type with ergonomic iterators, and expose it from `lib.rs`. Update `.agent/check.sh` to pass `--features im` so local runs exercise the adapter, while keeping the crate’s default feature set empty for downstream consumers. Refresh documentation and tests to reference the new API and explain how to enable it.

After the exploratory work, prepare the workspace. Add `ecow` and `rpds` to `crates/jsonmodem/Cargo.toml`, regenerate `Cargo.lock`, ignore `tmp/upstream/`, and pull relevant upstream documentation into `tmp/upstream/ecow/` and `tmp/upstream/rpds/` so offline contributors can inspect APIs. Capture the dependency rationale, spike findings, and safety implications in `docs/im/im-execplan.md`, including a layperson explanation of zippers (stack-backed cursors that let us modify a tree in place) and copy-on-write semantics.

Next, scaffold the immutable backend modules. Mirror `backend/std/` by adding `value.rs` for the immutable `Value` enum and type aliases, `value_zipper.rs` for the persistent stack frames, and `value_applicator.rs` for event application. Implement `mod.rs` to expose `ImBackend`, `ImStringAssembler`, and `ImValueAssembler`, and satisfy the `PathCtx`, `EventCtx`, `OwnedEventCtx`, and `BuilderCtx` traits. Lean on insights from the spikes to document how `*_mut` conversions operate and why they are safe. Make sure each abstraction is defined in plain language so a newcomer knows what problem it solves.

Once the backend compiles, integrate it with the public adapters. Allow callers to instantiate `JsonModem::<ImBackend>`, construct `JsonModemBuffers::with_builder` using an `ImValueAssembler`, and configure `JsonModemValues::with_buffer_builder`. Extend (or refactor) the prototype `JsonModemIm` into either production code or a documented experiment demonstrating the feasibility of non-lending iteration. Update benchmark helpers in `crates/jsonmodem/benches/streaming_json_common.rs` and related benches to add IM variants, maintaining symmetry with the standard backend. Expand unit tests in `crates/jsonmodem/tests/jsonmodem_values/tests.rs` to cover chunked feeds, partial snapshots, serde_json comparisons, and any behaviours surfaced by the spikes. Confirm the fuzz crate still compiles; adjust features or tests if the new backend affects build flags.

Throughout the implementation, run `.agent/check.sh` after each major milestone to maintain a green workspace. When a test or lint fails, document the cause and fix in `Surprises & Discoveries` with a pointer to the resolving commit or file. After integrating adapters and tests, run the Criterion benches with `JSONMODEM_BENCH_FAST=1` to ensure IM variants execute without panics and to capture preliminary performance numbers.

Finally, update documentation and reflect the experimental learnings. `docs/im/im-execplan.md` must summarise the architecture, number-handling decisions, decode modes, and Send + Sync guarantees, calling out how the spike results informed the design. Add module-level comments that clarify non-obvious logic (for example, why we rebuild EcoString fragments in a certain order). Once acceptance criteria are met, populate `Outcomes & Retrospective` with benchmarks, test evidence, and a discussion of whether the `JsonModemIm` adapter will proceed to production or remain an experiment.

## Concrete Steps

    # 1. Run EcoString and rpds spikes.
    #    - Add crates/jsonmodem/tests/im_spikes.rs (cfg(test)) with focused tests that mutate EcoString, clone it, and confirm fragments remain valid.
    #    - Write companion tests for rpds::VectorSync/RedBlackTreeMapSync demonstrating push_back_mut, set_mut, insert_mut, and structural sharing.
    #    - Record behavioural notes in Surprises & Discoveries with file and line references.

    # 2. Prototype a JsonModemIm adapter.
    #    - Build crates/jsonmodem/tests/im_adapter.rs (feature-gated) to simulate feeding events into an immutable Value and expose a standard iterator returning &Value.
    #    - Validate iterator lifetimes by holding references across multiple feed calls; add assertions covering mutation and observation.
    #    - Summarise outcomes (viable path or blockers) in Surprises & Discoveries and log decisions if the design direction changes.

    # 3. Add dependencies and upstream references.
    #    - Edit crates/jsonmodem/Cargo.toml to include ecow and rpds.
    #    - Update Cargo.lock via cargo fetch or build.
    #    - Append tmp/upstream/ to .gitignore.
    #    - Populate tmp/upstream/ecow/ and tmp/upstream/rpds/ with READMEs or checked-out sources for offline inspection.
    #    - Draft docs/im/im-execplan.md with dependency rationale, spike findings, number handling, and safety notes.

    # 4. Implement backend modules under crates/jsonmodem/src/backend/im/.
    #    - value.rs: define Str, Array, Map aliases and the Value enum with Display/Debug helpers.
    #    - value_zipper.rs: implement stack frames for arrays and objects using rpds persistent containers; explain push/pop semantics via comments.
    #    - value_applicator.rs: translate ParseEvent streams into zipper operations, handling string fragments, numbers, booleans, nulls, arrays, and objects while leveraging EcoString behaviour validated in spikes.
    #    - mod.rs: expose ImBackend, ImStringAssembler, and ImValueAssembler; implement PathCtx/EventCtx/OwnedEventCtx/BuilderCtx; surface decode modes.

    # 5. Integrate adapters and public exports.
    #    - Update crates/jsonmodem/src/backend/mod.rs and crates/jsonmodem/src/lib.rs to re-export new types.
    #    - Ensure JsonModemBuffers and JsonModemValues accept ImValueAssembler without API changes; adjust generics only if required.
    #    - Keep JsonModemValues-based smoke tests green while introducing additional adapters.

    # 6. Expose the `im` feature and JsonModemIm API.
    #    - Implement jsonmodem_im.rs with a feature-gated JsonModemIm type returning stable snapshot iterators.
    #    - Re-export JsonModemIm (and helper iterators) when `im` is enabled; document the feature flag.
    #    - Update .agent/check.sh to pass `--features im` so local workflows exercise the adapter, while keeping the crate’s default feature set empty.
    #    - Replace the prototype test with coverage that targets the public feature-gated API.

    # 7. Expand benches and tests.
    #    - Modify crates/jsonmodem/benches/streaming_json_common.rs and related bench files to include IM variants (events, buffers, values).
    #    - Extend crates/jsonmodem/tests/jsonmodem_values/tests.rs with immutable backend scenarios: basic roundtrip, nested chunked feeds, partial snapshots, serde_json comparisons, quickcheck hooks as needed.
    #    - Reuse the property suites in crates/jsonmodem/src/tests/property_multivalue.rs and property_partition.rs so QuickCheck validates Std and IM backends with the same arbitrary data generators.
    #    - Confirm fuzz crate builds; add compile-time checks if necessary.

    # 8. Validation and documentation.
    #    - Run .agent/check.sh until clean; resolve fmt, clippy, test, or bench issues.
    #    - Execute JSONMODEM_BENCH_FAST=1 cargo bench --bench streaming_json_medium to confirm benches run with IM helpers (optional but recommended; record observations).
    #    - Finalise docs/im/im-execplan.md with findings, safety guarantees, number/string decisions, and future work notes.
    #    - Update this ExecPlan’s Progress, Surprises & Discoveries, Decision Log, and Outcomes & Retrospective.

## Validation and Acceptance

Success requires the following observable behaviours:

- Running `.agent/check.sh` completes without failures, demonstrating that formatting, clippy, tests, and fuzz crate compilation succeed with the immutable backend present.
- A user can execute the code snippet from `docs/im/prompt.md` (instantiating `JsonModem`, `JsonModemBuffers`, and `JsonModemValues` with `ImBackend`) in a smoke test and parse chunked JSON, yielding correctly formatted final values.
- Criterion benches under `crates/jsonmodem/benches/` compile and report both Std and IM variants without panicking when run with `JSONMODEM_BENCH_FAST=1`.
- Unit tests and spike suites (in `im_spikes.rs` and related prototypes) pass, demonstrating EcoString/rpds behaviour and validating the non-lending iterator experiment.
- The experimental `JsonModemIm` prototype either lands as production code with passing tests or is documented in the Decision Log with clear blockers and follow-up actions.
- `docs/im/im-execplan.md` exists and clearly documents design decisions, safety considerations, and future number/string extensibility, referencing insights from the spikes.

## Idempotence and Recovery

Adding dependencies to `Cargo.toml` is idempotent; rerunning `cargo fetch` or `.agent/check.sh` after edits is safe. Creating `tmp/upstream/*` directories can be repeated; if directories already exist, ensure content remains current or remove them before regenerating. Backend module implementations should be version-controlled; if an experiment fails, revert the affected files using `git checkout -- <paths>` to restore the last known-good state. Running benches with `JSONMODEM_BENCH_FAST=1` is non-destructive and can be repeated to confirm performance.

## Artifacts and Notes

Captured outputs:

    ./.agent/check.sh
        ✅ All stages succeeded (rustfmt, build, tests, clippy, docs); Miri skipped per environment flag.

    JSONMODEM_BENCH_FAST=1 cargo bench --bench streaming_json_medium
        streaming_json_medium/jsonmodem_values/1000      ~71.8 µs
        streaming_json_medium/jsonmodem_values_im/1000   ~160.9 µs
        streaming_json_medium/jsonmodem_buffers/5000     ~127 µs
        streaming_json_medium/jsonmodem_buffers_im/5000  ~349 µs

    cargo test --features im
        ✅ Validates that the feature-gated JsonModemIm adapter and its snapshots behave as expected.

These runs confirm the IM backend integrates with existing benches and highlight the current performance delta relative to the std backend.

## Interfaces and Dependencies

Implement or modify the following interfaces:

    crates/jsonmodem/src/backend/im/value.rs
        pub type Str = ecow::EcoString;
        pub type Array = rpds::VectorSync<Value>;
        pub type Map = rpds::RedBlackTreeMapSync<Str, Value>;
        pub enum Value { Null, Boolean(bool), Number(f64), String(Str), Array(Array), Object(Map) }
        Provide Display implementations for Value and helpers for constructing arrays/objects.

    crates/jsonmodem/src/backend/im/value_zipper.rs
        Define structs representing the current construction stack (arrays and objects).
        Implement push/pop/set operations using rpds persistent containers with minimal cloning.

    crates/jsonmodem/src/backend/im/value_applicator.rs
        Expose ValueApplicator with methods:
            pub fn new(options: BufferOptions) -> Self;
            pub fn on_event(&mut self, event: ParseEvent<...>) -> Result<AppliedRef<'_>, ParseFloatError>;
            pub fn read_root(&self) -> &Value;
            pub fn take_root(&mut self) -> Value;
        Ensure string fragments append correctly using EcoString.

    crates/jsonmodem/src/backend/im/mod.rs
        pub struct ImBackend { decode_mode: RustDecodeMode }
        impl Default, PathCtx, EventCtx, OwnedEventCtx, BuilderCtx.
        pub struct ImStringAssembler { scratch: Str, options: BufferOptions }
        pub struct ImValueAssembler { applicator: ValueApplicator }
        Include `RustDecodeMode::StrictUnicode` and `RustDecodeMode::ReplaceInvalid`.

    crates/jsonmodem/tests/im_spikes.rs (new, #[cfg(test)])
        Define focused tests that mutate EcoString and rpds containers, asserting structural sharing and safe mutation patterns.
        Document observations inline so contributors understand why these behaviours are relied upon in production code.

    crates/jsonmodem/src/jsonmodem_im.rs (new, feature = "im")
        Provide JsonModemIm and JsonModemImIter types that collect immutable snapshots via standard iterators.
        Offer constructors (`new`, `with_options`, `with_config`), feed/finish helpers, and snapshot accessors.

    crates/jsonmodem/tests/im_adapter.rs (integration test, required feature)
        Exercise JsonModemIm using partial snapshots, validating final output stability and iterator behaviour.

    crates/jsonmodem/src/backend/mod.rs and crates/jsonmodem/src/lib.rs
        Re-export ImBackend, ImStringAssembler, ImValueAssembler, and associated aliases so callers can opt in.

    crates/jsonmodem/tests/jsonmodem_values/tests.rs
        Add test cases covering ImValueAssembler behaviour, ensuring serde_json parity and partial snapshot correctness.

    docs/im/im-execplan.md
        Document design goals, dependency choices, safety guarantees, number/string policies, and future extensions.

    .agent/check.sh
        Ensure local validation enables the `im` feature via `--features im` while keeping crate defaults untouched.

All new or modified code must include succinct comments clarifying non-obvious logic, particularly around persistent container mutations and decode-mode handling, to aid future maintainers.

## Change Notes

2025-02-14 Codex: Replaced previous plan with a PLANS.md-compliant ExecPlan that guides contributors through recreating the immutable backend introduced between HEAD~1 and HEAD.
2025-02-14 Codex: Expanded the plan to mandate EcoString/rpds spikes, a JsonModemIm prototype, and documentation of experimental findings before full implementation.
2025-02-14 Codex: Added milestones for feature-gating `JsonModemIm`, updating `.agent/check.sh`, and promoting the immutable adapter to a public API behind `im`.
