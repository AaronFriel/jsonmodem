#!/usr/bin/env bash
set -euo pipefail

# Render human-readable debug reports for all crash/minimized artifacts
# for the specified fuzz targets. If no targets are provided, autodetect
# from directories under fuzz/artifacts/.
#
# Usage:
#   scripts/fuzz_fmt_all.sh                 # autodetect targets
#   scripts/fuzz_fmt_all.sh fuzz_jsonmodem_buffers fuzz_jsonmodem_dna
#
# Each artifact's formatted output is written next to the artifact as
#   <artifact>.fmt.txt
# and is also printed to the terminal.

FUZZ_DIR="fuzz/artifacts"
FUZZ="cargo +nightly fuzz"

discover_targets() {
  if [[ ! -d "$FUZZ_DIR" ]]; then
    echo "No artifacts directory: $FUZZ_DIR" >&2
    exit 1
  fi
  for d in "$FUZZ_DIR"/*; do
    [[ -d "$d" ]] || continue
    basename "$d"
  done
}

targets=("$@")
if [[ ${#targets[@]} -eq 0 ]]; then
  mapfile -t targets < <(discover_targets)
fi

if [[ ${#targets[@]} -eq 0 ]]; then
  echo "No targets found under $FUZZ_DIR" >&2
  exit 0
fi

shopt -s nullglob

for target in "${targets[@]}"; do
  art_dir="$FUZZ_DIR/$target"
  if [[ ! -d "$art_dir" ]]; then
    echo "[skip] $target: no directory $art_dir" >&2
    continue
  fi

  echo "=== Target: $target ==="
  found=0
  for artifact in "$art_dir"/crash-* "$art_dir"/minimized-* "$art_dir"/oom-* "$art_dir"/timeout-*; do
    [[ -f "$artifact" ]] || continue
    [[ "$artifact" == *.fmt.txt ]] && continue
    found=1
    out="$artifact.fmt.txt"
    echo "--- fmt: $(basename "$artifact") -> $(basename "$out")" | sed 's/^/    /'
    # cargo-fuzz prints to stderr; capture both and tee to file
    if ! $FUZZ fmt "$target" "$artifact" 2>&1 | tee "$out"; then
      echo "    fmt failed for $artifact" >&2
    fi
  done
  if [[ $found -eq 0 ]]; then
    echo "    (no artifacts)"
  fi
done
