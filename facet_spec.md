**Facet + JsonModem Execution Spec**

- Status: design proposal
- Target: jsonmodem-facet (new adapter on top of crates/jsonmodem)
- Author: coding agent
- Last updated: 2025-09-08

This document specifies how to use the facet reflection library to update Rust values in place while parsing JSON incrementally with JsonModem. It defines a new streaming adapter that emits typed reflection events and applies changes directly to an existing Rust value that derives facet reflection (`#[derive(facet)]`). This spec is self-contained: it includes a minimal “facet abstraction” API sufficient to implement the adapter without internet access, plus pseudocode and error semantics.

The goal is: given an existing `T: Facet` and a stream of JSON text, update `t: T` in place as data arrives, without building an intermediate JSON Value tree and without allocating more than necessary. The adapter mirrors `JsonModemBuffers`/`JsonModemValues` but targets facet’s reflection (peek/poke/partial) APIs.


**Background**

- JsonModem
  - The core parser (`crates/jsonmodem/src/parser`) produces `ParseEvent<'src, P, Ctx>`, where `P` describes a path (by default `StdPath = Vec<PathItem>`), and `Ctx` defines number/string factories and error type. See `EventCtx`, `PathCtx`, and `BuilderCtx` in `crates/jsonmodem/src/context.rs` and the buffered adapters in `crates/jsonmodem/src/jsonmodem_buffers.rs` and `crates/jsonmodem/src/jsonmodem_values.rs`.
  - The “std” backend (`crates/jsonmodem/src/backend/std`) defines `StdBackend`, `StdPath`, an in-memory JSON `Value`, and an applicator/zipper to maintain a live Value tree.

- facet reflection (high level)
  - `#[derive(facet)]` synthesizes runtime reflection for struct/enum/tuple fields and exposes read (“peek”) and write (“poke”) access to nested fields using names or indices. For this spec, we do not require any specific external crate; we define a small “facet abstraction” below that can be implemented in terms of a real facet library later.


**Scope and Goals**

- Update an existing instance `&mut T` (root) that implements facet reflection while streaming JSON.
- Support partial updates for long strings and for nested arrays/objects as events arrive, emitting reflection events that carry both the current path (JsonModem path) and a reflection handle to the updated target.
- Maintain JsonModem’s separation of concerns: the parser stays allocation-light and string-fragment oriented; the new adapter is responsible for buffering policy and in-place application to the typed value.
- Preserve the iterator/lending-iterator ergonomics present in `JsonModemBuffers`/`JsonModemValues`.

Non-goals (initially):
- Arbitrary user-defined numeric coercions beyond the common Rust scalar set.
- Full generality of serde’s arbitrary transient borrow lifetimes; we focus on streaming text inputs.
- Lossless arbitrary-precision numbers.


**Public API Overview**

- New adapter (high level names; exact module placement under `crates/jsonmodem` TBD):

  - `JsonModemFacet<Ctx = StdBackend, A = FacetAssembler>`
    - Similar to `JsonModemValues`, constructed from `ParserOptions` and facet-specific `FacetOptions` (buffering/coercion knobs).
    - Holds the underlying `JsonModem<Ctx>` and a `FacetAssembler` that mutates the user-provided root and produces typed reflection events.

  - `FacetOptions` (non-exhaustive):
    - `partial_strings: bool` — whether to emit string prefix updates (append) for `String`/`Vec<u8>` fields versus waiting until `is_final`.
    - `allow_coerce_numbers: bool` — permit lossy/coercive number writes (e.g., float→int via truncation) when configured.
    - `unknown_field: UnknownFieldPolicy` — `Ignore | Error | InsertDynamic` (for map-like/dynamic fields).
    - `collection_growth: CollectionGrowthPolicy` — controls `Vec`/map resize behavior when indices/keys are sparse.

  - `FacetEvent<'a, P, Root>`
    - Iterator item emitted by the adapter. Each event ties a JsonModem path to a reflection view into the already-updated target inside `Root`.
    - Layout:
      - `path: &'a P` — borrowed path (e.g., `&'a StdPath`).
      - `view: facet_api::ValueRef<'a>` — read-only reflection handle to the updated slot (erased type with vtable; callers can try downcast or visit). For string fragments the value is the destination slot (`String`, `Vec<u8>`, etc.).
      - `kind: FacetEventKind` — detailed classification to mirror JSON shape and string streaming state.
      - `root: &'a Root` — borrow to the overall root; helpful when callers need a root-scoped computation.
    - `FacetEventKind`:
      - `Scalar { ty: TypeIdLike }`
      - `StringFragment { is_initial: bool, is_final: bool }`
      - `SeqBegin`
      - `SeqEnd { root_completed: bool }`
      - `MapBegin`
      - `MapEnd { root_completed: bool }`

  - `JsonModemFacet::feed(&mut self, chunk: &str) -> JsonModemFacetIter<'_, Ctx, A, Root>`
    - Lending iterator over `Result<FacetEvent<'_, &Ctx::Path, Root>, FacetError<Ctx>>`.

  - `JsonModemFacet::finish(self) -> JsonModemFacetClosed<Ctx, A, Root>`
    - Drain remainder after end-of-input (same contract as buffers/values adapters).

  - `FacetError<Ctx>`
    - `Parser(crate::parser::ParserError<Ctx>)`
    - `Assembler(FacetApplyError)` — structured type mismatch/coercion error with the JsonModem path and facet path/shape details.

  - Convenience constructors:
    - `JsonModemFacet::new_in_place(root: &mut Root, options: ParserOptions, facet: FacetOptions) -> Self`
    - `JsonModemFacet::with_assembler(root: &mut Root, options: ParserOptions, facet: FacetOptions, assembler: FacetAssembler<'_>) -> Self`


