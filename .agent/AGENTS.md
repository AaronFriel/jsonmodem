# Agent Instructions

To verify changes locally before submitting a PR, run the same checks as CI
(excluding the benchmark, fuzz, and Miri jobs).  The fuzz crate itself is
included in the normal build, test, and clippy steps, so ensure it compiles.

Required checks:

```bash
# Build release artifacts
cargo build --all --release --workspace

# Run tests
cargo test --all --workspace --all-features --verbose

# Lint with Clippy
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Check formatting using nightly rustfmt
cargo +nightly fmt --all -- --check

# Lint GitHub Actions workflows
bash <(curl https://raw.githubusercontent.com/rhysd/actionlint/main/scripts/download-actionlint.bash)
./actionlint -color
```

The `setup.sh` script installs the stable and nightly toolchains as well as
Clang 19 and the `llvm-tools-preview` component, which provide `llvm-nm` and
other utilities required to build the fuzz crate.

If any of the instructions in this file become inaccurate—for example, if a
benchmark or `perf` invocation no longer works—address the issue first and then
record the correction here. Add a brief addendum or note describing the
workaround so future contributors can rely on up‑to‑date guidance.

## Benchmarks

The default `cargo bench` command runs only jsonmodem's own benchmarks. The
partial JSON benchmarks skip the `serde`, `jiter`, and fix‑JSON variants unless
the optional `comparison` feature is enabled. The following commands produce
concise timings suitable for copy‑pasting:

```bash
# jsonmodem benchmarks only
cargo bench --bench streaming_parser -- --output-format bencher | rg '^test'

# sample output
# test streaming_parser_split/100  ... bench:   48241 ns/iter (+/- 1145)
# test streaming_parser_split/1000 ... bench:  161009 ns/iter (+/- 4103)
# test streaming_parser_split/5000 ... bench:  604477 ns/iter (+/- 8785)

# partial JSON benchmarks
cargo bench --bench streaming_json_medium -- --output-format bencher | rg '^test'

# include external implementations
cargo bench --features comparison --bench streaming_json_medium -- --output-format bencher | rg '^test'
```

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
PERF_BIN=$(command -v perf || true)
if [ ! -x "$PERF_BIN" ]; then
  PERF_BIN=$(find /usr/lib/linux-tools* -maxdepth 2 -name perf | sort -V | tail -n 1)
fi
# Record samples into perf.data while suppressing progress output
sudo "$PERF_BIN" record -F 999 --call-graph dwarf -o perf.data -- "$BIN" --bench >/dev/null 2>&1
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
