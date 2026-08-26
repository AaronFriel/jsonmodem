# Memory-safety tests

Run these commands from the repository root:

```sh
bash .agent/check-miri.sh
bash .agent/check-py-memory.sh
```

The first command executes Rust tests under Miri. The second loads an
AddressSanitizer-instrumented Python extension into CPython. They check different
operations; neither proves that every possible use is safe.

## Rust tests

Miri needs a Rust nightly toolchain with `miri` and `rust-src`, plus
`cargo-nextest`. `.agent/setup.sh` installs these tools. The script logs the
compiler version so a failure can be reproduced with the same nightly. Set
`JSONMODEM_MEMORY_TOOLCHAIN` to a dated nightly to reproduce a recorded run.

The full suite excludes the Python binding and fuzz executables. It includes
ordinary Rust tests and saved fuzz regression inputs. It does not run a live
fuzzer. Tests that render `insta` snapshots remain excluded under Miri;
`tests/memory_safety.rs` uses plain assertions for the streaming behavior it
checks, so it does not need snapshot filesystem access.

The targeted tests run with execution seeds 0, 1, and 2 under Stacked Borrows and
Tree Borrows. These are two models Miri uses to detect invalid overlapping Rust
references. The execution seed changes choices such as allocation addresses.
It does not specify the generated JSON inputs. The generated-input test uses
deterministic indices, defaults to 32 cases, and prints the count. Override it
with `JSONMODEM_SAFETY_CASES`; the script explicitly forwards that setting into
Miri's otherwise isolated environment. The two older QuickCheck tests retain
their existing reduced counts under Miri.

For a focused run:

```sh
bash .agent/check-miri.sh targeted
cargo test -p jsonmodem --lib memory_safety
cargo test -p jsonmodem --test memory_safety
```

### Scanner

`parser/scanner/mod.rs` has four unchecked UTF-8 conversions. Test and Miri builds
check the selected bytes before each conversion. These assertions are absent
from normal release builds.

`memory_safety_prefix_copy_operations` exercises `finish`,
`switch_to_owned_prefix_if_needed`, and `copy_prefix_to_scratch` with empty,
ASCII, and multibyte prefixes. Each assumes the anchor and cursor select valid
character boundaries in the input string. The test deliberately clears captured
text to exercise the branch that copies from the input batch.

`memory_safety_owned_ascii_and_raw_captures` exercises `push_ascii_to_scratch`,
whose text branch requires ASCII input. The invalid-UTF-8 test deliberately
violates that private helper's requirement and expects the test-only assertion
to panic before unchecked conversion. This confirms the assertion is active
without creating an invalid Rust string.

The integration test `every_character_boundary_preserves_values_and_strings`
tries every valid split position in short inputs, then feeds each character
separately. It covers Unicode escapes, surrogate pairs, multibyte characters,
empty strings, duplicate keys, and nested containers. Completed values are
compared with `serde_json`; buffered string events are compared with a single
feed of the same input.

### ValueZipper

`backend/std/value_zipper.rs` stores pointers to the current value and its
ancestors. `align_path` must discard pointers to old children before inserting
into a parent that might move its contents. `with_leaf` and `with_leaf_mut`
return references tied to the borrow of the zipper, preventing callers from
mutating the tree while those references remain in use.

Three direct tests exercise all five explicit unsafe operations. They grow
arrays and ordered maps, read the root between mutations, change siblings,
replace nested values, insert beyond an array's current end, and reuse the
zipper after `take_root`. The tests assert values and paths, not just that the
process survives. Integration tests also exercise generated containers, multiple
roots, partial values, iterator drop, and cleanup after parse errors.

## Python extension

The native check currently requires Linux x86_64, `uv`, a C linker, `nm` from
binutils, a Rust nightly, and Python with a shared `libpython`. Build products
and a separate virtual environment go under `target/python-memory`. The script
installs a wheel there; it does not replace the ordinary editable extension.
It uses `python3` by default. Set `JSONMODEM_MEMORY_PYTHON` to select another
interpreter, for example `3.13` to match CI.
CI tests Python 3.9 and 3.13. The older version exercises synchronous garbage
collection callbacks that can run while `feed()` allocates Python objects.

