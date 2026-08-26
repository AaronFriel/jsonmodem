#!/usr/bin/env bash
# Unified pre-push / CI check script
set -euo pipefail

###############################################################################
# Repo-specific helpers
###############################################################################
REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
REPO_NAME="$(basename "${REPO_ROOT}")"
FUZZ_CRATE="${REPO_NAME}-fuzz"        # convention: <repo>-fuzz
TIMING_LOG="$(mktemp -t pre-push.timings.XXXX)"
trap 'rm -f "$TIMING_LOG"' EXIT

###############################################################################
# Utility: measure elapsed seconds for each step
###############################################################################
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

###############################################################################
# 1. Formatting
###############################################################################
run_step "rustfmt (apply)"  cargo +nightly fmt --all
run_step "rustfmt (check)"  cargo +nightly fmt --all -- --check

###############################################################################
# 2. Build, test, lint (skip fuzz crate)
###############################################################################
EXCLUDE_ARGS=(--exclude "$FUZZ_CRATE" --exclude jsonmodem-fuzz --exclude jsonmodem-py)
# Speed up local iteration by enabling lighter-weight test and benchmark
# configurations. CI runs without these features for full coverage.
FAST_ENV=(JSONMODEM_TEST_FAST=1 JSONMODEM_BENCH_FAST=1)

run_step "build (release)"  env "${FAST_ENV[@]}" cargo build  --workspace --release              "${EXCLUDE_ARGS[@]}"
run_step "tests"            env "${FAST_ENV[@]}" cargo test   --workspace --verbose              "${EXCLUDE_ARGS[@]}"
run_step "clippy"           env "${FAST_ENV[@]}" cargo clippy --workspace --all-targets          "${EXCLUDE_ARGS[@]}" \
                               -- -D warnings
run_step "docs (public)"    env RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps "${EXCLUDE_ARGS[@]}"

# Extra clippy pass that compiles under the same cfg flags Miri uses.
run_step "clippy (cfg=miri)" \
         env RUSTFLAGS="--cfg miri" \
         env "${FAST_ENV[@]}" cargo clippy --workspace --all-targets "${EXCLUDE_ARGS[@]}" \
           -- -D warnings

###############################################################################
# 3. Optional Miri (cfg via env var)
###############################################################################
if [[ "${AGENT_CHECK_MIRI_DISABLE:-true}" != "true" ]]; then
  run_step "miri tests" bash "$REPO_ROOT/.agent/check-miri.sh"
else
  echo "⚠️  AGENT_CHECK_MIRI_DISABLE=true – skipping Miri checks."
fi

###############################################################################
# 4. GitHub Actions lint
###############################################################################
run_step "actionlint"       actionlint -color

###############################################################################
# 5. Quick fuzz sanity (if crate & target exist)
###############################################################################
if [[ -d "${FUZZ_CRATE}" ]]; then
  FIRST_TARGET="$(find "${FUZZ_CRATE}/fuzz_targets" -maxdepth 1 -name '*.rs' -printf '%f\n' \
                    | head -n1 | sed 's/\.rs$//')"
  if [[ -n "${FIRST_TARGET}" ]]; then
    run_step "fuzz (${FIRST_TARGET})" \
             cargo +nightly fuzz run "${FIRST_TARGET}" -- -runs=5000
  fi
fi

###############################################################################
# 6. Timing summary
###############################################################################
echo -e "\nTiming summary (seconds, highest first):"
sort -nr "$TIMING_LOG"
