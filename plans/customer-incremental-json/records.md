# Customer Incremental JSON Records

This file records shared observations and benchmark evidence for `plans/customer-incremental-json/plans.md`.

## 2026-06-08 Initial Requirements Read

Source: `/home/friel/.codex/attachments/9350a2dd-7d2f-4e5f-a138-69206aae3c7f/pasted-text.txt`.

Observation: the customer requirements make phase 1, selected string extraction with per-feed compaction, the mandatory first implementation. The key behavior is not merely accepting an iterable of chunks. JsonModem must compact selected string fragments in native code before creating Python event tuples, path objects, payload objects, or decoded Python strings.

Consequence: all implementation branches must prove phase 1 before claiming progress on phases 2 through 5.

## 2026-06-08 Cumulative Prefix Stance

Observation: use case 2 describes sources that repeatedly publish the full accumulated prefix. The requirements themselves state direct delta input is preferred and that validation cost must be reported separately when append-only input cannot be trusted.

Consequence: the root recommendation starts skeptical. The default public API should encourage callers to provide new bytes only. Documentation should show how callers can compute deltas before calling `JsonModem`. Do not export a production cumulative-prefix parser class.

## 2026-06-08 Worktree Creation

Command, from `/home/friel/c/aaronfriel/jsonmodem`:

    git worktree add -b customer/per-feed-compaction ../jsonmodem-customer-compaction origin/main
    git worktree add -b customer/feed-result-api ../jsonmodem-customer-feedresult origin/main
    git worktree add -b customer/adapter-stack ../jsonmodem-customer-adapters origin/main
    git worktree add -b customer/requirements-benchmarks ../jsonmodem-customer-benchmarks origin/main

Observation: all four worktrees were created at `47a542760f84dd402cecda6476b56dc92dae54e5`, the merged PR #72 commit.

Consequence: implementation work can proceed without touching the dirty `feat/facet-streaming-api-reboot` worktree.

## 2026-06-08 Parallel Assignments

Worktrees and agents:

- Russell (`019ea5ca-36a1-7810-9481-36d386abb461`) owns `/home/friel/c/aaronfriel/jsonmodem-customer-compaction`.
- Hypatia (`019ea5ca-3962-7ef3-a3d0-b1e777769dad`) owns `/home/friel/c/aaronfriel/jsonmodem-customer-feedresult`.
- Socrates (`019ea5ca-3d8e-70b0-b2cd-e1538d4d1395`) owns `/home/friel/c/aaronfriel/jsonmodem-customer-adapters`.
- Ampere (`019ea5ca-443e-7d71-b647-e98ea8793041`) owns `/home/friel/c/aaronfriel/jsonmodem-customer-benchmarks`.

Observation: each worker has a separate worktree and a branch-local `plan.md` plus `record.md`. The root agent keeps the shared recommendation record and will review worker results before choosing an implementation direction.

Consequence: implementation branches can diverge through phase 5 prototypes while benchmark evidence is developed in parallel.

## 2026-06-08 Baseline API Observation

Worktree: `/home/friel/c/aaronfriel/jsonmodem-customer-benchmarks`

Source: `crates/jsonmodem-py/src/lib.rs`, current merged `origin/main`.

Observation: `JsonModem.feed()` currently accepts either a scalar JSON input or an iterable of scalar inputs. The iterable path is eager and returns one Python iterator after processing all inputs, but normal event collection still calls `view_event_record()` for each matching parser event. That means it can reduce call overhead but does not satisfy the customer requirement to compact selected string fragments before path objects, payload objects, decoded Python strings, or event tuples are created.

Consequence: an implementation that only renames iterable `feed()` to `feed_many()` is not enough. Phase 1 must move selected-string aggregation ahead of Python event conversion.

## 2026-06-08 Benchmark Requirement Observation

Source: customer requirements document, Benchmark A through Benchmark K.

