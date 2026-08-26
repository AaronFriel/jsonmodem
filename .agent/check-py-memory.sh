#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"
if [[ "$(uname -s)" != Linux || "$(uname -m)" != x86_64 ]]; then
    echo "This check requires Linux x86_64." >&2
    exit 1
fi

TOOLCHAIN="${JSONMODEM_MEMORY_TOOLCHAIN:-nightly}"
BUILD="$ROOT/target/python-memory"
VENV="$BUILD/venv"
mkdir -p "$BUILD/wheels"
uv venv --allow-existing --python "${JSONMODEM_MEMORY_PYTHON:-python3}" "$VENV"
uv pip install --python "$VENV/bin/python" maturin pytest

rustc "+$TOOLCHAIN" -vV
"$VENV/bin/python" -VV
LIBDIR="$("$VENV/bin/python" -c 'import sysconfig; print(sysconfig.get_config_var("LIBDIR"))')"
LIBRARY="$("$VENV/bin/python" -c 'import sysconfig; print(sysconfig.get_config_var("LDLIBRARY").removeprefix("lib").split(".so")[0])')"

# A Rust executable supplies exactly the runtime used to instrument the extension.
rustc "+$TOOLCHAIN" --edition=2024 -Zsanitizer=address -Cdebuginfo=1 \
    -L "native=$LIBDIR" -l "$LIBRARY" \
    -C "link-arg=-Wl,-rpath,$LIBDIR" -C link-arg=-Wl,--export-dynamic \
    scripts/python_memory_runner.rs -o "$BUILD/python-memory"
rustc "+$TOOLCHAIN" --edition=2024 -Zsanitizer=address -Cdebuginfo=1 --crate-type cdylib \
    scripts/asan_failure.rs -o "$BUILD/asan_failure.so"

export PYTHONMALLOC=malloc
# CPython itself is uninstrumented and retains allocations at shutdown. This
# checks invalid accesses, not interpreter-wide leaks; buffer release has tests.
export ASAN_OPTIONS=detect_leaks=0:abort_on_error=1
ulimit -c 0
"$VENV/bin/python" scripts/check_asan_failure.py \
    "$BUILD/python-memory" "$VENV/bin/python" "$BUILD/asan_failure.so" "$BUILD/asan-failure.log"

RUSTUP_TOOLCHAIN="$TOOLCHAIN" CARGO_TARGET_DIR="$BUILD/cargo" \
    RUSTFLAGS="-Zsanitizer=address -Cdebuginfo=1" \
    "$VENV/bin/maturin" build -m crates/jsonmodem-py/Cargo.toml \
    --interpreter "$VENV/bin/python" \
    --release --target x86_64-unknown-linux-gnu --out "$BUILD/wheels"
uv pip install --python "$VENV/bin/python" --reinstall "$BUILD"/wheels/jsonmodem-*.whl
EXTENSION="$("$BUILD/python-memory" "$VENV/bin/python" -c \
    'import jsonmodem._jsonmodem as extension; print(extension.__file__)')"
if [[ "$EXTENSION" != "$VENV/"* ]]; then
    echo "Expected the instrumented wheel in $VENV, got $EXTENSION" >&2
    exit 1
fi
echo "Instrumented extension: $EXTENSION"
nm -D "$EXTENSION" | grep __asan_init
export JSONMODEM_MEMORY_RUNNER="$BUILD/python-memory"
"$BUILD/python-memory" "$VENV/bin/python" -m pytest -q crates/jsonmodem-py/tests
