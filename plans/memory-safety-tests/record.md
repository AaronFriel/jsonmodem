# Memory-safety test evidence


Implementation is complete; upstream publication is in progress. Detailed test
coverage and tool limitations are in `docs/memory-safety-testing.md`.

## Baseline and source commits


The branch starts at upstream `47a542760f84dd402cecda6476b56dc92dae54e5`.
`7883dad` adds the test infrastructure and a failing exporter regression before
changing production behavior. `2e61d8fabc0930bed8323e10124b1e197a099280` fixes
buffer ownership and avoids repeated attribute-name allocations. The branch
does not contain the separate orjson frontend commits.

The baseline scanner and Python suites each passed 27 tests. Miri excluded
`jsonmodem-py` and the fuzz executables. Snapshot-based values and buffers tests
were compiled out, although some adapter regression tests remained enabled.
The new tests use plain assertions and record their generated-input count.

## Reproduced defects


`with_readonly_byte_text` acquired input once for text, then called
`PyMemoryView::from(data)` for payload storage. A Python 3.12 exporter returning
`b'["first"]'` on the first two acquisitions and `b'["second"]'` on the third
produced `b'secon'`. The regression failed before the fix with
`b'secon' != b'first'`. Parsing now uses the exact retained export. Unknown
read-only exporters are snapshotted into immutable bytes before Rust borrows.

`with_buffer_text` held a borrowed string while Python allocations could trigger
garbage collection. On Python 3.9.25, a GC callback changed an ASCII byte in a
bytearray during `feed()`, changing the last parsed string from `abc` to `abz`.
The callback verified it ran inside feed by observing PyO3's mutable-borrow error
when reading `parser.is_finished`. The baseline subprocess ran nine such
callbacks and failed the unchanged-text assertion. The fixed subprocess passes
for both bytearray and memoryview input. Mutable or unverifiable input is copied
before parsing; exact bytes-backed storage remains borrowed.

Independent review also reproduced mutation to invalid UTF-8 in an isolated
process. AddressSanitizer did not diagnose that invariant violation. The checked-in
regressions use ASCII mutation or different immutable buffers, so they detect
the defects without deliberately constructing invalid Rust strings.

The source review found both fixes closed and no further defect in the launcher,
instrumentation verification, or Rust tests. This judgment is separate from
execution evidence and does not remove the documented limitations.

## Validation


Rust nightly: `1.100.0-nightly (e7769602a 2026-08-24)`, LLVM 23.1.0.
The existing stable checks used Rust 1.94.1. Local commands:

    .agent/check.sh
    .agent/check-py.sh
    cargo clippy -p jsonmodem-py -- -D warnings
    cargo test -p jsonmodem --release --lib memory_safety
    cargo test -p jsonmodem --release --test memory_safety
    MIRIFLAGS=-Zmiri-env-set=JSONMODEM_SAFETY_CASES=32 cargo +nightly miri nextest run --workspace --profile default-miri --exclude jsonmodem-fuzz --exclude jsonmodem-py
    bash .agent/check-miri.sh targeted
    JSONMODEM_MEMORY_PYTHON=3.9 bash .agent/check-py-memory.sh
    JSONMODEM_MEMORY_PYTHON=3.13 bash .agent/check-py-memory.sh

The ordinary Rust check script, Python build/tests, Clippy, workflow linting, and
release-mode safety tests passed. Miri's full suite passed 188 tests, with four
ignored debug helpers, in 242.313 seconds. Excluded crates and compiled-out
snapshot tests are additional omissions, not part of those four.

All six targeted Miri configurations passed: Stacked Borrows and Tree Borrows,
each with execution seeds 0, 1, and 2. Each configuration ran six unit tests and
four integration tests, including 32 deterministic generated cases. Unit tests
took about 8.5 seconds and integration tests about 98.5 seconds.

Native checks first detected the deliberately invalid test library's
heap-buffer-overflow, then verified the installed extension referenced
`__asan_init`. Python 3.9.25 passed 47 tests, skipping five Python-defined-exporter
cases requiring Python 3.12. Its GC regression exercised callbacks inside feed.
Python 3.13.14 passed 52 tests. These results include the final ownership and
attribute-name changes.

## Test and instrumentation decisions


Four scanner assertions validate UTF-8 before unchecked conversion in test/Miri
builds only. A should-panic test confirms the assertion runs before conversion.
Direct zipper tests cover array/map growth, siblings, replacements, root reads,
and root reuse. Integration tests exercise actual streaming adapters.

An initial Miri attempt used long combined inputs and partial snapshots after
every feed. It was stopped, not counted as passing. Shorter inputs retain every
valid split position; separate tests cover container growth and partial values.

The native launcher and extension share Rust's AddressSanitizer runtime.
CPython and the prebuilt Rust standard library are uninstrumented. Leak checking
is disabled for CPython shutdown allocations. Explicit buffer-release tests do
not prove general leak freedom. Tests do not inject every allocation failure,
fabricate invalid native exporter pointers, or establish safety under concurrent
mutation by another native extension. No passing result is a proof of soundness.

