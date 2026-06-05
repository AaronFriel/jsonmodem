#!/usr/bin/env python3
"""Fair incremental JSON stream benchmarks.

The primary comparison in this file is a stream of JSON fragments.  For each
document workload, jsonmodem consumes every fragment through its incremental
API, while jiter is measured by reparsing every cumulative prefix with
``partial_mode=True``.  Reassembled full-document decodes are available only
through the ``reference`` group and are not optimization targets for jsonmodem.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib
import json
import os
import platform
import sys
from pathlib import Path
from typing import Any, Callable

import pyperf


REPO_ROOT = Path(__file__).resolve().parents[3]
DATA_ROOT = REPO_ROOT / "crates" / "jsonmodem" / "benches" / "jiter_data"
DOC_WORKLOADS = ("medium_response.json", "response_large.json")
ARRAY_WORKLOADS = ("array_strings_1024", "array_strings_4096", "array_strings_16384")
SEQUENCE_WORKLOADS = ("sequence_medium", "sequence_large")
PARTIAL_VALUE_WORKLOADS = DOC_WORKLOADS + ARRAY_WORKLOADS
WORKLOAD_ENV = "JSONMODEM_PY_JITER_CHUNKED_WORKLOADS"
GROUP_ENV = "JSONMODEM_PY_JITER_CHUNKED_GROUPS"
CHUNK_ENV = "JSONMODEM_PY_JITER_CHUNKED_SIZE"


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


def chunk_bytes(data: bytes, size: int) -> list[bytes]:
    return [data[index : index + size] for index in range(0, len(data), size)]


def make_sequence(count: int) -> bytes:
    lines = []
    for index in range(count):
        row = {
            "id": index,
            "kind": "event" if index % 5 else "checkpoint",
            "path": f"/v1/items/{index}",
            "metadata": {
                "etag": f"etag-{index:06d}",
                "region": "us-west-2",
                "attempt": index % 7,
            },
            "body": "x" * 128,
        }
        lines.append(json.dumps(row, separators=(",", ":")).encode())
    return b"\n".join(lines) + b"\n"


def make_array_strings(count: int) -> bytes:
    return json.dumps(["abcd"] * count, separators=(",", ":")).encode()


def load_workloads() -> dict[str, bytes]:
    return {
        "medium_response.json": (DATA_ROOT / "medium_response.json").read_bytes(),
        "response_large.json": (DATA_ROOT / "response_large.json").read_bytes(),
        "array_strings_1024": make_array_strings(1024),
        "array_strings_4096": make_array_strings(4096),
        "array_strings_16384": make_array_strings(16384),
        "sequence_medium": make_sequence(500),
        "sequence_large": make_sequence(2000),
    }


def run_jiter_reassembled(chunks: list[bytes]) -> int:
    jiter = load_optional("jiter")
    if jiter is None:
        raise RuntimeError("jiter is not installed")
    value = jiter.from_json(b"".join(chunks))
    return len(repr(value))


def run_jiter_cumulative_partial_prefixes(chunks: list[bytes]) -> int:
    """Parse every cumulative document prefix with jiter partial mode.

    This is the apples-to-apples comparison for jsonmodem's incremental feed:
    both parsers are asked to process the same number of incoming fragments and
    surface the best partial result they can after each fragment.
    """

    jiter = load_optional("jiter")
    if jiter is None:
        raise RuntimeError("jiter is not installed")

    pending = bytearray()
    total = 0
    for chunk in chunks:
        pending.extend(chunk)
        value = jiter.from_json(bytes(pending), partial_mode=True)
        total += len(repr(value))
    return total


def run_jsonmodem_events_chunked(chunks: list[bytes]) -> int:
    from jsonmodem import JsonModem

    parser = JsonModem()
    count = 0
    for chunk in chunks:
        for _event in parser.feed(chunk):
            count += 1
    for _event in parser.finish():
        count += 1
    return count


def run_jsonmodem_feed_chunks_chunked(chunks: list[bytes]) -> int:
    from jsonmodem import JsonModem

    parser = JsonModem()
    count = 0
    for _event in parser.feed(chunks):
        count += 1
    for _event in parser.finish():
        count += 1
    return count


def run_jsonmodem_values_chunked(chunks: list[bytes]) -> int:
    from jsonmodem import JsonModemValues

    parser = JsonModemValues()
    total = 0
    for chunk in chunks:
        for _index, view, path, is_final in parser.feed(chunk):
            total += len(path)
            total += len(view.kind)
            total += int(is_final)
    for _index, view, path, is_final in parser.finish():
        total += len(path)
        total += len(view.kind)
        total += int(is_final)
    return total


def run_jsonmodem_values_feed_chunks(chunks: list[bytes]) -> int:
    from jsonmodem import JsonModemValues

    parser = JsonModemValues()
    total = 0
    for _index, view, path, is_final in parser.feed(chunks):
        total += len(path)
        total += len(view.kind)
        total += int(is_final)
    for _index, view, path, is_final in parser.finish():
        total += len(path)
        total += len(view.kind)
        total += int(is_final)
    return total


def run_jsonmodem_values_view_prefixes(chunks: list[bytes]) -> int:
    """Feed every fragment and snapshot the current value after every feed.

    This is the strict comparison against parsers that return a value for every
    cumulative prefix, even when the parser did not produce a new value event.
    """

    from jsonmodem import JsonModemValues

    parser = JsonModemValues()
    total = 0
    for chunk in chunks:
        for _update in parser.feed(chunk):
            pass
        total += len(repr(parser.view()))
    for _update in parser.finish():
        pass
    return total


def run_jsonmodem_sequence_chunked(chunks: list[bytes]) -> int:
    from jsonmodem import JsonModem, ParserOptions

    parser = JsonModem(ParserOptions(allow_multiple=True))
    count = 0
    for chunk in chunks:
        for kind, path, _payload in parser.feed(chunk):
            if kind == "object_end" and not path:
                count += 1
    for kind, path, _payload in parser.finish():
        if kind == "object_end" and not path:
            count += 1
    return count


def run_jsonmodem_sequence_feed_chunks(chunks: list[bytes]) -> int:
    from jsonmodem import JsonModem, ParserOptions

    parser = JsonModem(ParserOptions(allow_multiple=True))
    count = 0
    for kind, path, _payload in parser.feed(chunks):
        if kind == "object_end" and not path:
            count += 1
    for kind, path, _payload in parser.finish():
        if kind == "object_end" and not path:
            count += 1
    return count


def run_jiter_sequence_buffered_lines(chunks: list[bytes]) -> int:
    jiter = load_optional("jiter")
    if jiter is None:
        raise RuntimeError("jiter is not installed")

    pending = bytearray()
    count = 0
    for chunk in chunks:
        pending.extend(chunk)
        while True:
            newline = pending.find(b"\n")
            if newline < 0:
                break
            line = bytes(pending[:newline])
            del pending[: newline + 1]
            if line:
                jiter.from_json(line)
                count += 1
    if pending:
        jiter.from_json(bytes(pending))
        count += 1
    return count


def run_jiter_sequence_partial_first(chunks: list[bytes]) -> int:
    jiter = load_optional("jiter")
    if jiter is None:
        raise RuntimeError("jiter is not installed")
    value = jiter.from_json(b"".join(chunks), partial_mode=True)
    return len(repr(value))


def add_metadata(runner: pyperf.Runner, workloads: dict[str, bytes], chunk_size: int) -> None:
    jiter = load_optional("jiter")
    runner.metadata["python"] = sys.version.replace("\n", " ")
    runner.metadata["platform"] = platform.platform()
    runner.metadata["jsonmodem_worktree"] = str(REPO_ROOT)
    runner.metadata["jiter_version"] = package_version(jiter)
    runner.metadata["chunk_size_bytes"] = str(chunk_size)
    runner.metadata["benchmark_method"] = (
        "primary results parse every stream fragment; jiter document results "
        "use cumulative prefixes with partial_mode=True; reassembled "
        "full-document decoders are reference-only; JsonModemValues results "
        "emit a reused read-only root view and changed PathView paths; "
        "jsonmodem_values_view_prefixes snapshots after every fragment as a "
        "full-materialization control"
    )
    for name, data in workloads.items():
        runner.metadata[f"workload_{name}_bytes"] = str(len(data))
        runner.metadata[f"workload_{name}_sha256"] = stable_hash(data)


def parse_args() -> tuple[argparse.Namespace, list[str]]:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--workload",
        action="append",
        choices=PARTIAL_VALUE_WORKLOADS + SEQUENCE_WORKLOADS,
    )
    parser.add_argument(
        "--group",
        action="append",
        choices=("documents", "partial_values", "sequences", "reference"),
    )
    parser.add_argument("--chunk-size", type=int)
    parser.add_argument("--list", action="store_true")
    return parser.parse_known_args()


def main() -> None:
    args, pyperf_args = parse_args()

    if args.workload:
        os.environ[WORKLOAD_ENV] = ",".join(args.workload)
    if args.group:
        os.environ[GROUP_ENV] = ",".join(args.group)
    if args.chunk_size is not None:
        os.environ[CHUNK_ENV] = str(args.chunk_size)
    if (args.workload or args.group or args.chunk_size is not None) and not any(
        item == "--copy-env" or item.startswith("--inherit-environ") for item in pyperf_args
    ):
        pyperf_args.extend(["--inherit-environ", f"{WORKLOAD_ENV},{GROUP_ENV},{CHUNK_ENV}"])

    sys.argv = [sys.argv[0], *pyperf_args]

    selected_workloads = [
        item
        for item in os.environ.get(
            WORKLOAD_ENV,
            ",".join((*DOC_WORKLOADS, *SEQUENCE_WORKLOADS)),
        ).split(",")
        if item
    ]
    selected_groups = set(item for item in os.environ.get(GROUP_ENV, "documents,sequences").split(",") if item)
    chunk_size = int(os.environ.get(CHUNK_ENV, "64"))
    all_workloads = load_workloads()
    workloads = {name: all_workloads[name] for name in selected_workloads}

    benches: list[tuple[str, Callable[[], int]]] = []
    for name, data in workloads.items():
        chunks = chunk_bytes(data, chunk_size)
        if name in DOC_WORKLOADS and "documents" in selected_groups:
            benches.append((f"jsonmodem_events_chunked:{name}", lambda chunks=chunks: run_jsonmodem_events_chunked(chunks)))
            benches.append((f"jsonmodem_feed_chunks_chunked:{name}", lambda chunks=chunks: run_jsonmodem_feed_chunks_chunked(chunks)))
            benches.append((f"jiter_cumulative_partial_prefixes:{name}", lambda chunks=chunks: run_jiter_cumulative_partial_prefixes(chunks)))
        if name in PARTIAL_VALUE_WORKLOADS and "partial_values" in selected_groups:
            benches.append((f"jsonmodem_values_chunked:{name}", lambda chunks=chunks: run_jsonmodem_values_chunked(chunks)))
            benches.append((f"jsonmodem_values_feed_chunks:{name}", lambda chunks=chunks: run_jsonmodem_values_feed_chunks(chunks)))
            benches.append((f"jsonmodem_values_view_prefixes:{name}", lambda chunks=chunks: run_jsonmodem_values_view_prefixes(chunks)))
            benches.append((f"jiter_cumulative_partial_prefixes:{name}", lambda chunks=chunks: run_jiter_cumulative_partial_prefixes(chunks)))
        if name in DOC_WORKLOADS and "reference" in selected_groups:
            benches.append((f"reference_jiter_reassembled:{name}", lambda chunks=chunks: run_jiter_reassembled(chunks)))
        if name in SEQUENCE_WORKLOADS and "sequences" in selected_groups:
            benches.append((f"jsonmodem_sequence_chunked:{name}", lambda chunks=chunks: run_jsonmodem_sequence_chunked(chunks)))
            benches.append((f"jsonmodem_sequence_feed_chunks:{name}", lambda chunks=chunks: run_jsonmodem_sequence_feed_chunks(chunks)))
            benches.append((f"jiter_sequence_buffered_lines:{name}", lambda chunks=chunks: run_jiter_sequence_buffered_lines(chunks)))
        if name in SEQUENCE_WORKLOADS and "reference" in selected_groups:
            benches.append((f"reference_jiter_sequence_partial_first:{name}", lambda chunks=chunks: run_jiter_sequence_partial_first(chunks)))

    if args.list:
        for name, _func in benches:
            print(name)
        return

    runner = pyperf.Runner()
    add_metadata(runner, workloads, chunk_size)
    for name, func in benches:
        runner.bench_func(name, func)


if __name__ == "__main__":
    main()