**Design Rationale**

- Event shape mirrors `BufferedEvent`/`AppliedRef` but carries a reflection view (`ValueRef<'a>`) into the user’s `Root` instead of a `jsonmodem::Value` snapshot. Updates happen eagerly; the event is an observation point for clients (e.g., UI diffs, hooks, tracing).
- `ValueRef<'a>` is an erased typed view from facet that can be dynamically downcast or visited generically; using it avoids baking generic parameters for every possible field type into the event. This keeps the event type stable and object-safe to handle heterogeneity inside structs/enums.
- We avoid returning `&'a mut` to inner fields in events to prevent borrow conflicts with future updates; the assembler uses a “poke then drop mut, then peek” pattern per event to respect Rust’s aliasing rules while still giving read-only observation to callers.


**Adapter Architecture**

- `FacetAssembler<'root, Root>`
  - Owns a root pointer to `Root` and a “facet zipper” that mirrors the approach in `backend/std/value_zipper.rs` but for typed reflection nodes.
  - On each parser `ParseEvent`, aligns the zipper to the incoming JsonModem path and applies changes via facet’s poke/partial APIs.
  - Returns a `FacetEvent` that references (peek) the updated slot.

- `FacetZipper`
  - Maintains a stack of reflection node pointers corresponding to the current JsonModem path; analogous to `ValueZipper`’s `path_nodes` + `path_components` but with pointers to facet-reflect nodes instead of `Value` enum slots.
  - Supports:
    - `descend_key(&mut self, key: &str)` → navigate struct field by name (or map entry), creating/initializing slots as needed if policy allows.
    - `descend_index(&mut self, idx: usize)` → navigate vector/array/tuple field by index; grow if policy allows.
    - `ascend()` when parser emits `ArrayEnd`/`ObjectEnd`.
  - Each operation returns a typed write target (poke) to mutate, which is then immediately converted to a read-only view (peek) for the event payload.

- Mapping from JSON to facet targets
  - Objects →
    - Structs: field names match JSON keys; support `#[facet(rename = "...")]` if configured on the field. Unknown fields: obey `UnknownFieldPolicy`.
    - Maps: use key conversions (`&str`→`K`) per facet numeric/string key support; insert-or-update map entry.
  - Arrays →
    - `Vec<T>`/slices: ensure capacity/resize on demand; write each element as it arrives.
    - Fixed-size arrays/tuples: enforce bounds; treat out-of-bounds indices per policy (`Error | Ignore`).
  - Scalars →
    - `bool`, numbers, strings/bytes: write directly; `partial_strings` controls whether string fragments append to `String`.
    - `Option<T>`: create `Some(T::default())` on first event if the slot is `None`; assign `None` when JSON `null` is seen.
    - Enums: support `externally tagged` and `adjacently tagged` patterns if configured; initial implementation may limit to struct-like variants.


**Event Semantics**

For a stream of `ParseEvent`s, the adapter produces a 1:1 stream of `FacetEvent`s carrying the updated slot and shape markers, respecting `ValuesOptions`-like toggles for partial emission.

