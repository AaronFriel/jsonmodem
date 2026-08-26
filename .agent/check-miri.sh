#!/usr/bin/env bash
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
TOOLCHAIN="${JSONMODEM_MEMORY_TOOLCHAIN:-nightly}"
CASES="${JSONMODEM_SAFETY_CASES:-32}"
MODE="${1:-all}"
if [[ "$MODE" != all && "$MODE" != targeted ]]; then
    echo "Usage: $0 [all|targeted]" >&2
    exit 1
fi
if [[ ! "$CASES" =~ ^[1-9][0-9]*$ ]]; then
    echo "JSONMODEM_SAFETY_CASES must be a positive integer" >&2
    exit 1
fi
rustc "+$TOOLCHAIN" -vV
cargo "+$TOOLCHAIN" miri setup
if [[ "$MODE" == all ]]; then
    MIRIFLAGS="-Zmiri-env-set=JSONMODEM_SAFETY_CASES=$CASES" \
        cargo "+$TOOLCHAIN" miri nextest run --workspace --profile default-miri \
        --exclude jsonmodem-fuzz --exclude jsonmodem-py
fi
for model in stacked tree; do
    for seed in 0 1 2; do
        export MIRIFLAGS="-Zmiri-seed=$seed -Zmiri-env-set=JSONMODEM_SAFETY_CASES=$CASES"
        if [[ "$model" == tree ]]; then
            MIRIFLAGS+=" -Zmiri-tree-borrows"
        fi
        echo "Miri model=$model execution_seed=$seed generated_cases=$CASES"
        cargo "+$TOOLCHAIN" miri test -p jsonmodem --lib memory_safety -- --nocapture
        cargo "+$TOOLCHAIN" miri test -p jsonmodem --test memory_safety -- --nocapture
    done
done
