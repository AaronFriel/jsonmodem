# Production Integration Record

## 2026-06-08 Worktree Created

Command:

```bash
git worktree add -b codex/customer-incremental-json-production ../jsonmodem-customer-production origin/main
```

Observation: the branch starts from `47a542760f84dd402cecda6476b56dc92dae54e5`, the merged Python API simplification commit.

Consequence: production work can proceed without modifying the dirty `feat/facet-streaming-api-reboot` worktree.

## 2026-06-08 Phase 1 Ported

Changed files in `/home/friel/c/aaronfriel/jsonmodem-customer-production`:

- `crates/jsonmodem-py/src/lib.rs`
- `crates/jsonmodem-py/tests/test_events_simple.py`
- `crates/jsonmodem-py/python/jsonmodem/__init__.pyi`
- `crates/jsonmodem-py/README.md`
- `crates/jsonmodem-py/benchmarks/bench_jiter_chunked.py`

Observation: the production branch now exposes `JsonModem(..., string_events="per_feed")` and `feed_many(chunks)`. In per-feed mode, scalar `feed(chunk)` rejects iterables, `feed_many(chunks)` rejects scalar inputs, and selected string fragments are compacted in Rust before Python event tuples, path views, or `StringPayload` objects are built. Tests cover multi-chunk compaction, per-call boundaries, escaped Unicode progress with no decoded output, repeated same-path strings, eager iterable consumption, and temporary rejection of `byte_views=True` with per-feed mode until phase 5 defines the ownership contract.

Validation:

```bash
cargo check -p jsonmodem-py
.agent/setup-py.sh
.agent/check-py.sh
```

Result: `cargo check -p jsonmodem-py` passed. `.agent/check-py.sh` passed with `36` Python tests; existing pdoc `__hash__` stub warnings remain.

Consequence: phase 1 is now implemented and locally validated on the production branch. Phase 5 must revisit the temporary `byte_views=True` rejection for per-feed mode.

## 2026-06-08 Phase 2 Cumulative Prefix Guidance

Changed files in `/home/friel/c/aaronfriel/jsonmodem-customer-production`:

- `crates/jsonmodem-py/src/lib.rs`
- `crates/jsonmodem-py/python/jsonmodem/__init__.py`
- `crates/jsonmodem-py/python/jsonmodem/__init__.pyi`
- `crates/jsonmodem-py/README.md`

Observation: production no longer exposes `JsonModemPrefixes` or `PrefixResult`. Direct delta input remains documented as preferred. If a caller receives full accumulated prefixes, the caller owns append-only validation and should feed only newly appended bytes to `JsonModem`.

Validation:

```bash
cargo check -p jsonmodem-py
.agent/check-py.sh
```

Result: `cargo check -p jsonmodem-py` passed after removing the public prefix API.

Consequence: phase 2 is implemented as documentation and benchmark guidance, not as a shipped class. Benchmark group C should use caller-side trusted and validated prefix adapters.

## 2026-06-08 Phase 3 Live Values

Changed files in `/home/friel/c/aaronfriel/jsonmodem-customer-production`:

- `crates/jsonmodem-py/src/lib.rs`
- `crates/jsonmodem-py/python/jsonmodem/__init__.pyi`
- `crates/jsonmodem-py/README.md`
- `crates/jsonmodem-py/tests/test_values.py`

Observation: `JsonModemValues` now has `update(chunk_or_chunks, changed_paths=False)` for the no-notification path. It returns the reused read-only root `JsonModemValueView` and creates no per-mutation Python update tuples unless `changed_paths=True` is requested. `changed_paths=True` returns a summary dict with `view` and `changed_paths`. No-argument `finish()` keeps the existing update iterator behavior, while `finish(changed_paths=False/True)` validates end-of-input and returns the reused view or a summary. `reset()` preserves the same view object and returns it to the empty state. Duplicate keys are reflected in changed-path order, while the view uses last-write-wins object semantics.

Validation:

```bash
cargo check -p jsonmodem-py
.agent/check-py.sh
```

Result: `cargo check -p jsonmodem-py` passed. `.agent/check-py.sh` passed with `42` Python tests.

Consequence: phase 3 is implemented on the production branch. Benchmark group D should compare per-mutation `feed()`, no-notification `update()`, changed-path summaries, and snapshots separately.

## 2026-06-08 Phase 4 Completed Subtrees

Changed files in `/home/friel/c/aaronfriel/jsonmodem-customer-production`:

- `crates/jsonmodem-py/src/lib.rs`
- `crates/jsonmodem-py/python/jsonmodem/__init__.py`
- `crates/jsonmodem-py/python/jsonmodem/__init__.pyi`
- `crates/jsonmodem-py/README.md`
- `crates/jsonmodem-py/tests/test_subtrees.py`

Observation: production now exposes `JsonModemCompletedSubtrees(paths=..., release_after_emit=True)`. It emits `(path, value, released)` only when selected object or array subtrees close. Emitted values are ordinary owned Python values and remain valid after parser-owned storage is released. Tests cover document-order output, no emission for a truncated final item, multiple roots with `ParserOptions(allow_multiple=True)`, scalar versus iterable input shapes, reset behavior, and retained-state accounting.