- String fragments
  - For a target `String`, `Vec<u8>`, or a user field annotated to accept fragments, the adapter appends fragments and emits:
    - `FacetEventKind::StringFragment { is_initial, is_final }` with `view` pointing to the `String`/`Vec<u8>` inside `Root`.
  - If `partial_strings == false`, fragments are buffered in a small scratch (assembler-owned) and committed on `is_final = true`, emitting a single `Scalar` event.

- Arrays/objects
  - Emit `SeqBegin`/`MapBegin` when a container is created or confirmed at a path. Emit `SeqEnd`/`MapEnd` when popping the container. The `root_completed` flag mirrors `AppliedRef::{ArrayEnd,ObjectEnd}` behavior in `StdValueAssembler`.

- Scalars
  - On `Null`/`Boolean`/`Number`, write into the slot, then emit `Scalar` with the updated `view`.

Partial root snapshots
- Like `JsonModemValues`, when the current root is not complete but `partial_strings` or nested updates have occurred, the adapter may coalesce and emit a final `Scalar` or a container `End` with `root_completed = false` if the caller opts into partial reporting. (Exact thresholds mirror `jsonmodem_values.rs`’s `EmitKind` logic.)


**Detailed Execution Flow**

Below, `P` is `&'a Ctx::Path` (normally `&StdPath`), `Root` is the user type with `#[derive(facet)]`.

1) Parser chunk → events
   - `JsonModemFacet::feed(&mut self, chunk)` calls into `JsonModem<Ctx>::feed`, producing a `JsonModemIterator` over `ParseEvent<'src, &'a P, Ctx>`.
2) Assembler dispatch
   - For each `ParseEvent`, call `FacetAssembler::on_event(event) -> Result<FacetEvent<'a, P, Root>, FacetError<Ctx>>`.
3) Path alignment
   - Maintain `FacetZipper` depth equal to the parser path depth. Differences are reconciled by pushing/popping one component at a time (asserting depth changes by ≤ 1, same pattern as `ValueZipper::align_path`).
   - `Key` component: locate struct/map entry by name; create entry if allowed (for `Map`).
   - `Index` component: ensure vector capacity and set current slot index.
4) Application per event
   - Null → set slot to `None`/unit/zero-value per type; if type cannot accept `null`, emit `Assembler(TypeMismatch)`.
   - Boolean/Number → parse/coerce into the target scalar; validate per options.
   - String → if fragment: append or buffer; if final: commit. For new slot that is not a `String`, attempt conversion (e.g., to `PathBuf`/`OsString` if widening is supported/configured); otherwise error.
   - Container begin → ensure slot is a container (struct/map/vec) and emit `Begin`. Container end → emit `End { root_completed }`.
5) Event construction
   - After each write, drop mutable access; immediately obtain a read-only reflection `ValueRef<'_>` to the same slot and construct `FacetEvent { path, view, kind, root }`.
6) Iterator lifetime
   - As with `JsonModemBuffersIter`, the event borrows all data only for the duration of one `LendingIterator::next` call.


**Core Types (Sketch)**

```rust
pub struct JsonModemFacet<'root, Root, Ctx = StdBackend, A = FacetAssembler<'root, Root>>
where
    Ctx: BuilderCtx + EventCtx + Default,
{
    modem: crate::parser::JsonModem<Ctx>,
    assembler: A,
    options: FacetOptions,
    // same multiple-values behavior as other adapters
}

pub struct FacetAssembler<'root, Root: facet_api::Facet> {
    root: &'root mut Root,
    zipper: FacetZipper,
    scratch: String, // optional for string fragments when partial emission disabled
}

pub enum FacetEventKind {
    Scalar { ty_name: &'static str },
    StringFragment { is_initial: bool, is_final: bool },
    SeqBegin,
    SeqEnd { root_completed: bool },
    MapBegin,
    MapEnd { root_completed: bool },
}

pub struct FacetEvent<'a, P, Root> {
    pub path: &'a P,
    pub view: facet_api::ValueRef<'a>,
    pub kind: FacetEventKind,
    pub root: &'a Root,
}

pub enum FacetError<Ctx: EventCtx> {
    Parser(crate::parser::ParserError<Ctx>),
    Assembler(FacetApplyError),
}

pub struct FacetApplyError {
    pub path: crate::path::Path,
    pub expected: &'static str,
    pub found: &'static str,
    pub detail: alloc::string::String,
}
```


**FacetZipper (Sketch)**

The zipper maintains a vector of typed node pointers synchronized with the current `StdPath` depth much like `ValueZipper` (`crates/jsonmodem/src/backend/std/value_zipper.rs`). Operations return a write target via facet’s poke API, which is consumed before constructing a read-only `ValueRef` for emitting events.

