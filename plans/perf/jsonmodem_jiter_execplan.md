# Match JsonModemValues Single-Chunk Performance With Jiter

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must be kept up to date as work proceeds.

Refer to `PLANS.md` in the repository root for the governing standards. This document is authored and maintained in accordance with those requirements.

## Purpose / Big Picture

JsonModem today excels at zero-copy streaming but still lags behind the Jiter crate for single-chunk parsing of medium and large JSON payloads. After implementing this plan, a contributor can run the provided Criterion benchmarks and observe JsonModemValues matching or exceeding Jiter’s throughput when the entire document arrives in one chunk. This narrows a real-world gap for workloads that parse complete payloads from memory while keeping JsonModem’s zero-copy guarantees.

## Progress

- [x] (2025-10-12 00:38Z) Drafted initial ExecPlan and captured current repository context.
- [x] (2025-10-12 00:39Z) Acquired and studied the Jiter crate within a local, ignored checkout (completed: cloned into `third_party/jiter` and summarized parser optimizations).
- [x] (2025-10-12 00:43Z) Established single-chunk benchmarks comparing JsonModem variants against Jiter (added `single_chunk_json_large` bench, helpers, and ensured it builds).
- [x] (2025-10-12 01:02Z) Analyzed Jiter internals and prototyped JsonModemValues optimizations (completed: implemented ASCII fast path for string scanning; next: tackle container assembly and number parsing gaps).
- [ ] (2025-10-12 01:12Z) Optimize container assembly to shrink the remaining gap (completed: baseline profiling; deferred while we focus on event-parser parity; resume after core gap narrows).
- [x] (2025-10-12 01:20Z) Tightened numeric parsing and refreshed parity validation benchmarks (completed: added digit fast-path, re-ran single-chunk suite, captured JsonModem vs Jiter deltas).
- [ ] (2025-10-12 01:25Z) Profile JsonModem core parser vs Jiter and prototype lexer-level optimizations (completed: captured perf for jsonmodem_events; remaining: identify and implement reductions in `Scanner::consume_string_ascii_fast` / `JsonModem::lex_state_step`).

## Surprises & Discoveries

- Observation: Jiter’s string decoder keeps an ASCII fast path with a lookup table and optional SIMD chunk scanning, only falling back to tape-based copying on escapes, which minimizes per-character branching.  
  Evidence: third_party/jiter/crates/jiter/src/string_decoder.rs:200.
- Observation: Jiter avoids recursive descent by managing arrays/objects with a reusable `SmallVec` stack and preallocated `Vec` buffers, reducing heap churn on deep or wide JSON.  
  Evidence: third_party/jiter/crates/jiter/src/value.rs:334.
- Observation: Jiter parses numbers via a staged pipeline that classifies digits with a static bitmap and defers to `lexical_parse_float` only when needed, handling infinities/NaNs behind explicit gates.  
  Evidence: third_party/jiter/crates/jiter/src/number_decoder.rs:1.
- Observation: Baseline single-chunk benches show `JsonModemValues` taking ~184 µs for `response_large` versus ~12 µs for `jiter_value`, confirming a ~15× performance gap to close.  
  Evidence: JSONMODEM_BENCH_FAST=1 JSONMODEM_BENCH_COMPARISON=1 cargo bench --bench single_chunk_json_large (2025-10-12).
- Observation: Adding an ASCII fast-path for string scanning cut `jsonmodem_values/response_large` to ~44 µs (≈4× faster) while leaving semantics unchanged.  
  Evidence: JSONMODEM_BENCH_FAST=1 JSONMODEM_BENCH_COMPARISON=1 cargo bench --bench single_chunk_json_large (2025-10-12 01:02Z).
- Observation: Profiling shows container assembly now dominates single-chunk runtime (~40% of samples) due to Vec reallocations.
  Evidence: perf record + report targeting jsonmodem_values/response_large (2025-10-12 01:12Z).
- Observation: Adding a digit fast-path trims numeric scanning overhead by ~6% but overall runtime remains ~48 µs, so lexing still dominates.
  Evidence: cargo bench --bench single_chunk_json_large -- jsonmodem_values/response_large (2025-10-12 01:20Z).
- Observation: JsonModem core events benchmark sits around 39 µs for `response_large`, still ~3× slower than `jiter_value` despite bypassing container assembly; perf points to `Scanner::consume_string_ascii_fast` and `JsonModem::lex_state_step` as top consumers.
  Evidence: JSONMODEM_BENCH_FAST=1 JSONMODEM_BENCH_COMPARISON=1 cargo bench --bench single_chunk_json_large jsonmodem_events/response_large + perf record (2025-10-12 01:25Z).