`scripts/python_memory_runner.rs` starts CPython from a Rust executable linked
with AddressSanitizer. The extension is built with the same Rust toolchain and
instrumentation. This avoids loading a different compiler's sanitizer runtime.
The script checks that the imported extension is inside its own environment and
references `__asan_init`.

Before testing jsonmodem, the script loads `scripts/asan_failure.rs` as a separate
test library. It deliberately reads beyond an allocation. The check must exit
unsuccessfully and report `AddressSanitizer: heap-buffer-overflow`; an import
error or another crash does not count. That library is never linked into the
production extension. The report is retained in
`target/python-memory/asan-failure.log`.

`test_buffer_safety.py` exercises successful and failed buffer acquisition,
invalid UTF-8, input layout rejection, sliced and multidimensional buffers,
bytearray resizing after success and failure, and saved events after their
parser or input is discarded. It tries all 256 single-byte mutations of a short
JSON string. Python 3.12 and later also exercise Python-defined buffer exporters
and count buffer acquisitions and releases.

Two regressions found bugs that the original tests missed. One exporter returned
different immutable bytes for parsing and payload creation. Byte-view mode now
parses the exact retained export. Another test changed a bytearray from a GC
callback during Python 3.9 parsing. Ordinary buffer input is now copied unless
its storage is known to be immutable. `scripts/gc_buffer_regression.py` runs in a
subprocess and requires actual callbacks inside `feed` on Python 3.9-3.11.

The tuple-building unsafe operations are exercised by saved nested events,
path conversion, path slicing, and no-copy byte events. The buffer operations
are exercised through `with_buffer_text`, `with_readonly_byte_text`,
`supports_buffer_protocol`, and `PyBufferGuard::drop`. The exporter must supply a
valid allocation for the requested length, and the guard must keep the export
alive until the Rust borrow or copy ends. Unknown read-only exporters are copied
to immutable bytes before parsing, so their payloads retain the copy. Known
immutable bytes-backed payloads retain their own export after the temporary
guard releases its export.

## Measuring buffer-copy cost

`benchmarks/bench_buffer_inputs.py` compares two installed release builds using
the existing synthetic array-of-strings workload, split into 512-byte chunks.
Both environments need Python 3.12 or later, `pyperf`, and `memray`. For example:

```sh
python crates/jsonmodem-py/benchmarks/bench_buffer_inputs.py \
  --baseline-python /path/to/baseline/bin/python \
  --candidate-python /path/to/candidate/bin/python > comparison.json
```

The script alternates the builds' execution order for seven paired measurements.
Each measurement times 200 streams three times and takes the median. It also
tracks allocations over 100 streams in separate Memray runs. Allocation counts
exclude free and unmap records; peak tracked memory is not process RSS. Timings
on a shared host are evidence for that run, not a performance guarantee.

## Limits

CPython itself and the prebuilt Rust standard library are not instrumented by
the native script. `PYTHONMALLOC=malloc` makes Python allocations visible to the
sanitizer allocator, but does not add checks inside CPython. Leak detection is
disabled because CPython retains allocations during shutdown. Buffer-release
assertions detect the specific export leaks they exercise; they do not establish
general leak freedom.

The Python tests do not inject allocation failures into every tuple-construction
step. Those cleanup branches also require source review. They do not fabricate
invalid pointers, prove an arbitrary native exporter's allocation size, or
establish safety under concurrent mutation by another native extension.

Miri does not execute the CPython foreign calls in this setup. The sanitizer
does not check all Rust reference-aliasing rules. Both tools check executions
selected by the tests, not all executions. Coverage here means named assumptions
and tests, not a measured percentage of all unsafe instructions or proof of
soundness.
