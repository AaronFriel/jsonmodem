#!/bin/bash
set -euxo pipefail

# Install stable Rust toolchain matching CI
rustup toolchain install 1.87.0 || true
rustup default 1.87.0
# Components used in CI
rustup component add clippy rustfmt llvm-tools-preview

# Install nightly for formatting (rustfmt nightly)
rustup toolchain install nightly || true
rustup component add rustfmt llvm-tools-preview --toolchain nightly

# Install Clang 19 and perf for profiling
sudo apt-get update
sudo apt-get install -y wget gnupg lsb-release
wget -qO- https://apt.llvm.org/llvm-snapshot.gpg.key | sudo tee /usr/share/keyrings/llvm.asc
echo "deb [signed-by=/usr/share/keyrings/llvm.asc] http://apt.llvm.org/$(lsb_release -cs)/ llvm-toolchain-$(lsb_release -cs)-19 main" | sudo tee /etc/apt/sources.list.d/llvm19.list
sudo apt-get update
sudo apt-get install -y clang-19 lldb-19 lld-19 llvm-19-dev linux-tools-common "linux-tools-$(uname -r)" \
  || sudo apt-get install -y clang-19 lldb-19 lld-19 llvm-19-dev linux-tools-generic
sudo update-alternatives --install /usr/bin/clang clang /usr/bin/clang-19 100
sudo update-alternatives --install /usr/bin/clang++ clang++ /usr/bin/clang++-19 100

# linux-tools packages are installed above, preferring a version matching the
# running kernel and falling back to the generic package when unavailable.
# Attempt to enable perf events for the current user. This can fail if
# /proc/sys is read-only, such as in CI containers, so ignore errors.
sudo bash -c 'echo 0 > /proc/sys/kernel/perf_event_paranoid' || true
