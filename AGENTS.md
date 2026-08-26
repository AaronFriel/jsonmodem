# Agent Instructions

To verify changes locally before submitting a PR, run the same checks as CI
(excluding the benchmark, fuzz, and Miri jobs). The fuzz crate itself is
included in the normal build, test, and clippy steps, so ensure it compiles.
Local runs enable the `bench-fast` and `test-fast` features for quick feedback;
CI executes the full suites without these flags.

Required checks:

```bash
.agent/check.sh
```

Run this script early and often—ideally after every meaningful change—so
failures surface while the context is still fresh. Keeping the local workspace
green makes it easier to hand off high-quality changes and prevents last-minute
surprises when preparing a PR.

The `setup.sh` script installs the stable and nightly toolchains as well as
Clang 19 and the `llvm-tools-preview` component, which provide `llvm-nm` and
other utilities required to build the fuzz crate. When new development tools are
needed, document them here and add installation steps to `setup.sh` so
contributors can reproduce the environment.

If any of the instructions in this file become inaccurate—for example, if a
benchmark or `perf` invocation no longer works—address the issue first and then
record the correction here. Add a brief addendum or note describing the
workaround so future contributors can rely on up‑to‑date guidance.

## Benchmarks

The default `cargo bench` command runs only jsonmodem's own benchmarks. The
partial JSON suite can include `serde`, `jiter`, and fix-JSON variants when the
`JSONMODEM_BENCH_COMPARISON` environment variable is set. For quick local runs,
set `JSONMODEM_BENCH_FAST=1` to use shorter warmup and measurement intervals.

For Python performance work, the primary comparison is incremental parsing of a
stream of JSON fragments. Compare `jsonmodem` to `jiter` and other libraries by
having each parser process the same fragment boundaries and, when the
competitor supports partial parsing, every cumulative prefix. Do not present
one-shot `loads()`/full-document decode as the optimization target for
`jsonmodem`; include those numbers only as clearly labeled reference results.

## Flamegraphs and line-level profiling

This repository ships a GitHub Action that runs
`cargo flamegraph --bench streaming_json_medium -- --bench` and uploads
`flamegraph.svg`.  The `setup.sh` script installs `perf` so the same
command can be run locally:

```bash
cargo install flamegraph --locked
sudo apt-get install -y linux-tools-common "linux-tools-$(uname -r)" || \
  sudo apt-get install -y linux-tools-generic
sudo bash -c 'echo 0 > /proc/sys/kernel/perf_event_paranoid'
cargo flamegraph --package jsonmodem --bench streaming_json_medium -- --bench

# Finished release [optimized] target(s) in 0.23s
# Flamegraph written to flamegraph.svg
```

To attribute samples to individual lines, compile with frame pointers and
line-tables debug info and record with `perf`:

```toml
[profile.release]
debug = "line-tables-only"
```

```bash
RUSTFLAGS="-C force-frame-pointers=yes" \
  cargo bench --bench streaming_json_medium --no-run
BIN=$(find target/release/deps -maxdepth 1 -executable -name 'streaming_json_medium-*' | head -n 1)
# Locate the perf binary in case the wrapper doesn't match the running kernel
# Use the real perf binary instead of the wrapper which may fail when the
# kernel version doesn't match an installed package.
PERF_BIN=$(find /usr/lib/linux-tools* -maxdepth 2 -name perf | sort -V | tail -n 1)
if [ -z "$PERF_BIN" ]; then
  PERF_BIN=$(command -v perf)
fi
echo "Using $PERF_BIN"
# Record a short run of the parse_partial_json benchmark to keep the report small
sudo "$PERF_BIN" record -F 200 --call-graph fp -o perf.data -- \
  "$BIN" --bench parse_partial_json --sample-size 10 --measurement-time 1 >/dev/null 2>&1
# Change ownership so perf_report can read the file
sudo chown "$(id -u):$(id -g)" perf.data
# Generate a report showing file and line numbers
"$PERF_BIN" report -i perf.data -g fractal -F+srcline --stdio > perf_report.txt 2>&1
# Extract the hottest lines with surrounding code
python3 scripts/perf_snippet.py perf_report.txt | tee perf_snippet.log

The helper script reads `perf_report.txt`, extracts the hottest lines,
and prints them with short code snippets. Redirect the output if you
want to save it:

```bash
python3 scripts/perf_snippet.py | tee perf_with_code.txt
```

# Example output
```text
40.0% crates/jsonmodem/src/parser.rs:123
   122:     StringEscapeUnicode,
   123:     BeforePropertyName,
   124:     AfterPropertyName,

