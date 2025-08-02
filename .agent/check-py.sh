#!/usr/bin/env bash
# Smoke test for the Python bindings
set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
TIMING_LOG="$(mktemp -t check-py.timings.XXXX)"
trap 'rm -f "$TIMING_LOG"' EXIT

source "$REPO_ROOT/.venv/bin/activate"

run_step() {
  local label="$1"; shift
  local start end
  start="$(date +%s)"
  echo "▶︎ $label"
  "$@"
  end="$(date +%s)"
  local sec=$(( end - start ))
  printf "%3ds  %s\n" "$sec" "$label" | tee -a "$TIMING_LOG"
}

run_step "build bindings" maturin develop -m crates/jsonmodem-py/Cargo.toml --release
# Some maturin versions ignore `module-name`; create a compat symlink
SITE_DIR="$(python -c 'import sysconfig, pathlib; print(pathlib.Path(sysconfig.get_paths()["purelib"]))')"
if [[ -d "$SITE_DIR/jsonmodem_py" && ! -e "$SITE_DIR/jsonmodem" ]]; then
  ln -s jsonmodem_py "$SITE_DIR/jsonmodem"
  EXT="$(ls "$SITE_DIR/jsonmodem_py"/jsonmodem_py*.so 2>/dev/null | head -n1)"
  if [[ -n "$EXT" ]]; then
    mv "$EXT" "$SITE_DIR/jsonmodem/$(basename "$EXT" | sed 's/jsonmodem_py/jsonmodem/')"
    echo 'from .jsonmodem import *' > "$SITE_DIR/jsonmodem/__init__.py"
  fi
fi
run_step "python tests" python -m pytest -q crates/jsonmodem-py/tests

echo -e "\nTiming summary (seconds, highest first):"
sort -nr "$TIMING_LOG"