## Decision Log

- Decision: Host the Jiter source under `third_party/jiter` so researchers can inspect it without polluting version control.  
  Rationale: Keeps the upstream code accessible for comparison while respecting licensing and avoiding accidental commits.  
  Date/Author: 2025-10-12 / Codex agent.
- Decision: Introduce a scanner ASCII fast-path using `memchr` to mirror Jiter’s branchless string scan while preserving zero-copy semantics.  
  Rationale: String tokenization dominated CPU time for single-chunk parses; scanning contiguous ASCII in batches removes repeated peek/consume overhead without changing emitted fragments.  
  Date/Author: 2025-10-12 / Codex agent.
- Decision: Profile-guided focus shifts to container builders; plan to prototype pre-sized Vec growth patterned after Jiter’s SmallVec staging.
  Rationale: With string scanning optimized, arrays/objects are now the largest hotspot; staging capacity should reduce reallocs without breaking zero-copy semantics.
  Date/Author: 2025-10-12 01:12Z / Codex agent.
- Decision: Extend scanner numeric handling with a batch fast-path while keeping property-name escapes zero-copy via explicit prefix promotion.
  Rationale: Reduces per-digit overhead yet preserves existing semantics for borrowed keys.
  Date/Author: 2025-10-12 01:20Z / Codex agent.
- Decision: Shift immediate focus to the core event parser gap versus Jiter before returning to container assembly work.
  Rationale: Even without building values, JsonModem remains ~3× slower; addressing lex/scan hotspots will benefit all adapters.
  Date/Author: 2025-10-12 01:25Z / Codex agent.

## Outcomes & Retrospective

- Pending.

## Context and Orientation

The workspace contains the `crates/jsonmodem` crate implementing the streaming parser. Benchmarks live under `crates/jsonmodem/benches/`. The existing `streaming_json_large.rs` benchmark measures chunked streaming throughput using helper routines in `streaming_json_common.rs`, which in turn calls `JsonModem`, `JsonModemBuffers`, and `JsonModemValues`. Criterion drives these benches; enabling the `JSONMODEM_BENCH_COMPARISON` environment variable adds external comparisons including Jiter.

The new work focuses on JsonModem’s `JsonModemValues` adapter defined in `crates/jsonmodem/src/jsonmodem_values.rs`. This layer consumes buffered streaming events and yields root values with minimal copying. Its hot path is the `next_value_for_source` helper that classifies buffered events before emitting a root snapshot. Any optimization must preserve the adapter’s zero-copy semantics, meaning we cannot introduce owned clones or detach from the existing buffer lifetimes.

The Jiter crate (repository `https://github.com/pydantic/jiter/`) is an optimized JSON iterator written in Rust and C that is widely cited for its speed. Understanding how Jiter manages buffer traversal, SIMD usage, and allocation patterns will inform JsonModem improvements.

## Plan of Work

First, clone Jiter into an ignored directory (`third_party/jiter`) and document its layout. Build its benches and review the core parsing pipeline to catalogue the optimizations it employs (SIMD scanning, branchless classification, arena allocation, etc.). Summaries from this research will live in this plan’s `Surprises & Discoveries` section and feed the eventual code changes.

Next, create a new Criterion benchmark (e.g., `crates/jsonmodem/benches/single_chunk_json_large.rs`) derived from `streaming_json_large.rs` but operating on a single chunk. Reuse the existing data files (`benches/jiter_data/response_large.json`, etc.) and add helper functions in `streaming_json_common.rs` to run one-shot parsing for JsonModem, JsonModemBuffers, JsonModemValues, and Jiter. Extend the benchmark group to measure medium and large payloads, and ensure the bench honours the `JSONMODEM_BENCH_FAST` flag to keep local runs quick.

With the baseline in place, run the new benchmark and capture relative performance. Populate the plan with observations describing JsonModemValues’ gap versus Jiter. This data will ground the optimization work.

Study Jiter’s source to understand its speed tricks. Identify at least two concrete hypotheses to try within JsonModemValues without breaking zero-copy semantics. Examples could include caching root path checks, reducing iterator indirection, or special-casing the single-chunk finish path. Implement quick spikes behind feature flags or helper functions, measure with the new benchmark, and keep notes in `Surprises & Discoveries`. Any spike that regresses correctness or fails to improve speed should be reverted immediately.

