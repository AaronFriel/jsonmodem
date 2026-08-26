#!/usr/bin/env python3
"""Compare buffer handling in two release wheels; requires pyperf and memray.

Run from the repository root with --baseline-python and --candidate-python
pointing to environments containing the two wheels. Results are JSON on stdout.
Each of seven pairs alternates which interpreter runs first. Each timing is the
median of three measurements of 200 complete streams. Allocation tracking is a
separate measurement of 100 streams after ten warmup streams.
"""

import argparse
import json
from pathlib import Path
import runpy
import statistics
import subprocess
import sys
import tempfile
import timeit


def measure(mode):
    import memray
    from jsonmodem import JsonModem

    bench = runpy.run_path(str(Path(__file__).with_name("bench_jiter_chunked.py")))
    chunks = bench["chunk_bytes"](bench["make_array_strings"](1024), 512)

    class Exporter:
        """A valid read-only exporter whose owner is not directly visible."""

        def __init__(self, data):
            self.data = data

        def __buffer__(self, flags):
            return memoryview(self.data)

    def byte_events(data):
        parser = JsonModem(byte_views=True)
        count = 0
        for chunk in data:
            count += sum(1 for _ in parser.feed(chunk))
        count += sum(1 for _ in parser.finish())
        return count

    events = bench["run_jsonmodem_events_chunked"]
    cases = {
        "bytes": (events, chunks),
        "bytearray": (events, [bytearray(chunk) for chunk in chunks]),
        "memoryview": (events, [memoryview(chunk) for chunk in chunks]),
        "byte_views_bytes": (byte_events, chunks),
        "byte_views_exporter": (byte_events, [Exporter(chunk) for chunk in chunks]),
    }
    results = {}
    for name, (function, data) in cases.items():
        def run():
            return function(data)

        count = run()
        if mode == "time":
            samples = timeit.repeat(run, number=200, repeat=3)
            results[name] = {"ns": statistics.median(samples) * 1e9 / 200, "events": count}
        else:
            for _ in range(10):
                run()
            with tempfile.TemporaryDirectory() as directory:
                artifact = Path(directory) / "allocations.bin"
                with memray.Tracker(str(artifact), native_traces=True, trace_python_allocators=True):
                    for _ in range(100):
                        run()
                reader = memray.FileReader(str(artifact))
                frees = {memray.AllocatorType.FREE, memray.AllocatorType.PYMALLOC_FREE, memray.AllocatorType.MUNMAP}
                allocations = sum(record.n_allocations for record in reader.get_allocation_records() if record.allocator not in frees)
                results[name] = {
                    "allocations_per_stream": allocations / 100,
                    "peak_tracked_bytes": reader.metadata.peak_memory,
                    "events": count,
                }
    return results


def compare(baseline, candidate):
    interpreters = {"baseline": baseline, "candidate": candidate}

    def invoke(python, mode):
        output = subprocess.check_output([python, __file__, "--worker", mode], text=True)
        return json.loads(output)

    timings = {name: [] for name in interpreters}
    for pair in range(7):
        order = list(interpreters) if pair % 2 == 0 else list(reversed(interpreters))
        for name in order:
            timings[name].append(invoke(interpreters[name], "time"))
    allocations = {name: invoke(python, "memory") for name, python in interpreters.items()}
    summary = {}
    for case in timings["baseline"][0]:
        times = {name: [sample[case]["ns"] for sample in samples] for name, samples in timings.items()}
        ratios = [new / old for new, old in zip(times["candidate"], times["baseline"])]
        counts = {sample[case]["events"] for samples in timings.values() for sample in samples}
        assert len(counts) == 1, f"different event counts for {case}"
        summary[case] = {
            "median_ns": {name: statistics.median(samples) for name, samples in times.items()},
            "paired_ratio_median": statistics.median(ratios),
            "paired_ratio_range": [min(ratios), max(ratios)],
            "allocations": {name: results[case] for name, results in allocations.items()},
        }
    return {"summary": summary, "timings": timings}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline-python")
    parser.add_argument("--candidate-python", default=sys.executable)
    parser.add_argument("--worker", choices=["time", "memory"], help=argparse.SUPPRESS)
    args = parser.parse_args()
    if sys.version_info < (3, 12):
        parser.error("Python 3.12 or later is required for the Python exporter case")
    if args.worker:
        result = measure(args.worker)
    else:
        if not args.baseline_python:
            parser.error("--baseline-python is required")
        result = compare(args.baseline_python, args.candidate_python)
    print(json.dumps(result, indent=2))