Observation: the most urgent benchmark groups are A, B, H, I, and K because they validate phase 1: long selected strings, many short selected strings, no-match filtering, Python feed overhead, and caller-owned batching. Cumulative-prefix, live-value, completed-subtree, byte-payload, error, and memory-scaling groups are still needed for phase 2 through phase 5, but they should not decide the phase 1 API before selected-string evidence exists.

Consequence: final recommendations should report phase 1 timing and correctness first, then describe later-phase prototypes as separate API choices with their own evidence.

## 2026-06-08 FeedResult Prototype Complete

Worktree: `/home/friel/c/aaronfriel/jsonmodem-customer-feedresult`

Agent: Hypatia (`019ea5ca-3962-7ef3-a3d0-b1e777769dad`).

Changed files recorded by the worker:

- `crates/jsonmodem-py/src/lib.rs`
- `crates/jsonmodem-py/python/jsonmodem/__init__.py`
- `crates/jsonmodem-py/python/jsonmodem/__init__.pyi`
- `crates/jsonmodem-py/README.md`
- `crates/jsonmodem-py/tests/test_feed_result.py`
- `crates/jsonmodem-py/benchmarks/bench_feed_result_api.py`
- `plans/customer-incremental-json/completed/2026-06-09-feed-result-api/plan.md`
- branch-local `record.md` in `/home/friel/c/aaronfriel/jsonmodem-customer-feedresult`

Observation: this branch adds `FeedResult`, `JsonModemFeed`, and `JsonModemPrefixes`. `JsonModemFeed.feed()` accepts one scalar input, `feed_many()` accepts an iterable and consumes it eagerly, and compacted selected string events are stored as native feed records before Python event tuples, paths, and payloads are built. `FeedResult` also exposes prototype fields for `changed_paths`, `completed_subtrees`, `view`, error state, prefix status, and bytes consumed.

Review finding addressed: the initial compactor emitted an empty non-final string event when a feed operation opened a selected string but produced no decoded output. The branch now tracks whether decoded output was produced and emits only when there is decoded output or the lexical string is final. Tests cover both suppression of the empty non-final event and preservation of the empty final delta after a prior decoded feed.

Validation reported by the worker:

- `cargo check -p jsonmodem-py`: passed.
- `crates/jsonmodem-py/tests/test_feed_result.py`: `9 passed`.
- `.agent/check-py.sh`: passed, `36 passed`; existing pdoc `__hash__` stub warnings were observed.
- `PATH="$HOME/.local/bin:$PATH" .agent/check.sh`: passed, with Miri skipped using `AGENT_CHECK_MIRI_DISABLE=true`.
- `git diff --check`: passed.
- `crates/jsonmodem-py/benchmarks/bench_feed_result_api.py --fast ... --output target/customer-incremental-json/feed-result-smoke.json`: completed and `pyperf check` passed.

Consequence: FeedResult is a strong API candidate for explicit grouped input, error state, prefix status, and per-feed summaries. It is not complete for phase 5 because it currently emits owned decoded text payloads rather than truthful byte-view ownership and lifetime information.

## 2026-06-08 Minimal JsonModem Compaction Prototype Complete

Worktree: `/home/friel/c/aaronfriel/jsonmodem-customer-compaction`

Agent: Russell (`019ea5ca-36a1-7810-9481-36d386abb461`).

Changed files recorded by the worker:

- `crates/jsonmodem-py/src/lib.rs`
- `crates/jsonmodem-py/tests/test_events_simple.py`
- `crates/jsonmodem-py/python/jsonmodem/__init__.pyi`
- `crates/jsonmodem-py/README.md`
- `crates/jsonmodem-py/benchmarks/bench_jiter_chunked.py`
- `plans/customer-incremental-json/completed/2026-06-09-per-feed-compaction/plan.md`
- branch-local `record.md` in `/home/friel/c/aaronfriel/jsonmodem-customer-compaction`

Observation: this branch adds `JsonModem(..., string_events="per_feed")` and explicit eager `feed_many(chunks)`. Fragment mode remains default. In per-feed mode, `feed()` accepts one scalar input and rejects iterables; `feed_many()` accepts an iterable and rejects scalar inputs. Selected string compaction happens in the native binding before Python event tuples, path views, and `StringPayload` objects are built.