25.0% crates/jsonmodem/src/lexer.rs:87
    86:     };
    87: }
    88:
```

For deterministic instruction counts, `cargo profiler callgrind --release --bench streaming_json_medium` will emit
`callgrind.out.*` which can be viewed with `kcachegrind` and also prints the hottest lines directly in the
terminal.

## Python bindings

Memory-safety checks are separate from the ordinary test scripts. Run
`bash .agent/check-miri.sh` for the Rust suite and targeted tests under both Miri
reference models. Run `bash .agent/check-py-memory.sh` for the Python extension
under AddressSanitizer. The latter requires Linux x86_64, a Rust nightly
toolchain, `uv`, `nm` (binutils), a C linker, and a Python interpreter with a
shared `libpython`. It creates its own environment under `target/python-memory`.
The existing setup scripts install the Rust and Python development tools;
`.agent/setup.sh` installs binutils and the linker. See
`docs/memory-safety-testing.md` for coverage and limitations.

Building and testing the Python bindings is driven by two helper scripts,
`setup-py.sh` and `check-py.sh`.  `setup-py.sh` installs
[uv](https://github.com/astral-sh/uv), creates a `.venv` in the repository root,
and installs `maturin` before building the extension with
`maturin develop`.  Like `setup.sh`, it is idempotent and is executed
automatically when the agent environment is prepared.

`check-py.sh` rebuilds the bindings and runs the smoke tests under `pytest`.
The `py.yml` GitHub Action calls `setup.sh`, then `setup-py.sh`, and finally
`check-py.sh` to verify that the Python package can be built and imported.


## Snapshot Testing

This repo uses the `insta` crate for snapshot tests. To avoid churn and
out-of-date `.snap` files, always use inline snapshots and the `cargo insta`
workflow when adding or updating tests.

- Run tests that update snapshots with:
  - `cargo insta test` (executes tests and records new/changed snapshots)
  - `cargo insta review` (interactive approval of changed snapshots)

- Use inline snapshots only. Prefer this pattern:

  ```rust
  let output = render_something();
  insta::assert_snapshot!(output, @"");
  ```

  The second argument `@""` is a default inline snapshot. On first run,
  `cargo insta test` will populate the inline content with the actual output.
  Avoid named snapshots (e.g. `assert_snapshot!("name", data)`) and avoid
  `.snap` files entirely.

- Do not assert inside loops. `insta` forbids inline assertions in loops by
  default. Either unroll the cases or extract a helper and use separate
  assertions for each case. If you absolutely must loop, wrap the section in
  `insta::allow_duplicates!` and justify in a comment.

- Multi-line snapshots: use a raw string if helpful (e.g. `@r#"..."#`). Keep
  snapshots stable and legible.

Following these rules ensures that contributors can run and update snapshots
consistently and that CI remains deterministic.
# ExecPlans

When writing complex features or significant refactors, use an ExecPlan (as described in PLANS.md) from design to implementation. ExecPlans are living documents and should be referred to and updated frequently throughout implementation. Store new execplans in plans/$short-feature-name/, e.g.: plans/py for the Python library.

When instructed to implement an ExecPlan, implement it from start to finish autonomously, solving issues that arise independently. Work tirelessly, diligently; indefatigably. You have infinite time to complete ExecPlans, your context window will auto-compact, so refer back to the ExecPlan whenever it is no longer in your context window and diligently maintain it.
