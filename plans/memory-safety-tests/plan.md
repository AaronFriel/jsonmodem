# Test unsafe streaming code and Python buffers


Status: upstream PR #74 is ready for review at the user's request; hosted
validation awaits maintainer approval (2026-08-26).
This plan follows `PLANS.md`. Keep Progress, Decision Log, and Outcomes current
until the upstream PR's final checks pass.

## Purpose / Big Picture


Make memory-safety testing reproducible for jsonmodem's streaming scanner,
raw-pointer value traversal, and Python buffer handling. The previous Miri job
excluded the Python crate and some Rust integration tests. The new tests found
two Python input ownership defects, which this branch fixes.

## Plan Layout


This file owns scope and completion status. `record.md` records regressions,
commands, measurements, and publication evidence. `docs/memory-safety-testing.md`
maps unsafe operations to tests and explains the tools' limits. Full logs,
wheels, and generated measurement data remain outside tracked files. Planning
notes were condensed before publication; their earlier versions remain in Git.

## Work Boundaries


Branch `dev/friel/memory-safety-tests` starts directly at upstream
`47a542760f84dd402cecda6476b56dc92dae54e5`. It does not depend on the separate
orjson frontend work. Publish from `friel-openai/jsonmodem` to
`AaronFriel/jsonmodem:main`; do not merge or modify another PR.

Changes cover `crates/jsonmodem/src/parser/scanner/`,
`crates/jsonmodem/src/backend/std/value_zipper.rs`, Python buffer handling in
`crates/jsonmodem-py/src/lib.rs`, their tests, and test tooling. Use only public
or synthetic inputs. Preserve streaming events, supported valid inputs, and
borrowing of known immutable storage. Do not add runtime dependencies.

## Definition of Done


Each explicit unsafe operation in the three source areas has a recorded safety
assumption and named tests, or an explicit limitation. Relevant Rust tests run
under Miri, including targeted runs under both reference models with three
execution seeds. Python tests run with verified native instrumentation. Every
confirmed defect has a failing baseline and a passing regression after the fix.

Existing tests pass. Performance and allocation changes are measured. The
separate upstream PR is labeled `jsonmodem` if repository permissions allow it,
and required checks pass on its final head. The user explicitly requested ready
status before those checks could run; preserve that status.
Document any permission limitation rather than bypassing repository controls.
No merge is authorized.

## Progress


- [x] (2026-08-26) Create the independent worktree from upstream `47a5427`.
- [x] (2026-08-26) Record unsafe assumptions and add six unit tests and four integration tests.
- [x] (2026-08-26) Pass 188 full-suite Miri tests and six targeted configurations.
- [x] (2026-08-26) Reproduce and fix exporter reacquisition and Python 3.9 GC mutation of borrowed input.
- [x] (2026-08-26) Verify native instrumentation; pass 47 Python 3.9 tests and 52 Python 3.13 tests.
- [x] (2026-08-26) Measure streaming time and allocations; remove per-chunk attribute-name allocations.
- [x] (2026-08-26) Complete local checks and independent source review.
- [x] (2026-08-26) Refresh upstream once; confirm the base remains `47a5427` and no matching PR exists.
- [x] (2026-08-26) Push the branch and create upstream draft PR #74.
- [x] (2026-08-26) Record the project-label permission limitation; the publishing account cannot create the missing label.
- [x] (2026-08-26) Mark PR #74 ready for review at the user's explicit request.
- [ ] Verify required hosted checks on the final published head after maintainer approval.

## Surprises and Discoveries


A Python exporter could return one buffer for parsing and a different buffer
for payload views. Python 3.9 could also run a garbage-collection callback during
`feed()` and mutate a borrowed bytearray. These are input ownership defects;
AddressSanitizer did not diagnose the UTF-8 invariant violation. The regression
tests therefore check behavior as well as native memory errors.

The existing local check script compiled a Miri configuration for Clippy but
skipped actual Miri execution by default. Actual execution is now a separate,
documented command. Long, combined split-input cases were too slow under Miri;
short cases retain exhaustive split positions, while separate tests cover growth.

## Decision Log


2026-08-26: Base this work on upstream, since the scanner, ValueZipper, and
streaming Python binding already exist there. Keep it independently mergeable.

2026-08-26: Use Miri for Rust and AddressSanitizer plus behavioral regressions for
Python. Verify instrumentation with a deliberately invalid test library that is
never linked into the production extension.

2026-08-26: Snapshot mutable or unverifiable storage before Python callbacks can
run. Preserve borrowing of immutable bytes-backed storage. Unknown read-only
exporters remain accepted but their payloads retain an immutable snapshot.