```rust
struct FacetZipper {
    // parallel stacks
    nodes: alloc::vec::Vec<facet_api::ErasedPtr>,
    items: alloc::vec::Vec<crate::path::PathItem>,
}

impl FacetZipper {
    fn with_slot_mut<'a, F, R>(&'a mut self, root: *mut u8, path: &crate::path::Path, f: F) -> R
    where
        F: FnOnce(facet_api::PokeTarget<'a>) -> R,
    { /* align, descend, yield PokeTarget */ }
}
```


**Serde Integration**

We support “update during serde” in two complementary ways:

1) Deserializer bridge on top of JsonModemFacet
   - Implement a custom `serde::Deserializer<'de>` that sources tokens from `JsonModemFacet`. It translates `ParseEvent`/`FacetEvent` into `Deserializer` calls, allowing `T: Deserialize` to be populated as usual. For `deserialize_in_place`, pass a seed that wraps `&mut T` and uses facet poke internally, enabling true in-place updates.
   - This path is the most compatible with existing serde-based code but will not expose facet events directly to user code.

2) Seed + Visitor for in-place updates
  - Provide `FacetUpdateSeed<'a, T>` which implements `DeserializeSeed` and `Visitor` to write into an existing `&'a mut T` via facet poke. `serde_json` (or any serde data source) can drive this seed. For our streaming case, the deserializer is backed by JsonModem; the seed writes field-by-field using facet-derived shape.

Serde bridge skeleton:

```rust
#[cfg(feature = "serde")]
mod serde_bridge {
    use super::*;
    use serde::de::{self, DeserializeSeed, Visitor, MapAccess, SeqAccess};

    pub struct FacetDeserializer<'a, Ctx, A, Root> {
        pub facet: &'a mut JsonModemFacet<'a, Root, Ctx, A>,
    }

    impl<'de, 'a, Ctx, A, Root> serde::Deserializer<'de> for FacetDeserializer<'a, Ctx, A, Root>
    where
        Ctx: BuilderCtx + EventCtx + Default,
        Root: facet_api::Facet,
    {
        type Error = FacetSerdeError;

        fn deserialize_any<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            // Implement by pulling the next FacetEvent and forwarding to a suitable
            // deserialize_* based on FacetEventKind; iterate containers by
            // implementing MapAccess/SeqAccess wrappers over the stream.
            unimplemented!()
        }

        // Implement all typed `deserialize_*` shims similarly…
    }

    pub struct FacetUpdateSeed<'a, T> { pub target: &'a mut T }

    impl<'de, 'a, T> DeserializeSeed<'de> for FacetUpdateSeed<'a, T>
    where
        T: facet_api::Facet,
    {
        type Value = ();
        fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            // Drive a Visitor that writes into self.target using facet_api PokeTarget
            // obtained by traversing incoming keys/indices.
            struct V<'a, T>(&'a mut T);
            impl<'de, 'a, T: facet_api::Facet> Visitor<'de> for V<'a, T> {
                type Value = ();
                fn expecting(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
                    f.write_str("JSON object/array compatible with target")
                }
                fn visit_map<M>(self, mut access: M) -> Result<(), M::Error>
                where M: MapAccess<'de> {
                    while let Some((k, ())) = access.next_entry_seed(KeySeed, UnitSeed)? {
                        // For each key: descend and delegate to seeds for values.
                    }
                    Ok(())
                }
                fn visit_seq<S>(self, mut seq: S) -> Result<(), S::Error>
                where S: SeqAccess<'de> {
                    let mut idx = 0;
                    while seq.next_element_seed(ElemSeed(idx))?.is_some() { idx += 1; }
                    Ok(())
                }
            }
            deserializer.deserialize_any(V(self.target))
        }
    }

    #[derive(Debug)]
    pub struct FacetSerdeError(String);
    impl serde::de::Error for FacetSerdeError {
        fn custom<T: core::fmt::Display>(msg: T) -> Self { Self(msg.to_string()) }
    }
    impl core::fmt::Display for FacetSerdeError { fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { f.write_str(&self.0) } }
}
```

Recommended implementation order: start with the adapter and events; then build the deserializer bridge that reuses the same assembler logic, so both streaming and serde-driven entry points share code paths.


**Type Handling and Coercion Rules**

- Null → Option/Unit
  - `Option<T>`: set to `None`.
  - For non-optional non-nullable types, `Null` is an error unless a default/`Partial` placeholder is configured.

