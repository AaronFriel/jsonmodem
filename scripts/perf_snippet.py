#!/usr/bin/env python3
"""Extract hot lines from a perf report and print surrounding code.

The script prints results to stdout. Redirect or pipe the output as needed."""

from __future__ import annotations

import linecache
import re
import sys
from pathlib import Path


def main() -> None:
    report_path = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("perf_report.txt")
    max_entries = int(sys.argv[2]) if len(sys.argv) > 2 else 10

    pattern = re.compile(r"(\d+\.\d+%)\s+.*\s+([^\s:]+\.rs):(\d+)")
    entries: list[tuple[str, Path, int]] = []

    with report_path.open() as f:
        for line in f:
            m = pattern.search(line)
            if m:
                pct, file, line_no = m.group(1), Path(m.group(2)), int(m.group(3))
                entries.append((pct, file, line_no))
                if len(entries) >= max_entries:
                    break

    for pct, file, line_no in entries:
        print(f"{pct} {file}:{line_no}")
        for i in range(line_no - 1, line_no + 2):
            if i <= 0:
                continue
            text = linecache.getline(str(file), i)
            if text:
                print(f"{i:6}: {text}", end="")
        print()


if __name__ == "__main__":
    main()
