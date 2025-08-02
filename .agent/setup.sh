#!/usr/bin/env bash
set -euxo pipefail

################################################################################
# Basic repo info
################################################################################
REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
REPO_NAME="$(basename "$REPO_ROOT")"
FUZZ_CRATE="${REPO_NAME}-fuzz"           # convention: <repo>-fuzz
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SETUP_DONE_FILE="${SCRIPT_DIR}/.setup_done"
[[ -f "$SETUP_DONE_FILE" ]] && { echo "✅ Rust development environment already set up."; exit 0; }

################################################################################
# Toolchains – “stable” floats automatically, no version parsing needed
################################################################################
STABLE_TOOLCHAIN="stable"
NIGHTLY_TOOLCHAIN="nightly"
CLANG_VERSION=19

if ! command -v rustup >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
    sh -s -- -y --profile minimal
  export PATH="$HOME/.cargo/bin:$PATH"
fi

rustup toolchain install "$STABLE_TOOLCHAIN"  || true   # already installed in most images
rustup default          "$STABLE_TOOLCHAIN"
rustup component add clippy rustfmt llvm-tools-preview

rustup toolchain install "$NIGHTLY_TOOLCHAIN" || true
rustup component add rustfmt llvm-tools-preview miri rust-src --toolchain "$NIGHTLY_TOOLCHAIN"

HOST_TRIPLE="$(rustc -vV | awk '/host:/ {print $2}')"
export PATH="$(rustc +stable  --print sysroot)/lib/rustlib/${HOST_TRIPLE}/bin:$PATH"
export PATH="$(rustc +nightly --print sysroot)/lib/rustlib/${HOST_TRIPLE}/bin:$PATH"

################################################################################
# Developer utilities
################################################################################
curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/v1.14.3/install-from-binstall-release.sh \
  | BINSTALL_VERSION=v1.14.3 bash
cargo binstall -y cargo-nextest cargo-edit cargo-fuzz flamegraph

if ! command -v cargo-insta >/dev/null 2>&1; then
  curl -LsSf https://insta.rs/install.sh | sh
fi

go install github.com/rhysd/actionlint/cmd/actionlint@latest

if command -v mise >/dev/null 2>&1; then
  mise settings add idiomatic_version_file_enable_tools python || true
fi

################################################################################
# LLVM/Clang + perf tooling
################################################################################
sudo apt-get update
sudo apt-get install -y wget gnupg lsb-release
wget -qO- https://apt.llvm.org/llvm-snapshot.gpg.key | sudo tee /usr/share/keyrings/llvm.asc
echo "deb [signed-by=/usr/share/keyrings/llvm.asc] \
  http://apt.llvm.org/$(lsb_release -cs)/ llvm-toolchain-$(lsb_release -cs)-${CLANG_VERSION} main" | \
  sudo tee /etc/apt/sources.list.d/llvm${CLANG_VERSION}.list

sudo apt-get update
sudo apt-get install -y \
  clang-${CLANG_VERSION} lldb-${CLANG_VERSION} lld-${CLANG_VERSION} llvm-${CLANG_VERSION}-dev 
sudo apt-get install -y linux-tools-common "linux-tools-$(uname -r)" \
  || sudo apt-get install -y linux-tools-generic

# If the running kernel lacks a matching linux-tools package,
# ensure the generic tools are accessible via the expected path
TOOLS_DIR="/usr/lib/linux-tools/$(uname -r)"
if [[ ! -d "$TOOLS_DIR" ]]; then
  FALLBACK_DIR="$(ls -d /usr/lib/linux-tools/* 2>/dev/null | head -n1 || true)"
  if [[ -n "$FALLBACK_DIR" && ! -e "$TOOLS_DIR" ]]; then
    sudo ln -s "$FALLBACK_DIR" "$TOOLS_DIR"
  fi
fi

sudo update-alternatives --install /usr/bin/clang   clang   /usr/bin/clang-${CLANG_VERSION}   100
sudo update-alternatives --install /usr/bin/clang++ clang++ /usr/bin/clang++-${CLANG_VERSION} 100
sudo bash -c 'echo 0 > /proc/sys/kernel/perf_event_paranoid' || true

################################################################################
# Pre-build (skip fuzz crate)
################################################################################
EXCLUDE_ARGS=(--exclude "$FUZZ_CRATE")

cargo fetch
cargo build  --workspace --release               "${EXCLUDE_ARGS[@]}"
cargo test   --workspace --all-features --no-run  "${EXCLUDE_ARGS[@]}"
cargo clippy --workspace --all-targets --all-features "${EXCLUDE_ARGS[@]}" -- -D warnings

# Optional fuzz pre-compile
if [[ -d "$FUZZ_CRATE" ]]; then
  cargo +nightly fuzz build "$FUZZ_CRATE" || true
fi

################################################################################
touch "$SETUP_DONE_FILE"
echo "✅ Rust development environment ready."