- Booleans
  - Accept only for `bool` targets.

- Numbers
  - Default policy: exact type match (e.g., JSON integer to `i{N}`; float to `f32/f64`). If `allow_coerce_numbers = true`, permit float→int by truncation and int widening; reject overflow.

- Strings/Bytes
  - `String` appends fragments when `partial_strings = true`; otherwise buffered until final.
  - For `PathBuf`, `OsString`, or user types with `From<String>`, allow conversion on final string.

- Containers
  - Arrays/Vec: grow to `idx + 1` (mirrors `ValueZipper::descend_one`), filling with default `T::default()` or `None` as appropriate.
  - Objects/Structs: resolve field by name; respect any facet-provided rename metadata.


**Error Handling**

- On type mismatch:
  - Emit `FacetError::Assembler(FacetApplyError { path, expected, found, detail })`.
  - Continue parsing if recovery is possible (e.g., ignore unknown field while inside a map); otherwise enter error state and stop the stream.

- On numeric overflow/underflow or invalid UTF-8 (depending on `StdBackend.decode_mode`): propagate the `Parser` or `Assembler` error with full path context.


**Performance Considerations**

- No intermediate `jsonmodem::Value` tree is built.
- For `String` when `partial_strings = true`, no extra copies: we append directly into the destination field.
- Path traversal mirrors `ValueZipper::align_path` with O(1) growth/shrink per depth step.
- `ValueRef` is a lightweight view; we obtain it only after we’ve released the mutable write guard to maintain sound borrows.


**Safety and Lifetimes**

- Each `FacetEvent` is valid only for the duration of a single `LendingIterator::next` call (same contract as buffered events in `jsonmodem_buffers.rs`). Implementor notes:
  - Apply (poke) with `&mut` to the slot.
  - Drop the `&mut` before acquiring the `ValueRef<'_>` (peek) to avoid aliasing.
  - The `root: &'a Root` borrow in `FacetEvent` must be obtained without violating alias rules (e.g., via shared reference reborrow of the assembler’s `&'root mut Root` after mutation).


**Compatibility and Feature Flags**

- Optional features:
  - `serde` — Enable the deserializer bridge and `DeserializeSeed` helpers.
  - `coerce_numbers` — Widening/truncation rules behind a feature or a runtime option bit.
  - `bytes` — Recognize `Vec<u8>`/`Bytes` fields for raw string fragments.


**Test Strategy**

- Golden tests mirroring `tests/snapshots_buffers_simple.rs` and `tests/snapshots_values.rs`, but asserting event streams and in-place mutations to a `#[derive(facet)]` test type.
- Property tests comparing serde_json(`T`) parsing against JsonModemFacet-driven updates from equivalent JSON text (when options forbid coercions and partials).
- Fuzz: reuse existing fuzz harnesses by swapping adapters and a small corpus of facet-derived types.


**Worked Example**

```rust
use facet_reflect::Facet; // proc-macro provides the derive

#[derive(Default, Facet)]
struct Config {
    title: String,
    flags: Vec<bool>,
    retries: Option<u32>,
}

let mut cfg = Config::default();
let mut modem = JsonModemFacet::<_, FacetAssembler>::new_in_place(
    &mut cfg,
    ParserOptions::default(),
    FacetOptions { partial_strings: true, ..Default::default() },
);

for ev in modem.feed("{\"title\":\"He\"" ) { /* partial append to title */ }
for ev in modem.feed("\"llo\", \"flags\":[true,false], \"retries\": 3}") { /* updates */ }

assert_eq!(&cfg.title, "Hello");
assert_eq!(cfg.flags, vec![true, false]);
assert_eq!(cfg.retries, Some(3));
```


**Mapping to Existing Code (Where to Fit)**

- New files (proposed):
  - `crates/jsonmodem/src/jsonmodem_facet.rs` — the adapter (public API).
  - `crates/jsonmodem/src/backend/facet/mod.rs` — module glue.
  - `crates/jsonmodem/src/backend/facet/api.rs` — minimal facet abstraction defined above.
  - `crates/jsonmodem/src/backend/facet/zipper.rs` — typed zipper.
  - `crates/jsonmodem/src/backend/facet/assembler.rs` — event applicator.
  - `crates/jsonmodem/src/backend/facet/tests.rs` — unit/integration tests with small facet types.

- Reuse existing traits from `context.rs` for path/number/string factories and from `jsonmodem_buffers.rs` for `PathRoot` etc.


**Open Questions / Future Work**

