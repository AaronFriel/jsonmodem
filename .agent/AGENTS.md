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