Review finding addressed: the initial pending string flush emitted an empty non-final event when no decoded output was produced. The branch now tracks decoded output and preserves `is_initial=True` for the first later visible event after suppressed no-output progress. Test coverage includes split escape progress: `feed(b'{"content":"\\u00')` emits no event, then the closing feed emits the decoded final event.

Validation reported by the worker:

- `cargo check -p jsonmodem-py`: passed.
- `.agent/check-py.sh`: passed with `36` Python tests after the review fix.
- `PATH="$HOME/.local/bin:$PATH" .agent/check.sh`: passed, with Miri skipped using `AGENT_CHECK_MIRI_DISABLE=true`.
- Benchmark smoke on `response_large.json`, 64-byte chunks, 10 chunks per feed: scalar selected fragments about `190 us`, grouped fragment mode about `108 us`, grouped per-feed mode about `108 us`.

Consequence: this is the simplest phase 1 user-facing API and should be seriously considered if recommendation-quality benchmarks show lower Python object creation and no default-mode regression. It does not cover phases 2 through 5 beyond documentation and explicit rejection of `byte_views=True` with per-feed mode.

## 2026-06-08 Customer Benchmark Suite Complete

Worktree: `/home/friel/c/aaronfriel/jsonmodem-customer-benchmarks`

Agent: Ampere (`019ea5ca-443e-7d71-b647-e98ea8793041`).

Changed files recorded by the worker:

- `crates/jsonmodem-py/benchmarks/bench_customer_requirements.py`
- `crates/jsonmodem-py/benchmarks/measure_customer_requirements_memory.py`
- `plans/customer-incremental-json/completed/2026-06-09-requirements-benchmarks/plan.md`
- branch-local `record.md` in `/home/friel/c/aaronfriel/jsonmodem-customer-benchmarks`

Artifacts:

- `target/customer-incremental-json/customer-ab-hik-smoke.json`: 32 smoke rows, all `ok`.
- `target/customer-incremental-json/customer-ab-hik-fast.pyperf.json`: 32 pyperf benchmarks across A, B, H, I, and K.
- `target/customer-incremental-json/customer-memory-smoke.json`: memory helper validation.

Validation reported by the worker:

- `.venv/bin/python -m py_compile` for both benchmark scripts: passed.
- `.venv/bin/python -m pyperf check target/customer-incremental-json/customer-ab-hik-fast.pyperf.json`: exited `0`, with expected fast-mode stability warnings.
- `.agent/check-py.sh`: passed with `27` pytest tests and docs generated.
- `PATH="$HOME/.local/bin:$PATH" .agent/check.sh`: passed, with Miri skipped using `AGENT_CHECK_MIRI_DISABLE=true`.

Notable current-main observations from the fast artifact:

- A, 64 KiB ASCII selected string with 8-byte input chunks: repeated `feed(chunk)` about `36.8 ms`, grouped iterable `feed(list)` about `32.0 ms`, caller-joined groups about `4.85 ms`, and `jiter` cumulative partial prefixes about `108 ms`.
- B, ten short strings: repeated feed about `300 us`, grouped iterable feed about `281 us`, caller-joined groups about `135 us`, and `jiter` cumulative partial prefixes about `245 us`.
- H, no-match filtering: selected no-match path about `1.90 ms`, unfiltered event stream about `55.8 ms`.
- K, caller-owned batching: larger batches reduce parser CPU from about `30-31 ms` to about `21-23 ms` for this trace.

Observation: the branch also records a current-main correctness weakness: the mixed Unicode fixture can be rejected when bytes chunks split a UTF-8 code point. That fixture remains in the suite because the customer requirements explicitly call out split UTF-8 behavior.

Consequence: the benchmark suite is sufficient for first-pass A/B/H/I/K comparisons and has a command template for running implementation worktrees. It is not a full final performance report until it runs against the implementation branches and covers C, D, E, F, G, and J with stable pyperf settings and memory comparisons.

