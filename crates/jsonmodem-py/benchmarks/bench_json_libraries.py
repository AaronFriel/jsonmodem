#!/usr/bin/env python3
"""Reference JSON library benchmarks.

This file is useful for context, regression checks, and allocation summaries,
but it is not the optimization target for Python performance work. Headline
incremental comparisons belong in ``bench_jiter_chunked.py`` and must parse the
same stream of JSON fragments, with jiter measured on every cumulative prefix
when using ``partial_mode=True``.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib
import json
import os
import platform
import sys
import tracemalloc
from pathlib import Path
from typing import Any, Callable

import pyperf


REPO_ROOT = Path(__file__).resolve().parents[3]
DATA_ROOT = REPO_ROOT / "crates" / "jsonmodem" / "benches" / "jiter_data"
DEFAULT_WORKLOADS = (
    "medium_response.json",
    "response_large.json",
    "string_array.json",
    "string_array_unique.json",
    "floats_array.json",
    "massive_ints_array.json",
    "true_object.json",
    "unicode.json",
)
WORKLOAD_ENV = "JSONMODEM_PY_BENCH_WORKLOADS"
GROUP_ENV = "JSONMODEM_PY_BENCH_GROUPS"


def load_optional(module_name: str) -> Any | None:
    try:
        return importlib.import_module(module_name)
    except ImportError:
        return None


def package_version(module: Any | None) -> str:
    if module is None:
        return "missing"
    return str(getattr(module, "__version__", "unknown"))


def stable_hash(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def consume_events(data: bytes) -> int:
    from jsonmodem import JsonModem

    parser = JsonModem()
    count = 0
    text = data.decode("utf-8")
    for _event in parser.feed(text):
        count += 1
    for _event in parser.finish():
        count += 1
    return count


def run_native_decode(decoder: Callable[[bytes], Any], data: bytes) -> int:
    value = decoder(data)
    return len(repr(value))


def measure_tracemalloc(label: str, func: Callable[[bytes], Any], data: bytes) -> tuple[str, int, int]:
    tracemalloc.start()
    func(data)
    current, peak = tracemalloc.get_traced_memory()
    tracemalloc.stop()
    return label, current, peak


def discover_decoders() -> dict[str, Callable[[bytes], Any]]:
    decoders: dict[str, Callable[[bytes], Any]] = {
        "stdlib_json": lambda data: json.loads(data),
    }

    orjson = load_optional("orjson")
    if orjson is not None:
        decoders["orjson"] = orjson.loads

    msgspec = load_optional("msgspec")
    if msgspec is not None:
        decoders["msgspec"] = msgspec.json.decode

    jiter = load_optional("jiter")
    if jiter is not None and hasattr(jiter, "from_json"):
        decoders["jiter"] = jiter.from_json

    rapidjson = load_optional("rapidjson")
    if rapidjson is not None:
        decoders["python_rapidjson"] = rapidjson.loads

    simdjson = load_optional("simdjson")
    if simdjson is not None and hasattr(simdjson, "Parser"):
        parser = simdjson.Parser()
        decoders["pysimdjson"] = lambda data, parser=parser: parser.parse(data, recursive=True)

    ujson = load_optional("ujson")
    if ujson is not None:
        decoders["ujson"] = ujson.loads

    return decoders


def add_metadata(runner: pyperf.Runner, workloads: dict[str, bytes]) -> None:
    optional = {
        "orjson": load_optional("orjson"),
        "msgspec": load_optional("msgspec"),
        "jiter": load_optional("jiter"),
        "rapidjson": load_optional("rapidjson"),
        "simdjson": load_optional("simdjson"),
        "ujson": load_optional("ujson"),
    }
    metadata = {
        "python": sys.version.replace("\n", " "),
        "platform": platform.platform(),
        "jsonmodem_worktree": str(REPO_ROOT),
        "orjson_version": package_version(optional["orjson"]),
        "msgspec_version": package_version(optional["msgspec"]),
        "jiter_version": package_version(optional["jiter"]),
        "python_rapidjson_version": package_version(optional["rapidjson"]),
        "pysimdjson_version": package_version(optional["simdjson"]),
        "ujson_version": package_version(optional["ujson"]),
        "benchmark_method": (
            "reference-only full decode context; not the incremental "
            "jsonmodem optimization target"
        ),
    }
    for name, data in workloads.items():
        metadata[f"workload_{name}_bytes"] = str(len(data))
        metadata[f"workload_{name}_sha256"] = stable_hash(data)
    for key, value in metadata.items():
        runner.metadata[key] = value


def parse_args() -> tuple[argparse.Namespace, list[str]]:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--workload",
        action="append",
        choices=DEFAULT_WORKLOADS,
        help="Run only the named workload. May be passed multiple times.",
    )
    parser.add_argument(
        "--group",
        action="append",
        choices=("events", "native"),
        help="Run only the selected benchmark group. May be passed multiple times.",
    )
    parser.add_argument(
        "--list",
        action="store_true",
        help="Print benchmark names and exit.",
    )
    parser.add_argument(
        "--alloc-summary",
        action="store_true",
        help="Print Python-visible tracemalloc peaks for selected workloads and exit.",
    )
    return parser.parse_known_args()


def main() -> None:
    args, pyperf_args = parse_args()

    if args.workload:
        os.environ[WORKLOAD_ENV] = ",".join(args.workload)
    if args.group:
        os.environ[GROUP_ENV] = ",".join(args.group)
    if (args.workload or args.group) and not any(
        item == "--copy-env" or item.startswith("--inherit-environ") for item in pyperf_args
    ):
        pyperf_args.extend(["--inherit-environ", f"{WORKLOAD_ENV},{GROUP_ENV}"])

    sys.argv = [sys.argv[0], *pyperf_args]

    selected_workloads = tuple(
        item
        for item in os.environ.get(WORKLOAD_ENV, ",".join(DEFAULT_WORKLOADS)).split(",")
        if item
    )
    selected_groups = set(
        item for item in os.environ.get(GROUP_ENV, "events,native").split(",") if item
    )
    workloads = {name: (DATA_ROOT / name).read_bytes() for name in selected_workloads}
    decoders = discover_decoders()

    if args.list:
        for workload in selected_workloads:
            if "events" in selected_groups:
                print(f"jsonmodem_events:{workload}")
            if "native" in selected_groups:
                for name in decoders:
                    print(f"{name}:{workload}")
        return

    if args.alloc_summary:
        decoders_for_alloc: dict[str, Callable[[bytes], Any]] = {
            "jsonmodem_events": consume_events,
            **discover_decoders(),
        }
        for workload_name, data in workloads.items():
            print(f"# {workload_name} bytes={len(data)} sha256={stable_hash(data)}")
            for decoder_name, decoder in decoders_for_alloc.items():
                label, current, peak = measure_tracemalloc(decoder_name, decoder, data)
                print(f"{label}: current={current} peak={peak}")
        return

    runner = pyperf.Runner()
    add_metadata(runner, workloads)

    for workload_name, data in workloads.items():
        if "events" in selected_groups:
            runner.bench_func(
                f"jsonmodem_events:{workload_name}",
                consume_events,
                data,
            )

        if "native" in selected_groups:
            for decoder_name, decoder in decoders.items():
                runner.bench_func(
                    f"{decoder_name}:{workload_name}",
                    run_native_decode,
                    decoder,
                    data,
                )


if __name__ == "__main__":
    main()