2026-08-26: Add Python 3.9 to native CI because its synchronous garbage collection
exercises a defect that newer interpreters did not reproduce. Intern the `obj`
attribute name after measurements showed avoidable allocations.

2026-08-26: Reopen this plan for the user's upstream publication request. The
publishing account has read-only upstream access; project-label creation failed.

2026-08-26: The user requested ready status while fork workflows await approval.
Mark PR #74 ready without changing CI requirements. Do not return it to draft
without the user's permission. Readiness does not establish successful testing.

## Outcomes and Retrospective


Implementation and local validation are complete. Two Python ownership defects
were fixed without adding production unsafe blocks. Scanner assertions are
test/Miri-only. Bytearray input adds one snapshot allocation per chunk; measured
immutable-input allocation counts are unchanged. Shared-host timings do not
support a general speedup claim or a consistent slowdown.

[Upstream PR #74](https://github.com/AaronFriel/jsonmodem/pull/74) is ready for
review at the user's request. All ten workflow runs require maintainer approval
before executing fork code. The publishing account cannot approve them or create
the missing project label. Final hosted validation remains incomplete.
Local results and coverage limits are in `record.md`; passing tests are not proof
of soundness.

## Context and Orientation


The scanner's unchecked conversions require valid UTF-8. ValueZipper caches
pointers into a boxed root, arrays, and ordered maps; old child pointers must be
discarded before ancestor storage moves. Python buffer guards retain native
storage, but read-only access does not establish immutable backing.

Miri executes Rust tests and checks certain invalid memory operations and
reference uses. Its Stacked Borrows and Tree Borrows models check overlapping
references. AddressSanitizer instruments native access checks but does not
check every Rust reference rule. CPython itself remains uninstrumented here.

## Plan of Work


The completed implementation adds test-only scanner assertions, dedicated
ValueZipper tests, and plain integration assertions that do not depend on
snapshot filesystem access. Python tests cover buffer release, retained views,
input mutations, and the two confirmed callback defects.

`.agent/check-miri.sh` runs the existing full suite and targeted configurations.
`.agent/check-py-memory.sh` builds a launcher and extension using the same Rust
sanitizer runtime, verifies an expected failure, and runs Python tests. CI calls
these scripts. Publication must retain this test coverage and its limitations.

## Concrete Steps


Run from this worktree:

    .agent/check.sh
    .agent/setup-py.sh
    .agent/check-py.sh
    cargo clippy -p jsonmodem-py -- -D warnings
    cargo test -p jsonmodem --release --lib memory_safety
    cargo test -p jsonmodem --release --test memory_safety
    bash .agent/check-miri.sh
    JSONMODEM_MEMORY_PYTHON=3.9 bash .agent/check-py-memory.sh
    JSONMODEM_MEMORY_PYTHON=3.13 bash .agent/check-py-memory.sh

The fastest Rust checks are `cargo test -p jsonmodem --lib memory_safety` and
`cargo test -p jsonmodem --test memory_safety`. The native script requires Linux
x86_64 and a shared libpython. Tool prerequisites are documented in `AGENTS.md`.

## Validation and Acceptance


All commands must pass without unexplained diagnostics. Keep the deliberate
sanitizer failure in its isolated subprocess; it is required evidence that
instrumentation works. Distinguish excluded crates, compiled-out tests, ignored
tests, and actual execution. Preserve baseline failures for production fixes.
After publication, inspect checks for the exact published head before completing
this plan. The PR is already ready at the user's request. Do not bypass approvals
or classify ready status or missing checks as passing CI.

## Idempotence and Recovery


Use this worktree's environments and target directories. Do not reset another
checkout or force-push. Keep full artifacts outside Git. For a failing command,
record whether the cause is a test failure, tool setup, or missing permission.
Do not suppress a memory error or disable reference checks to obtain a pass.

## Interfaces and Dependencies


Public Rust and Python entry points remain unchanged. Valid external exporters
are still accepted, with snapshots where immutability cannot be established.
Use existing PyO3, pytest, Miri, and native compiler tooling. Memray and pyperf
are development-only dependencies for the buffer comparison benchmark.

## Artifacts and Notes


`record.md` retains the source commits, reproducer results, validation commands,
timing and allocation tables, and publication status. Generated wheels, logs,
and allocator traces are not part of the upstream diff.

## Next Action


A maintainer must approve the fork workflows on PR #74 and apply the `jsonmodem`
label. After approval, inspect the runs for the current PR head and address
in-scope failures. Preserve ready status and complete this plan only after
required checks pass. Do not bypass approval or treat an empty check list as
passing CI.

Revision (2026-08-26): Record the user's request for ready status separately from
the still-incomplete hosted validation, so review status cannot be mistaken for
test evidence.