- Enum coverage: finalize tagging strategies and how to stream-update an enum variant switch mid-stream.
- Borrowed fields: safe support for fields that borrow from the input (e.g., `&'de str`) using `StdBackend.decode_mode` and lifetime threading.
- Zero-copy string fragments into `Vec<u8>` or arena-backed string types.
- Pluggable key de/normalization (case-insensitive matching, kebab↔snake mapping) informed by facet metadata.
- Error recovery policy customization (skip unknown field vs. hard error) per-struct via facet attributes.

This spec intentionally mirrors terminology from the existing `StdValueAssembler`/`ValueZipper` design to keep the mental model consistent.


**Minimal Facet Abstraction API (to implement once, use everywhere)**

This section defines a small trait surface the adapter requires. Implement it either by wrapping the real facet crate (recommended) or by a local mock for tests. Place it at `crates/jsonmodem/src/backend/facet/api.rs` as `mod facet_api`.

```rust
pub mod facet_api {
    use alloc::string::String;
    use alloc::vec::Vec;

    // Erased handle to a location inside a reflected value.
    pub struct ErasedPtr(*mut u8);

    // Read-only view of a reflected value. Provides downcasts and visitors.
    pub struct ValueRef<'a> { /* opaque; must outlive event */ }

    // Writable view (one-shot) to a location; cannot be cloned. After write, drop to release.
    pub struct PokeTarget<'a> { /* opaque */ }

    pub trait Facet {
        // Return an erased pointer to self suitable for zipper root.
        fn erased_ptr(&mut self) -> ErasedPtr;
    }

    pub enum ContainerKind { Seq, Map, Struct, Tuple }

    pub trait PokeScalar {
        fn write_null(self) -> Result<(), &'static str>;
        fn write_bool(self, v: bool) -> Result<(), &'static str>;
        fn write_i64(self, v: i64) -> Result<(), &'static str>;
        fn write_u64(self, v: u64) -> Result<(), &'static str>;
        fn write_f64(self, v: f64) -> Result<(), &'static str>;
        fn write_str_final(self, v: &str) -> Result<(), &'static str>;
        fn string_push_str(&mut self, fragment: &str) -> Result<(), &'static str>;
    }

    pub trait PokeContainer {
        // Ensure container exists and return a handle to descend.
        fn ensure_container(self, kind: ContainerKind) -> Result<(), &'static str>;
    }

    pub trait Descend {
        // Navigate by key; create slots for Map/Struct as needed. Returns a new PokeTarget.
        fn key(self, name: &str) -> Result<PokeTarget<'_>, &'static str>;
        // Navigate by index; grow Vec/Array as needed. Returns a new PokeTarget.
        fn index(self, idx: usize) -> Result<PokeTarget<'_>, &'static str>;
        // Peek a read-only view after writing.
        fn to_view(self) -> ValueRef<'_>;
    }

    // Implementations may provide blanket impls so that PokeTarget implements
    // PokeScalar + PokeContainer + Descend.
}
```

The adapter only depends on:
- obtaining an `ErasedPtr` for the root,
- descending by key/index, creating/growing containers,
- writing scalars and string fragments, and
- reading a `ValueRef<'_>` view after mutation.


**Algorithmic Pseudocode**

Implementation of `FacetAssembler::on_event` in terms of the minimal facet API:

```rust
fn on_event<'a>(&'a mut self,
    event: ParseEvent<'_, &'a StdPath, StdBackend>,
) -> Result<FacetEvent<'a, &'a StdPath, Root>, FacetError<StdBackend>> {
    use crate::event::ParseEvent as E;
    match event {
        E::ArrayBegin { path } => {
            let mut slot = self.zipper.align(&mut self.root, path)?; // -> facet_api::PokeTarget
            slot.ensure_container(facet_api::ContainerKind::Seq)
                .map_err(err_apply(path))?;
            let view = slot.to_view();
            Ok(FacetEvent { path, view, kind: FacetEventKind::SeqBegin, root: &*self.root })
        }
        E::ArrayEnd { path } => {
            let (_slot, view, root_completed) = self.zipper.finish_container(path)?;
            Ok(FacetEvent { path, view, kind: FacetEventKind::SeqEnd { root_completed }, root: &*self.root })
        }
        E::ObjectBegin { path } => {
            let mut slot = self.zipper.align(&mut self.root, path)?;
            slot.ensure_container(facet_api::ContainerKind::Map)
                .map_err(err_apply(path))?;
            let view = slot.to_view();
            Ok(FacetEvent { path, view, kind: FacetEventKind::MapBegin, root: &*self.root })
        }
        E::ObjectEnd { path } => {
            let (_slot, view, root_completed) = self.zipper.finish_container(path)?;
            Ok(FacetEvent { path, view, kind: FacetEventKind::MapEnd { root_completed }, root: &*self.root })
        }
        E::Null { path } => {
            let mut slot = self.zipper.align(&mut self.root, path)?;
            slot.write_null().map_err(err_apply(path))?;
            let view = slot.to_view();
            Ok(FacetEvent { path, view, kind: FacetEventKind::Scalar { ty_name: "null" }, root: &*self.root })
        }
        E::Boolean { path, value } => {
            let mut slot = self.zipper.align(&mut self.root, path)?;
            slot.write_bool(value).map_err(err_apply(path))?;
            let view = slot.to_view();
            Ok(FacetEvent { path, view, kind: FacetEventKind::Scalar { ty_name: "bool" }, root: &*self.root })
        }
        E::Number { path, value } => {
            let mut slot = self.zipper.align(&mut self.root, path)?;
            // Convert according to FacetOptions
            coerce_number(&mut slot, value, self.options).map_err(err_apply(path))?;
            let view = slot.to_view();
            Ok(FacetEvent { path, view, kind: FacetEventKind::Scalar { ty_name: "number" }, root: &*self.root })
        }
        E::String { path, fragment, is_initial, is_final } => {
            let mut slot = self.zipper.align(&mut self.root, path)?;
            if self.options.partial_strings {
                if is_initial { /* no special action; appends will start */ }
                slot.string_push_str(fragment.as_ref()).map_err(err_apply(path))?;
                let view = slot.to_view();
                Ok(FacetEvent { path, view, kind: FacetEventKind::StringFragment { is_initial, is_final }, root: &*self.root })
            } else {
                if is_final {
                    slot.write_str_final(fragment.as_ref()).map_err(err_apply(path))?;
                    let view = slot.to_view();
                    Ok(FacetEvent { path, view, kind: FacetEventKind::Scalar { ty_name: "string" }, root: &*self.root })
                } else {
                    // Buffer locally until final, emit no event.
                    self.scratch.push_str(fragment.as_ref());
                    Ok(FacetEvent { path, view: self.zipper.peek(path)?, kind: FacetEventKind::StringFragment { is_initial, is_final }, root: &*self.root })
                }
            }
        }
    }
}
```

`FacetZipper::align` maintains a stack of `(ErasedPtr, PathItem)` entries; when depth increases, it descends via `key`/`index`, when decreases, it pops. It returns a `PokeTarget` for the current leaf slot.


**Type Conversion Table**

- JSON → Rust target:
  - null → Option::None; otherwise error unless `NullDefault` policy is set.
  - bool → bool only.
  - number → i8/i16/i32/i64/u{N}/f32/f64 per exact match; if `allow_coerce_numbers`, allow:
    - integer widening (u8→u64, i8→i64, etc.), error on overflow.
    - float→int by truncation toward zero; error on NaN/Inf.
    - int→float exact cast (within f64 range).
  - string → String/OsString/PathBuf via `write_str_final`.


**Error Codes**

- `EUNKFIELD` — unknown struct field and policy=Error.
- `ETYPE` — scalar type mismatch (e.g., number to bool).
- `EOVERFLOW` — numeric overflow/underflow.
- `EBOUNDS` — array/tuple index out of bounds and policy=Error.
- `ECONTAINER` — expected container, found scalar (or vice versa).
- Each carries `path: crate::path::Path` and optional detail text.


**Finish/Multiple Roots Semantics**

- `ParserOptions::with_allow_multiple_json_values(true)` should be used by the adapter (mirroring `JsonModemValues`) so that streams like `1 2 [3]` are accepted.
- A root is “completed” when:
  - we observe a scalar at the empty path, or
  - we observe `ArrayEnd`/`ObjectEnd` at the empty path.
- The adapter increments `next_index` and may emit a final `SeqEnd`/`MapEnd` event with `root_completed = true`.


**Implementation Checklist**

- [ ] Introduce `backend/facet/api.rs` with the minimal facet API and a test/mock implementation.
- [ ] Implement `backend/facet/zipper.rs` mirroring `backend/std/value_zipper.rs` for typed slots.
- [ ] Implement `backend/facet/assembler.rs` with `on_event` matching the pseudocode.
- [ ] Add `jsonmodem_facet.rs` adapter that wraps `JsonModem` and yields `FacetEvent` via a lending iterator (follow `jsonmodem_values.rs` structure and emit rules).
- [ ] Add `FacetOptions` and error types.
- [ ] Tests: unit + integration as outlined in Test Strategy.


