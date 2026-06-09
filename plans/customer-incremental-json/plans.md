# Customer Incremental JSON Requirements

This Plans.md tree coordinates production work for the customer requirements in `/home/friel/.codex/attachments/9350a2dd-7d2f-4e5f-a138-69206aae3c7f/pasted-text.txt`.

The earlier 2026-06-08 branches are evidence branches. They are not complete deliverables for phases 1 through 5. The current production target is `/home/friel/c/aaronfriel/jsonmodem-customer-production` on branch `codex/customer-incremental-json-production`.

The repository root `PLANS.md` still defines the detailed ExecPlan rules for implementation-heavy work. These files use the newer Plans.md layout: one root `plans.md`, one `records.md`, and one active child `plan.md` for each worktree. Each worker should keep its child `plan.md` and `record.md` current as code, tests, and benchmarks change.

## Plan Layout

- `plans/customer-incremental-json/plans.md`: this overview and the active worktree list.
- `plans/customer-incremental-json/records.md`: shared observations, commands, benchmark decisions, and final recommendations.
- `plans/customer-incremental-json/active/per-feed-compaction/plan.md`: implementation A, a minimal extension of the current `JsonModem` Python API.
- `plans/customer-incremental-json/active/feed-result-api/plan.md`: implementation B, an explicit `FeedResult` API that separates parser advancement from result consumption.
- `plans/customer-incremental-json/active/adapter-stack/plan.md`: implementation C, Rust-first adapters for selected strings, prefixes, live values, completed subtrees, and byte payloads.
- `plans/customer-incremental-json/active/requirements-benchmarks/plan.md`: benchmark, fixture, competitor, and recommendation work.
- `plans/customer-incremental-json/active/production-integration/plan.md`: production branch that integrates the selected API and completes phases 1 through 5.

## Active Worktrees

`customer/per-feed-compaction` lives in `/home/friel/c/aaronfriel/jsonmodem-customer-compaction`. This branch should keep the public API close to current `JsonModem` and prove whether `string_events="per_feed"` plus `feed_many(chunks)` can satisfy phase 1 without major restructuring. It may prototype later phases behind clearly named classes or options, but it should not complicate the default fragment behavior.

`customer/feed-result-api` lives in `/home/friel/c/aaronfriel/jsonmodem-customer-feedresult`. This branch should prototype a result-returning API such as `parser.feed_many(chunks) -> FeedResult`, where the parser consumes input eagerly and the result owns compacted events, changed paths, completed roots, or errors for that feed operation.

`customer/adapter-stack` lives in `/home/friel/c/aaronfriel/jsonmodem-customer-adapters`. This branch should move more behavior into Rust adapters before Python conversion. It should test whether a composable Rust design gives lower Python allocation and cleaner phase 3 through phase 5 semantics.

`customer/requirements-benchmarks` lives in `/home/friel/c/aaronfriel/jsonmodem-customer-benchmarks`. This branch should build the fixture generators, pyperf benchmark groups, competitor comparisons, memory accounting, and final recommendation report used to judge all implementation branches.

`codex/customer-incremental-json-production` lives in `/home/friel/c/aaronfriel/jsonmodem-customer-production`. This branch owns the production implementation, public Python API, tests, docs, and final benchmark report.

## Requirements Interpretation

Use case 1, selected string extraction, is the mandatory first target. JsonModem should parse only new caller-provided input, match selected paths in native code, and compact string fragments within one caller-selected feed operation before creating Python tuples, paths, payload objects, or decoded strings.

Use case 2, cumulative prefixes, must not be folded into the default parser path. Direct deltas remain the preferred and correctness-preserving API. The production Python package should not expose a cumulative-prefix parser class. Callers that receive full accumulated prefixes should enforce their own append-only contract, compute the newly appended bytes, and pass only those bytes to `JsonModem.feed()` or `JsonModem.feed_many()`. Benchmarks should account for trusted and validated caller-side prefix handling separately.

The customer text labels use cases 3 through 5 as research requirements, but the active user goal requires production completion through phase 5. A phase can remain incomplete only if the production branch records concrete evidence that the current parser architecture prevents a truthful implementation and names the follow-up design needed to remove that blocker.

## Coordination Rules

Each implementation branch has a disjoint primary write scope:

- `customer/per-feed-compaction`: Python binding API and minimal Rust/Python support for selected per-feed events.
- `customer/feed-result-api`: Python result objects, eager feed execution, and summary objects.
- `customer/adapter-stack`: Rust adapter modules and thin Python wrappers.
- `customer/requirements-benchmarks`: benchmark fixtures, pyperf harnesses, memory measurement scripts, and comparison reports.

Workers may edit tests and docs in their worktree as needed. If two branches need the same helper, each branch should prototype independently first. Integration happens after benchmark evidence exists.

Do not use timers, async stream consumption, background tasks, or transport-specific code in the parser implementation. Documentation may show caller-side batching examples, but the parser API remains synchronous.

## Recommended Order