## 2026-06-08 Adapter Stack Prototype Complete

Worktree: `/home/friel/c/aaronfriel/jsonmodem-customer-adapters`

Agent: Socrates (`019ea5ca-3d8e-70b0-b2cd-e1538d4d1395`).

Changed files recorded by the worker:

- `crates/jsonmodem/src/selected_strings.rs`
- `crates/jsonmodem/src/lib.rs`
- `crates/jsonmodem-py/src/lib.rs`
- `crates/jsonmodem-py/python/jsonmodem/__init__.py`
- `crates/jsonmodem-py/python/jsonmodem/__init__.pyi`
- `crates/jsonmodem-py/tests/test_selected_strings_adapter.py`
- `plans/customer-incremental-json/completed/2026-06-09-adapter-stack/plan.md`
- branch-local `record.md` in `/home/friel/c/aaronfriel/jsonmodem-customer-adapters`

Observation: this branch adds a native `JsonModemSelectedStrings` adapter with selected-path matching and per-feed string compaction before Python object creation. It also exposes Python prototypes for `JsonModemSelectedStrings`, `JsonModemLiveValuesNoNotify`, and `JsonModemCompletedSubtrees`.

Phase coverage reported by the worker:

- Phase 1: working native adapter plus Python wrapper and tests.
- Phase 2: explicit cumulative-prefix adapter with validation byte accounting and mismatch reporting.
- Phase 3: no-notification live value update prototype returning a reused read-only view.
- Phase 4: completed subtree prototype that emits selected subtrees; true constant-memory release still requires a deeper value-store change.
- Phase 5: conservative payload labels for owned decoded text only; no borrowed-output claim.

Validation reported by the worker:

- `cargo test -p jsonmodem selected_strings`: passed with `5` tests.
- `cargo test -p jsonmodem-py`: passed.
- `.agent/check-py.sh`: passed with `34` Python tests.
- `PATH="$HOME/.local/bin:$PATH" .agent/check.sh`: passed.

Consequence: the adapter branch is the strongest implementation evidence because it proves the desired native ordering directly. Its prototype classes should not become the permanent public API; the implementation should be folded into the simpler `JsonModem(..., string_events="per_feed")` interface.

## 2026-06-08 Cross-Worktree Selected-String Benchmark Comparison

Benchmark script updated in `/home/friel/c/aaronfriel/jsonmodem-customer-benchmarks` to detect optional prototype APIs:

- `JsonModem(..., string_events="per_feed")` on the minimal branch.
- `JsonModemFeed` on the FeedResult branch.
- `JsonModemSelectedStrings` on the adapter branch.

Commands used the implementation worktree `.venv/bin/python` with the benchmark script from the benchmark worktree. Fast pyperf artifacts:

- `target/customer-incremental-json/compaction-selected-fast.pyperf.json`
- `target/customer-incremental-json/feedresult-selected-fast.pyperf.json`
- `target/customer-incremental-json/adapters-selected-fast.pyperf.json`

Validation:

- `py_compile` on the benchmark scripts passed after adding prototype hooks.
- `pyperf check` exited `0` for all three artifacts, with expected fast-mode stability warnings.

Selected means from the fast artifacts:

| Workload | Baseline grouped fragment | Minimal per-feed | FeedResult | Native adapter | jiter cumulative prefixes |
| --- | ---: | ---: | ---: | ---: | ---: |
| A 64 KiB selected string, 8-byte input chunks, 10 chunks/feed | 31.99-32.97 ms | 7.05 ms | 7.03 ms | 6.52 ms | 108.45 ms |
| B 10 selected strings, 16-byte chunks, 4 chunks/feed | 277-282 us | 154.7 us | 159.0 us | 152.9 us | 244.8 us |

Observation: all three native compaction designs are about 4.5x to 5x faster than grouped fragment output for the long selected-string case, and about 1.75x to 1.85x faster for the short-string case. The native adapter is slightly fastest in these fast runs. Full-value `orjson` and `jiter` one-shot decode remain much faster but answer a different question and are not equivalent to selected streaming output.