## Measurement method


Before measuring, the question was whether immutable-owner checks and snapshots
slow chunked input or add avoidable allocations. The decision rule was to retain
borrowing of known immutable storage and remove unnecessary copying or callback
work without weakening ownership. The comparison below uses the existing public
array-of-strings workload and separate timing and Memray runs.

## Final measurements


The first measurement found avoidable allocations from repeatedly constructing
the Python attribute name `obj`. The fix now interns that name and reads a
byte-view owner's attribute only once. This removed those per-chunk allocations
without weakening the ownership checks.

The reproducible comparison is now
`crates/jsonmodem-py/benchmarks/bench_buffer_inputs.py`. Command:

    .venv/bin/python crates/jsonmodem-py/benchmarks/bench_buffer_inputs.py --baseline-python target/memory-benchmark-baseline/bin/python --candidate-python .venv/bin/python

The baseline release wheel contains production code from `7883dad`; the candidate
contains the buffer fixes and interned attribute name committed in
`2e61d8fabc0930bed8323e10124b1e197a099280`. Both used CPython 3.12.13,
Rust 1.94.1, and Memray 1.20.0 on the same shared Linux x86_64 host, without CPU
pinning. Seven paired timing measurements alternate execution order. Each is
the median of three measurements of 200 streams. Each stream contains 1,024
strings, 7,169 input bytes, 15 chunks, and 1,034 emitted events. All event counts
matched. Memray runs are separate from timing, with 100 streams after ten warmups.

| Input/mode | Baseline us/stream | Fixed us/stream | Median paired fixed/baseline | Paired ratio range |
| --- | ---: | ---: | ---: | --- |
| bytes, ordinary events | 365.25 | 354.69 | 0.970 | 0.955-1.024 |
| bytearray, ordinary events | 366.36 | 365.86 | 0.986 | 0.960-1.054 |
| bytes-backed memoryview, ordinary events | 366.70 | 358.04 | 0.981 | 0.904-0.994 |
| bytes, byte-view events | 535.55 | 532.71 | 1.001 | 0.938-1.021 |
| Python exporter, byte-view events | 544.51 | 551.24 | 1.003 | 0.961-1.054 |

Ratios near one indicate similar timings, not proof of zero overhead. Variation
on this shared host does not support a general speedup claim. The bytes fast
case was not changed by the ownership fix.

| Input/mode | Baseline allocations/stream | Fixed allocations/stream | Baseline peak tracked bytes | Fixed peak tracked bytes |
| --- | ---: | ---: | ---: | ---: |
| bytes, ordinary events | 2921.46 | 2921.46 | 11164 | 11164 |
| bytearray, ordinary events | 2921.07 | 2936.07 | 8644 | 9084 |
| bytes-backed memoryview, ordinary events | 2921.07 | 2921.07 | 8644 | 8644 |
| bytes, byte-view events | 5144.07 | 5144.07 | 2902027 | 2902027 |
| Python exporter, byte-view events | 5339.07 | 5339.07 | 2902515 | 2904596 |

These counts use native and Python allocator tracing and exclude free/unmap
records. Bytearray input adds exactly one snapshot allocation per chunk. The
Python exporter needs a snapshot but avoids reacquiring another export, leaving
the measured allocation count unchanged. Peak tracked memory is not RSS.
No claim about orjson performance follows from this safety-fix comparison.

## Publication


On 2026-08-26, upstream main was refreshed and remained at `47a5427`. The branch
was clean and no matching upstream PR existed. The publishing account is
`friel-openai`, with read-only upstream permission. The `jsonmodem` label does
not exist upstream; an attempt to create it returned HTTP 404. Labeling requires
maintainer assistance.

[Upstream PR #74](https://github.com/AaronFriel/jsonmodem/pull/74) was created as
a draft from the user's fork into upstream main. On 2026-08-26, the user explicitly
requested ready status. `gh pr ready 74 -R AaronFriel/jsonmodem` succeeded, and
`gh pr view` confirmed `isDraft=false` at `bcce5200ff12403ae5ddc111acba3fcc6f8d785c`.
Preserve ready status; no merge is authorized.

All ten workflows for both the initial published head `f99ef28` and the
subsequent head `bcce520` concluded `action_required`; no hosted tests ran.
The [Miri run for bcce520](https://github.com/AaronFriel/jsonmodem/actions/runs/32936966789)
requires maintainer approval. Ready status and the empty PR check rollup are not
passing evidence. Hosted validation remains incomplete and must cover the final
published head, including any later documentation commit.

GitHub's repository API confirmed that the authenticated publishing account has
admin/push permission on `friel-openai/jsonmodem` but no write/triage permission
on `AaronFriel/jsonmodem`. The upstream PR has no labels. Workflow approval and
creation/application of the missing `jsonmodem` label require upstream authority
that these credentials do not have. Recheck the current head after approval.