Important limitation: `release_after_emit=True` removes emitted object fields and replaces emitted array entries with `CoreValue::Null`. `retained_state()` reports `released_array_entries_retained_as_null`, so memory behavior is measurable, but this implementation does not prove constant parser-retained memory for large arrays.

Validation:

```bash
cargo check -p jsonmodem-py
.agent/check-py.sh
```

Result: `cargo check -p jsonmodem-py` passed. `.agent/check-py.sh` passed with `47` Python tests.

Consequence: phase 4 is production-safe for owned completed-subtree emission and truthful memory reporting. A deeper value-store design is still needed before claiming completed array entries leave no parser-retained placeholder state.

## 2026-06-08 Phase 5 Byte Payload Modes

Changed files in `/home/friel/c/aaronfriel/jsonmodem-customer-production`:

- `crates/jsonmodem-py/src/lib.rs`
- `crates/jsonmodem-py/python/jsonmodem/__init__.pyi`
- `crates/jsonmodem-py/README.md`
- `crates/jsonmodem-py/tests/test_events_simple.py`

Observation: byte-view payloads now include explicit `payload_kind` and `ownership` labels. Borrowed unescaped input spans are labeled `payload_kind == "raw_source"` and `ownership == "borrowed"` and are returned as `memoryview`. Escaped strings and owned fallbacks are labeled `payload_kind == "decoded_text"` and `ownership == "owned"`. `string_events="per_feed", byte_views=True` is now accepted and returns owned decoded text for compacted output; it does not claim a borrowed payload for escaped or cross-buffer data. Documentation explains that retaining a small borrowed `memoryview` pins the backing source object.

Validation:

```bash
cargo check -p jsonmodem-py
.agent/check-py.sh
```

Result: `cargo check -p jsonmodem-py` passed. `.agent/check-py.sh` passed with `47` Python tests.

Consequence: phase 5 is implemented with truthful ownership labels and documented lifetime tradeoffs. Benchmark group F and memory helper should measure borrowed-view retention separately from owned decoded output.

## 2026-06-08 Final Production Validation

Changed production files:

- `crates/jsonmodem-py/src/lib.rs`
- `crates/jsonmodem-py/python/jsonmodem/__init__.py`
- `crates/jsonmodem-py/python/jsonmodem/__init__.pyi`
- `crates/jsonmodem-py/README.md`
- `crates/jsonmodem-py/tests/test_events_simple.py`
- `crates/jsonmodem-py/tests/test_values.py`
- `crates/jsonmodem-py/tests/test_subtrees.py`
- `crates/jsonmodem-py/benchmarks/bench_customer_requirements.py`
- `crates/jsonmodem-py/benchmarks/measure_customer_requirements_memory.py`
- `crates/jsonmodem-py/benchmarks/bench_jiter_chunked.py`

Additional production fix after benchmark smoke: byte inputs can now end in an
incomplete UTF-8 character. `JsonModem`, `JsonModemValues`, and
`JsonModemCompletedSubtrees` carry those trailing bytes into the next byte or
memoryview input. `finish()` raises `TypeError` if the stream ends with
incomplete UTF-8. Byte-view payloads reconstructed across that boundary are
owned decoded text, not borrowed memoryviews.

Artifacts:

- `target/customer-incremental-json/production-representative-ak-no-prefix-api-smoke.json`: A-K representative smoke after removing the public prefix API, `78` rows, all `ok`.
- `target/customer-incremental-json/production-prefix-no-public-api-smoke.json`: group C smoke after removing the public prefix API, `8` rows, all `ok`.
- `target/customer-incremental-json/production-representative-ak-fast-comparable.pyperf.json`: comparable fast pyperf artifact from before public prefix API removal, `77` rows, `pyperf check` exit `0` with expected fast-mode warnings. Rows shared with the current API remain useful; the removed prefix-specific rows should not be cited as current production API evidence.
- `target/customer-incremental-json/production-memory-smoke.json`: memory helper output, `16` rows, all `ok`.

Validation:

```bash
cargo check -p jsonmodem-py
.agent/check-py.sh
PATH="$HOME/.local/bin:$PATH" .agent/check.sh
.venv/bin/python -m py_compile crates/jsonmodem-py/benchmarks/bench_customer_requirements.py crates/jsonmodem-py/benchmarks/measure_customer_requirements_memory.py
git diff --check
```

Result: all passed. `.agent/check-py.sh` passed with `54` Python tests. `.agent/check.sh` passed with Miri skipped by `AGENT_CHECK_MIRI_DISABLE=true`.

Conclusion: production implementation satisfies phases 1 through 5 with the
documented phase 4 retained-array-placeholder caveat, and the public prefix
API has been removed in favor of caller-side append validation plus normal
`JsonModem.feed()` / `JsonModem.feed_many()` calls.