Consequence: the performance case for native per-feed compaction is real. The next production implementation should use the adapter branch's native compaction approach behind the minimal branch's `JsonModem(..., string_events="per_feed")` and `feed_many(chunks)` public API.

## 2026-06-08 Final Recommendation

Adopt one public event-stream API:

```python
parser = JsonModem(paths=["message", "items.*.content"], string_events="per_feed")
for kind, path, payload in parser.feed_many(chunks):
    ...
```

Keep `feed(chunk)` scalar and `feed_many(chunks)` iterable. Keep fragment mode as the default. Do not ship permanent public prototype classes such as `JsonModemSelectedStrings` or `JsonModemFeed` unless a later API review finds a specific need. The customer wanted ergonomic streaming input, and the minimal API is the least surprising shape.

Use the adapter branch as the implementation source. Its native `JsonModemSelectedStrings` work best proves the requirement: selected path matching and per-feed string compaction occur before Python tuple, path, payload, and decoded string object creation. Fold that code into the normal Python binding rather than layering compaction over already-created Python events.

Keep cumulative-prefix support out of the production public API. Document delta input as the preferred interface. The benchmark suite may still measure caller-side trusted and validated prefix handling because those represent work the application must choose to do.

Treat phase 3 through phase 5 as follow-up production work, not blockers for phase 1:

- Low-notification live values: benchmark the adapter branch's `JsonModemLiveValuesNoNotify` design before adding a public API.
- Completed subtrees: keep the API idea, but do not claim constant memory until the value store can release completed entries without retaining placeholders for prior array positions.
- Byte payloads: keep the conservative rule that compacted decoded strings are owned output. Borrowed raw source spans need source-buffer tracking and explicit lifetime documentation before exposure.

Do not optimize or promote a full-document `loads()` API. The benchmark suite and docs should continue to compare selected streaming fragments, cumulative-prefix partial values, full-value decoders, completed subtrees, and byte payload modes as distinct workloads.

## 2026-06-08 Production Goal Reopened

Observation: the active user goal requires completion of phases 1 through 5, not only evidence branches and a recommendation. The earlier recommendation remains useful for choosing the phase 1 API and implementation source, but it is not the final deliverable.

Command:

```bash
git worktree add -b codex/customer-incremental-json-production ../jsonmodem-customer-production origin/main
```

Consequence: production work now converges in `/home/friel/c/aaronfriel/jsonmodem-customer-production` on branch `codex/customer-incremental-json-production`. The shared plan now tracks phase 1 through phase 5 as incomplete until implementation, tests, docs, benchmarks, and validation evidence exist on that branch.

## 2026-06-08 Production Phase 1 Port

Worktree: `/home/friel/c/aaronfriel/jsonmodem-customer-production`

Observation: the minimal compaction branch's public API and native PyO3 compaction path have been ported into the production branch. The implementation keeps `JsonModem` as the public event-stream class, adds `string_events="per_feed"`, and adds explicit eager `feed_many(chunks)`.

Validation:

```bash
cargo check -p jsonmodem-py
.agent/setup-py.sh
.agent/check-py.sh
```

Result: `cargo check -p jsonmodem-py` passed. `.agent/check-py.sh` passed with `36` Python tests.

Consequence: phase 1 can now be used as the production API base for the remaining phases. The phase 5 work must replace or justify the current `string_events="per_feed", byte_views=True` rejection.

## 2026-06-08 Production Phase 2 Cumulative Prefix Decision

Worktree: `/home/friel/c/aaronfriel/jsonmodem-customer-production`

Observation: `JsonModemPrefixes` and `PrefixResult` were removed from the production public API after review. A full-prefix source has an application-level append-only contract, so the caller should decide whether to trust or validate that contract and then pass only the appended bytes to `JsonModem.feed()` or `JsonModem.feed_many()`.

Validation:

```bash
cargo check -p jsonmodem-py
.agent/check-py.sh
```

Result: `cargo check -p jsonmodem-py` passed. `.agent/check-py.sh` passed with `39` Python tests.

