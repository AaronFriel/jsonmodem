#!/usr/bin/env python3
"""RSS and tracemalloc helper for customer requirement benchmarks.

Timing and memory instrumentation are intentionally separate.  This script runs
one benchmark function per fresh subprocess, records Python-visible peak heap
from ``tracemalloc``, and records process peak RSS through ``resource``.
"""

from __future__ import annotations

import argparse
import gc
import json
import os
import resource
import subprocess
import sys
import tracemalloc
from pathlib import Path
from typing import Callable

import bench_customer_requirements as customer


REPO_ROOT = Path(__file__).resolve().parents[3]


def memory_cases() -> dict[str, Callable[[], int]]:
    fixtures = customer.FIXTURES
    return {
        "D_live_values_updates": lambda: customer.run_jsonmodem_values(
            fixtures["D_live_values_nested_256_64b_g8"],
            snapshot_each_feed=False,
            read_view_each_feed=False,
        ),
        "D_live_values_snapshot_each_feed": lambda: customer.run_jsonmodem_values(
            fixtures["D_live_values_nested_256_64b_g8"],
            snapshot_each_feed=True,
            read_view_each_feed=True,
        ),
        "D_live_values_no_notify_optional": lambda: customer.run_live_values_no_notify_if_available(
            fixtures["D_live_values_nested_256_64b_g8"],
            snapshot_each_feed=False,
            read_view_each_feed=True,
        ),
        "H_no_match_selected": lambda: customer.run_jsonmodem_selected(
            fixtures["H_no_match_items_2000_512b_g20"], "iterable_list"
        ),
        "H_no_match_unfiltered": lambda: customer.run_jsonmodem_unfiltered(
            fixtures["H_no_match_items_2000_512b_g20"], "iterable_list"
        ),
        "E_completed_markers": lambda: customer.run_completed_subtree_markers(
            fixtures["E_completed_items_1000_1k_128b_g16"]
        ),
        "E_completed_values_optional": lambda: customer.run_jsonmodem_subtrees_if_available(
            fixtures["E_completed_items_1000_1k_128b_g16"]
        ),
        "F_byteviews_contiguous": lambda: customer.run_jsonmodem_byteviews(
            fixtures["F_byteviews_contiguous_64k"], "iterable_list"
        ),
        "F_byteviews_cross_buffer": lambda: customer.run_jsonmodem_byteviews(
            fixtures["F_byteviews_cross_buffer_257b_g10"], "iterable_list"
        ),
        "F_byteviews_escaped_unicode": lambda: customer.run_jsonmodem_byteviews(
            fixtures["F_byteviews_escaped_unicode_17b_g10"], "iterable_list"
        ),
        "G_truncated_after_complete": lambda: customer.run_error_case(
            fixtures["G_truncated_string_after_complete"]
        ),
        "G_invalid_byte_middle": lambda: customer.run_error_case(
            fixtures["G_invalid_byte_middle"]
        ),
        "J_completed_markers": lambda: customer.run_completed_subtree_markers(
            fixtures["J_memory_items_10000_128b_256b_g20"]
        ),
        "J_completed_values_optional": lambda: customer.run_jsonmodem_subtrees_if_available(
            fixtures["J_memory_items_10000_128b_256b_g20"]
        ),
        "J_full_decode_stdlib": lambda: customer.run_full_decode(
            fixtures["J_memory_items_10000_128b_256b_g20"], "stdlib_json"
        ),
    }


def ru_maxrss_bytes() -> int:
    value = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    if sys.platform == "darwin":
        return int(value)
    return int(value) * 1024


def run_worker(case: str) -> None:
    cases = memory_cases()
    if case == "idle":
        gc.collect()
        print(
            json.dumps(
                {
                    "case": "idle",
                    "status": "ok",
                    "checksum": 0,
                    "tracemalloc_current": 0,
                    "tracemalloc_peak": 0,
                    "ru_maxrss_bytes": ru_maxrss_bytes(),
                },
                sort_keys=True,
            )
        )
        return
    func = cases[case]
    gc.collect()
    tracemalloc.start()
    status = "ok"
    error = ""
    try:
        checksum = func()
    except customer.MissingFeature as exc:
        status = "skipped"
        error = str(exc)
        checksum = 0
    except Exception as exc:
        status = "error"
        error = f"{type(exc).__name__}: {exc}"
        checksum = 0
    current, peak = tracemalloc.get_traced_memory()
    tracemalloc.stop()
    gc.collect()
    print(
        json.dumps(
            {
                "case": case,
                "status": status,
                "error": error,
                "checksum": checksum,
                "tracemalloc_current": current,
                "tracemalloc_peak": peak,
                "ru_maxrss_bytes": ru_maxrss_bytes(),
            },
            sort_keys=True,
        )
    )


def run_parent(cases: list[str], output: Path | None) -> None:
    rows = []
    baseline = run_child("idle")
    rows.append(baseline)
    baseline_rss = int(baseline["ru_maxrss_bytes"])
    for case in cases:
        row = run_child(case)
        row["ru_maxrss_above_idle_bytes"] = int(row["ru_maxrss_bytes"]) - baseline_rss
        rows.append(row)
        print(json.dumps(row, sort_keys=True))
    if output is not None:
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(json.dumps(rows, indent=2, sort_keys=True) + "\n")


def run_child(case: str) -> dict[str, object]:
    completed = subprocess.run(
        [sys.executable, __file__, "--worker", case],
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=os.environ.copy(),
        cwd=REPO_ROOT,
    )
    return json.loads(completed.stdout)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--case", action="append", choices=tuple(memory_cases()))
    parser.add_argument("--list", action="store_true")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--worker", choices=("idle", *tuple(memory_cases())))
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    if args.list:
        for name in memory_cases():
            print(name)
        return
    if args.worker:
        run_worker(args.worker)
        return
    run_parent(args.case or list(memory_cases()), args.output)


if __name__ == "__main__":
    main()
