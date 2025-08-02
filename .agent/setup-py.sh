#!/usr/bin/env bash
set -euxo pipefail

# Idempotent setup for Python bindings development
REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SETUP_DONE_FILE="${SCRIPT_DIR}/.setup_py_done"
[[ -f "$SETUP_DONE_FILE" ]] && { echo "✅ Python environment already set up."; exit 0; }

# Install uv if missing
if ! command -v uv >/dev/null 2>&1; then
  curl -Ls https://astral.sh/uv/install.sh | sh
fi

# Create and activate virtual environment
uv venv .venv
source .venv/bin/activate

# Install maturin and pytest
uv pip install --upgrade maturin pytest

# Build the Python extension into the venv
maturin develop -m crates/jsonmodem-py/Cargo.toml --release

touch "$SETUP_DONE_FILE"
echo "✅ Python environment ready."