Consequence: benchmark group C compares direct deltas, caller-side trusted prefixes, caller-side validated prefixes, and cumulative-prefix reparsing separately, without implying a shipped prefix-parser class.

## 2026-06-08 Production Phase 3 Live Values

Worktree: `/home/friel/c/aaronfriel/jsonmodem-customer-production`

Observation: `JsonModemValues.update()` now provides the no-notification live view path on the production branch. It returns the reused `JsonModemValueView` by default and returns a changed-path summary only when requested. `reset()` preserves outstanding view identity and resets the view to empty state.

Validation:

```bash
cargo check -p jsonmodem-py
.agent/check-py.sh
```

Result: `cargo check -p jsonmodem-py` passed. `.agent/check-py.sh` passed with `42` Python tests.

Consequence: benchmark group D should use `JsonModemValues.update()` for the no-notification case rather than the adapter branch's prototype class name.

## 2026-06-08 Production Phase 4 Completed Subtrees

Worktree: `/home/friel/c/aaronfriel/jsonmodem-customer-production`

Observation: `JsonModemCompletedSubtrees` is implemented on the production branch. It emits owned Python subtree values in document order when selected objects or arrays close, skips incomplete final subtrees, supports multiple roots when parser options allow them, and exposes `retained_state()` for memory accounting.

Validation:

```bash
cargo check -p jsonmodem-py
.agent/check-py.sh
```

Result: `cargo check -p jsonmodem-py` passed. `.agent/check-py.sh` passed with `47` Python tests.

Consequence: production can satisfy completed-subtree extraction semantics, but retained-state metrics must accompany any memory claim. Current release uses `null` placeholders for emitted array entries, so parser-retained memory is not yet independent of completed item count.

## 2026-06-08 Production Phase 5 Byte Payload Modes

Worktree: `/home/friel/c/aaronfriel/jsonmodem-customer-production`

Observation: byte-view payloads now expose `payload_kind` and `ownership`. Fragment byte-view mode returns borrowed `memoryview` output only for unescaped raw source spans in immutable input buffers. Escaped strings and per-feed compacted byte-view output return owned decoded text. The README documents source-buffer retention caused by borrowed views.

Validation:

```bash
cargo check -p jsonmodem-py
.agent/check-py.sh
```

Result: `cargo check -p jsonmodem-py` passed. `.agent/check-py.sh` passed with `47` Python tests.

Consequence: benchmark group F should report borrowed raw spans and owned decoded text separately, and memory results should include pinned source-buffer behavior when callers retain borrowed views.

## 2026-06-08 Production Final Report

Worktree: `/home/friel/c/aaronfriel/jsonmodem-customer-production`

Branch: `codex/customer-incremental-json-production`

Base commit under test: `47a542760f84dd402cecda6476b56dc92dae54e5`

Environment:

- Python: `3.12.3`
- Rust: `rustc 1.96.0 (ac68faa20 2026-05-25)`
- Cargo: `cargo 1.96.0 (30a34c682 2026-05-25)`
- Packages: `jsonmodem 0.0.0a0`, `jiter 0.15.0`, `orjson 3.11.9`, `partial-json-parser 0.2.1.1.post7`, `pyperf 2.10.0`, `psutil 7.2.2`

Implemented production API:

- `JsonModem(paths=..., string_events="per_feed")` plus scalar `feed(chunk)` and eager `feed_many(chunks)` for selected-string compaction before Python event tuple/path/payload creation.
- Cumulative-prefix sources documented as caller-side append validation plus normal `JsonModem.feed()` or `JsonModem.feed_many()` calls. Direct deltas remain the recommended input shape.
- `JsonModemValues.update(chunks, changed_paths=False)` for a reused read-only value view with no per-mutation Python update tuples on the default path.
- `JsonModemCompletedSubtrees(paths=..., release_after_emit=True)` for owned completed selected values, document-order output, truncation behavior, multiple-root support, reset, and retained-state accounting.
- `JsonModem(..., byte_views=True)` with truthful `payload_kind` and `ownership` labels. Byte chunks may split UTF-8 characters; the binding carries incomplete trailing bytes into the next byte input and returns owned decoded text for reconstructed boundary fragments.

