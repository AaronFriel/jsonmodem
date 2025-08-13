#!/usr/bin/env bash
set -euo pipefail

echo "Running core benchmark suite"
cargo bench --bench streaming_json_strategies --bench streaming_json_medium \
  --bench streaming_json_large --bench streaming_json_incremental --bench streaming_parser

echo "Running comparison benchmarks"
JSONMODEM_BENCH_COMPARISON=1 cargo bench --bench competitive_benchmarks