1. Each implementation branch should first produce phase 1 selected-string compaction and correctness tests.
2. The benchmark branch should produce fixture generators and baseline timing for current `JsonModem`, `JsonModemValues`, `jiter`, `orjson`, standard-library JSON, and at least one named partial JSON package where semantics match.
3. Implementation branches should extend toward phases 2 through 5 only after their phase 1 behavior passes the shared correctness fixtures.
4. The benchmark branch should run comparable pyperf groups for each branch and write `plans/customer-incremental-json/records.md` entries summarizing results.
5. The final recommendation should name one API design to adopt, which implementation pieces to keep, which prototypes to discard, and which customer requirements remain future work.

## Production Acceptance Criteria

- Phase 1: `JsonModem(..., string_events="per_feed")`, scalar `feed(chunk)`, and eager iterable `feed_many(chunks)` compact selected string output in native code before constructing Python event tuples, path views, payload objects, or decoded strings.
- Phase 2: cumulative-prefix sources are documented as caller-side append validation plus normal delta feeds; direct delta input remains the documented preferred path and no production `JsonModemPrefixes` API is exported.
- Phase 3: live values update a reused read-only view without per-mutation Python tuples unless changed paths are requested; snapshot allocation is explicit.
- Phase 4: completed selected subtrees are emitted as owned values in document order, incomplete final values are not marked complete, and memory measurements truthfully report what parser storage retains.
- Phase 5: byte payload modes label decoded/raw and owned/borrowed output accurately, never claim zero-copy for escaped or cross-buffer decoded output, and document source-buffer retention.
- Benchmarks cover A through K where the API exists, keep comparable output semantics separate, and record command lines, versions, artifacts, and checksums.

## Review Checklist

An implementation is not considered a valid phase 1 prototype unless:

- selected path matching happens before Python path objects are constructed for unselected values;
- selected string fragments are compacted in native code within one feed operation;
- one compacted Python event is created per selected lexical string per feed operation, not one event per source fragment;
- fragment mode remains behaviorally unchanged in default construction;
- escaping, Unicode, empty strings, repeated same-path strings, wildcard paths, duplicate keys, and final empty deltas have tests;
- errors either expose preceding events or document atomic failure consistently;
- any cumulative-prefix behavior is explicitly separate from ordinary delta input.

A benchmark result is not considered recommendation-quality unless:

- it records exact package versions and command lines;
- it checks output so a faster result cannot skip work;
- it states input type and output semantics;
- it separates selected streaming events, full partial values, completed subtrees, and byte payload modes;
- it reports current main and at least one prototype under the same fixture and chunk grouping.

## Record Format

`records.md` uses short dated entries with: worktree, command or source, observation, and consequence for the recommendation. Benchmark entries must include commit SHA, command, Python version, Rust version, input fixture, output semantics, and artifact path.

Each child `record.md` may include more detailed command output, failed attempts, and branch-specific decisions.

## Progress

- [x] Created four clean worktrees from `origin/main` commit `47a542760f84dd402cecda6476b56dc92dae54e5`.
- [x] Recorded that cumulative-prefix inputs need an explicit adapter contract and should not become the default parser mode.
- [x] Assign subagents to the four active child plans.
- [x] Collect phase 1 prototype results from all implementation branches.
- [x] Collect benchmark evidence and memory measurements.
- [x] Write initial evidence-branch recommendation in `records.md`.
- [x] Create production integration worktree from `origin/main`.
- [x] Reopen child plan statuses so phases 1 through 5 are not mistaken for done work.
- [x] Integrate phase 1 native selected-string compaction into the production branch.
- [x] Complete phase 2 cumulative-prefix guidance without a production prefix parser API.
- [x] Complete phase 3 live-values API in the production branch.
- [x] Complete phase 4 completed-subtree extraction in the production branch.
- [x] Complete phase 5 byte payload modes in the production branch.
- [x] Extend and run benchmark groups C, D, E, F, G, and J.
- [x] Run `.agent/check-py.sh`, `PATH="$HOME/.local/bin:$PATH" .agent/check.sh`, and pyperf artifact checks on the production branch.
- [x] Write the final production recommendation in `records.md`.
- [x] Remove the public `JsonModemPrefixes` and `PrefixResult` API from the production branch.
- [x] Refresh A-K smoke evidence without the public prefix API.
- [x] Open PR #73 for `codex/customer-incremental-json-production` and request Codex review.
- [x] Address first relevant Codex review comment.
- [ ] Address any further Codex review feedback until review passes.
- [ ] Move the production integration plan to `completed/` when the review goal is complete.

## Decisions

- Decision: create three implementation branches plus one benchmark branch.
  Rationale: the customer requirements contain API design risk, Python allocation risk, and lifetime-contract risk. Parallel prototypes will produce concrete code and measurements instead of a single speculative design.
  Date/Author: 2026-06-08 / Codex

- Decision: treat cumulative-prefix support as an adapter, not as normal `JsonModem.feed()` input.
  Rationale: a caller that repeatedly provides the full prefix can rewrite old bytes. Without an append-only proof or validation pass, reusing parser state can be wrong. Direct deltas or byte ranges of newly appended input are the correct default.
  Date/Author: 2026-06-08 / Codex

- Decision: keep initial plan files additive in the dirty facet worktree.
  Rationale: `/home/friel/c/aaronfriel/jsonmodem` has active facet-streaming edits. Creating new worktrees from `origin/main` avoids disturbing that branch while still giving each prototype a clean base.
  Date/Author: 2026-06-08 / Codex