After selecting the winning techniques, integrate them cleanly into `JsonModemValues`, adding targeted unit tests or benches if needed. Update documentation comments to explain the rationale, especially if new invariants are introduced. Keep `.agent/check.sh` passing throughout by running it after meaningful changes.

Finally, update README or performance documentation with the new benchmark guidance, summarize the outcome in this plan’s `Outcomes & Retrospective`, and ensure the working tree reflects the benchmark parity goal. Prepare the repo for handoff with clear instructions on running the benches.

## Concrete Steps

    # 1. Clone Jiter for local study.
    cd /var/mnt/pool/friel/c/github.com/aaronfriel/jsonmodem-perf
    mkdir -p third_party
    git clone https://github.com/pydantic/jiter.git third_party/jiter

    # 2. Add /third_party/jiter to .gitignore to avoid accidental commits.

    # 3. Review Jiter’s parser implementation and record findings here.

    # 4. Copy the streaming benchmark scaffold to a new single-chunk benchmark file.
    #    Extend streaming_json_common.rs with single-chunk helpers.

    # 5. Run targeted benches and record numbers.
    JSONMODEM_BENCH_FAST=1 cargo bench --bench single_chunk_json_large

    # 6. Iterate on JsonModemValues optimizations, running `.agent/check.sh` frequently.
    ./.agent/check.sh

    # 7. Once JsonModemValues matches Jiter in the single-chunk bench, update docs and rerun checks.

## Validation and Acceptance

Acceptance requires Criterion results showing `jsonmodem_values` within 5% of `jiter` for the medium and large single-chunk cases using the new benchmark with `JSONMODEM_BENCH_COMPARISON=1`. Run `.agent/check.sh` and expect all stages to pass. Demonstrate that existing streaming benchmarks remain stable by running `JSONMODEM_BENCH_FAST=1 cargo bench --bench streaming_json_large` and observing no regressions in event counts or panics.

## Idempotence and Recovery

Cloning Jiter into `third_party/jiter` is idempotent as long as you remove the directory before recloning; otherwise, run `git pull` inside. Benchmark runs and `.agent/check.sh` are safe to repeat. If an optimization spike fails, revert the touched files using `git checkout -- <paths>` before continuing; avoid history rewriting.

## Artifacts and Notes

Populate this section with key benchmark outputs and diffs as work proceeds. Capture before-and-after Criterion summaries and any profiling notes relevant to the final solution.

    JSONMODEM_BENCH_FAST=1 JSONMODEM_BENCH_COMPARISON=1 cargo bench --bench single_chunk_json_large
        jsonmodem_values/response_large ≈ 184 µs
        jiter_value/response_large ≈ 12 µs
        jiter_value_owned/response_large ≈ 17.7 µs
    JSONMODEM_BENCH_FAST=1 JSONMODEM_BENCH_COMPARISON=1 cargo bench --bench single_chunk_json_large
        jsonmodem_events/response_large ≈ 35 µs
        jsonmodem_values/response_large ≈ 44 µs
        jiter_value/response_large ≈ 14 µs
    JSONMODEM_BENCH_COMPARISON=1 cargo bench --bench single_chunk_json_large jsonmodem_values/response_large
        jsonmodem_values/response_large ≈ 48 µs (digit fast path active)
    JSONMODEM_BENCH_FAST=1 JSONMODEM_BENCH_COMPARISON=1 cargo bench --bench single_chunk_json_large jsonmodem_events/response_large
        jsonmodem_events/response_large ≈ 39 µs
        jiter_value/response_large ≈ 11.6 µs

## Interfaces and Dependencies

Expect to touch the following:

    crates/jsonmodem/src/jsonmodem_values.rs
        Optimize the emission path while preserving the public API. Potentially add internal helpers but keep `JsonModemValues`’s external behaviour unchanged.

    crates/jsonmodem/benches/streaming_json_common.rs
        Add single-chunk helpers (e.g., `run_jsonmodem_values_single`) and shared utilities for new benchmarks.
    Cargo dependency: memchr (no_std)
        Provides `memchr2` for branchless ASCII scanning in the parser fast-path.


    crates/jsonmodem/benches/single_chunk_json_large.rs (new)
        Define Criterion benchmarks covering JsonModem variants and Jiter for single-chunk payloads.

    .gitignore
        Ignore `third_party/jiter`.

    docs/perf/jsonmodem_jiter_execplan.md (this file)
        Keep `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` updated as milestones are completed.

Any new helper functions must be documented with inline comments explaining the performance motivation so future contributors understand why they exist.