**Facet‑Deserialize Alignment Addendum**

This adapter can be implemented directly on top of a `Partial<'shape>` builder using the same operations and invariants facet-deserialize uses. The minimal operations needed (names and roles mirror facet-deserialize’s public use):

- Frame control
  - `Partial::frame_count() -> usize` — for asserts when unwinding to root before finalizing.
  - `Partial::end() -> Result<(), ReflectError>` — close the current frame (field/item/container).
  - `Partial::peek() -> Peek` — read-only snapshot view for copying defaults.
  - `Partial::shape() -> Shape` — describes the active frame’s shape; used to branch on Def/Type.

- Structs and fields
  - `Partial::begin_nth_field(index) -> Result<(), ReflectError>` — enter field by numeric index.
  - `Partial::is_field_set(index) -> Result<bool, ReflectError>` — whether a field was assigned.
  - `Partial::set_field_default(fnptr)` — initialize current field with field-level default.
  - `Partial::set_from_peek(&FieldPeek)` — copy value from a default instance (type-level default).

- Containers
  - Lists: `Partial::begin_list()`, `Partial::begin_list_item()`.
  - Arrays: `Partial::begin_nth_element(idx)` with bounds checking against array length from `shape.def`.
  - Enums (tuple variants): `Partial::begin_nth_enum_field(idx)`; track `field_count` and `current_field` to enforce element counts.

- Scalars and defaults
  - `Partial::set_default()` — set the current slot to its type default when available.

Container close post-processing
- On `ObjectEnd`, for every unset field: apply field default function if present, else copy from a type-default instance via `set_from_peek`, else error (policy dependent).

Arrays vs Lists vs Tuples
- Arrays (fixed length) use `begin_nth_element(idx)`; indices tracked on a stack; close does not require calling `begin_list()`.
- Lists (`Vec<T>`) use `begin_list()` and `begin_list_item()` per element.
- Tuples and enum tuple variants: treat incoming `[...]` as fields by index; use `begin_nth_field(i)` / `begin_nth_enum_field(i)`; enforce element counts.


**Builder Mode (Partial) Addendum**

Although the primary goal is in-place updates via Poke, a fully functional adapter can first be delivered using `Partial<'shape>` with these steps:

- On each `ParseEvent`, align a shadow path (like the zipper) and call the appropriate `Partial::*` begin method.
- Apply scalars and string fragments per policy.
- On container end, run the default-filling pass for missing fields.
- For root completion, call `HeapValue::materialize()` (or `Partial.build()` if using a typed wrapper) and swap into `&mut Root`. Events in this mode use `peek()` to build `FacetEvent.view`.

This mode matches facet-msgpack/facet-deserialize’s flow and is implementable offline with the API shapes above.


**Numeric Conversion Semantics**

Adopt facet-deserialize’s conversions:
- Integer→integer: fallible, overflow-checked cast.
- Integer→float: allowed (within target domain).
- Float→integer: only if finite, within range, and with no fractional part.
- Float→float: allowed; precision loss on f64→f32 acceptable.
Gate with `allow_coerce_numbers` (if false, require exact target type match).


**Enum Variants and Tuple Fields**

- When a variant is externally tagged (e.g., `{ "V": { ... } }`), select `V` on `ObjectBegin` upon first key and emit `EnumVariantSelected { name, index }`.
- For tuple variants, track `(field_count, current_field)`; each array element maps to `begin_nth_enum_field(current_field)`, then increment. Enforce exact lengths unless `EnumTupleLenPolicy` relaxes it.


**Options and Policies (Extended)**

- `MissingFieldPolicy` (default: Error): `Error | UseFieldDefault | UseTypeDefault`.
- `EnumTupleLenPolicy` (default: Exact): `Exact | AllowShortFillDefaults | ErrorOnExtra`.
- `UnknownFieldPolicy` (already specified): if `Ignore`, skip value using PathCtx to fast-forward until matching close.


**Smart Pointers, Option, and Wrappers**

- `Option<T>`: `null` sets `None`; non-null opens/initializes `Some(T)` and delegates.
- Smart pointers (Box/Arc/Rc): create pointee frame, deserialize into it, then wrap on close.
- Newtype wrappers: transparently delegate to inner shape and wrap on close.