Correctness and smoke artifacts:

```bash
.venv/bin/python crates/jsonmodem-py/benchmarks/bench_customer_requirements.py \
  --fixture A_long_ascii_64k_8b_g10 \
  --fixture B_items_10_text64_16b_g4 \
  --fixture C_prefix_growing_string_4b_repeat \
  --fixture D_live_values_nested_256_64b_g8 \
  --fixture E_completed_items_1000_1k_128b_g16 \
  --fixture F_byteviews_contiguous_64k \
  --fixture F_byteviews_cross_buffer_257b_g10 \
  --fixture F_byteviews_escaped_unicode_17b_g10 \
  --fixture G_truncated_string_after_complete \
  --fixture G_invalid_byte_middle \
  --fixture H_no_match_items_2000_512b_g20 \
  --fixture I_feed_overhead_64k_8b_g10 \
  --fixture J_memory_items_10000_128b_256b_g20 \
  --fixture K_batching_llm_trace_32k_8b \
  --smoke --smoke-output target/customer-incremental-json/production-representative-ak-no-prefix-api-smoke.json
```

Result: `78` smoke rows, all `ok`, covering benchmark families A through K after removing the public prefix API.

Comparable fast pyperf command:

```bash
.venv/bin/python crates/jsonmodem-py/benchmarks/bench_customer_requirements.py \
  --fixture A_long_ascii_64k_8b_g10 \
  --fixture B_items_10_text64_16b_g4 \
  --fixture C_prefix_growing_string_4b_repeat \
  --fixture D_live_values_nested_256_64b_g8 \
  --fixture E_completed_items_1000_1k_128b_g16 \
  --fixture F_byteviews_contiguous_64k \
  --fixture F_byteviews_cross_buffer_257b_g10 \
  --fixture F_byteviews_escaped_unicode_17b_g10 \
  --fixture G_truncated_string_after_complete \
  --fixture G_invalid_byte_middle \
  --fixture H_no_match_items_2000_512b_g20 \
  --fixture I_feed_overhead_64k_8b_g10 \
  --fixture J_memory_items_10000_128b_256b_g20 \
  --fixture K_batching_llm_trace_32k_8b \
  --parser jsonmodem \
  --parser jsonmodem_values \
  --parser jsonmodem_live_values \
  --parser jsonmodem_subtrees \
  --parser stdlib_json \
  --parser orjson \
  --parser jiter \
  --fast --quiet --output target/customer-incremental-json/production-representative-ak-fast-comparable.pyperf.json
.venv/bin/python -m pyperf check target/customer-incremental-json/production-representative-ak-fast-comparable.pyperf.json
```

Result: `77` pyperf benchmark rows before public prefix API removal. `pyperf check` exited `0`; fast-mode stability warnings were expected and mean this artifact is quick comparison evidence, not a publication-grade run. Rows shared with the current API remain useful; the removed prefix-specific rows should not be cited as current production API evidence. `partial-json-parser` was included in the smoke artifact but excluded from the pyperf run because the cumulative-prefix case was much slower than the rest of the representative set.

Selected pyperf means:

- A selected 64 KiB string: fragment iterable groups `35.94 ms`; per-feed native compaction `7.31 ms`; jiter cumulative prefixes `104.34 ms`; full-value `orjson` `29.4 us` and full-value `jiter` `26.2 us` are reference-only because they return complete values.
- B ten selected strings: per-feed native compaction `162 us`; jiter cumulative prefixes `240 us`.
- C cumulative prefix: direct deltas `25.6 ms`; caller-side trusted prefix adapter `25.2 ms`; caller-side validated prefix adapter `27.5 ms`; jiter cumulative prefixes `18.4 ms`; `orjson` complete-prefix try-decode `33.2 ms`.
- D live values: per-mutation updates `3.60 ms`; no-notification `JsonModemValues.update()` `594 us`; jiter cumulative prefixes `11.0 ms`.
- E completed subtrees: `JsonModemCompletedSubtrees` owned values `12.2 ms`; full-value `orjson` reference `2.85 ms`.
- F byte payloads: contiguous borrowed byte views `34.0 us`; cross-buffer byte views `1.17 ms`; escaped/Unicode owned fallback `687 us`.
- H no-match filtering: selected no-match path `1.93 ms`; unfiltered event stream `61.9 ms`.
- I feed overhead: repeated feed `41.7 ms`; `feed_many` groups `35.4 ms`; caller-joined groups `5.16 ms`.
- J large completed-subtree memory fixture: `JsonModemCompletedSubtrees` owned values `72.5 ms`; full-value `orjson` reference `14.5 ms`.
- K LLM-style batching: 1 ms mean, 0 ms window `32.3 ms`; 1 ms mean, 4 ms window `23.2 ms`.

Memory command:

```bash
.venv/bin/python crates/jsonmodem-py/benchmarks/measure_customer_requirements_memory.py \
  --output target/customer-incremental-json/production-memory-smoke.json
```

Result: `16` rows, all `ok`. Notable Python-visible peaks: no-notification live values `168,364` bytes, byte-view contiguous `52,321` bytes, escaped Unicode byte-view fallback `187,309` bytes, J completed-subtree values `144,807` bytes, and J full stdlib decode `7,304,541` bytes with `11,837,440` RSS bytes above idle.

Validation:

```bash
cargo check -p jsonmodem-py
.agent/check-py.sh
PATH="$HOME/.local/bin:$PATH" .agent/check.sh
.venv/bin/python -m py_compile crates/jsonmodem-py/benchmarks/bench_customer_requirements.py crates/jsonmodem-py/benchmarks/measure_customer_requirements_memory.py
git diff --check
```

Result: all passed. `.agent/check-py.sh` passed with `54` Python tests and the existing pdoc `__hash__` stub warnings. `.agent/check.sh` passed; Miri was skipped by the repo default `AGENT_CHECK_MIRI_DISABLE=true`.

Recommendation:

Adopt the production branch API. Do not ship the prototype-only public classes from the evidence branches (`JsonModemFeed`, `JsonModemSelectedStrings`, `JsonModemLiveValuesNoNotify`), and do not ship `JsonModemPrefixes`. The production branch gives the user two stable choices: event streams through `JsonModem`, and reused read-only values through `JsonModemValues`. Continue documenting direct deltas as preferred. Keep `loads()` out of the optimized story.

Remaining caveat:

`JsonModemCompletedSubtrees(release_after_emit=True)` releases owned emitted values safely and reports retained state, but arrays still retain `null` placeholders for emitted items. The implementation is production-safe and measurable, but it should not claim parser-retained memory independent of completed array item count until the core value store can retain only necessary container progress.

## 2026-06-09 PR #73 Codex Review Fix

Observation: Codex review found that `released_array_entries_retained_as_null` counted all retained JSON nulls instead of only placeholders left behind by released array entries.

Change: the production branch now tracks released array placeholder paths separately and counts only those paths in `retained_state()`. Regression tests cover ordinary JSON nulls and `release_after_emit=False`.

Validation: `cargo check -p jsonmodem-py`, `.agent/check-py.sh`, `PATH="$HOME/.local/bin:$PATH" .agent/check.sh`, and `git diff --check` passed.

## 2026-06-09 PR #73 Overlapping Subtree Review Fix

Observation: Codex review found that overlapping completed-subtree paths such as `["items", "items.*"]` could release child entries before the selected ancestor emitted, so the ancestor result could contain placeholders instead of original values.

Change: the production branch now skips release for selected children when a selected ancestor can still emit. The child event reports `released == False`, and the ancestor can later emit and release the original value.

Validation: `cargo check -p jsonmodem-py`, `.agent/check-py.sh`, `PATH="$HOME/.local/bin:$PATH" .agent/check.sh`, and `git diff --check` passed.
