#!/usr/bin/env python3
"""Realistic streaming JSON scenarios.

Primary jsonmodem results in this file should use incremental input fragments:
HTTP-like body chunks, LLM tool-call chunks, NDJSON lines, and nested extraction
streams. Full-decode competitors are included to show application tradeoffs,
but they are reference results and not the target for incremental jsonmodem
optimization.
"""

from __future__ import annotations

import argparse
import asyncio
import hashlib
import importlib
import io
import json
import os
import platform
import sys
from pathlib import Path
from typing import Any, Callable

import pyperf


REPO_ROOT = Path(__file__).resolve().parents[3]
SCENARIO_ENV = "JSONMODEM_PY_REALISTIC_SCENARIOS"
GROUP_ENV = "JSONMODEM_PY_REALISTIC_GROUPS"


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


def chunk_text(data: bytes, size: int) -> list[str]:
    text = data.decode("utf-8")
    return [text[index : index + size] for index in range(0, len(text), size)]


def make_http_nested_response(count: int = 500) -> bytes:
    items = []
    for index in range(count):
        items.append(
            {
                "id": index,
                "metadata": {
                    "etag": f"etag-{index:06d}",
                    "region": "us-west-2",
                    "tags": ["api", "stream", f"bucket-{index % 17}"],
                },
                "metrics": {
                    "latency_ms": index % 997,
                    "size": 1024 + index * 3,
                },
                "body": "x" * 96,
            }
        )
    return json.dumps({"items": items, "next": None}, separators=(",", ":")).encode()


def make_llm_tool_arguments(code_bytes: int = 64 * 1024) -> bytes:
    line = b"print('jsonmodem streaming tool call')\n"
    repeats = code_bytes // len(line) + 1
    code = (line * repeats)[:code_bytes].decode()
    payload = {
        "path": "generated/tool_output.py",
        "language": "python",
        "content": code,
        "metadata": {"model": "example-llm", "finish_reason": None},
    }
    return json.dumps(payload, separators=(",", ":")).encode()


def make_ndjson_logs(count: int = 2000) -> bytes:
    lines = []
    for index in range(count):
        row = {
            "ts": f"2026-06-03T05:{index % 60:02d}:00Z",
            "level": "info" if index % 19 else "warning",
            "service": f"worker-{index % 8}",
            "trace_id": f"{index:032x}",
            "message": "request completed",
            "duration_ms": index % 251,
        }
        lines.append(json.dumps(row, separators=(",", ":")))
    return ("\n".join(lines) + "\n").encode()


def make_deep_nested(count: int = 300) -> bytes:
    records = []
    for index in range(count):
        records.append(
            {
                "outer": {
                    "middle": {
                        "inner": {
                            "target": f"value-{index:06d}",
                            "other": [index, index + 1, index + 2],
                        }
                    }
                }
            }
        )
    return json.dumps({"records": records}, separators=(",", ":")).encode()


def make_har_capture(count: int = 400) -> bytes:
    entries = []
    for index in range(count):
        entries.append(
            {
                "startedDateTime": f"2026-06-03T06:{index % 60:02d}:00.000Z",
                "request": {
                    "method": "GET" if index % 5 else "POST",
                    "url": f"https://api.example.test/v1/items/{index}?region=us-west-2",
                    "headers": [
                        {"name": "Accept", "value": "application/json"},
                        {"name": "Trace-ID", "value": f"{index:032x}"},
                    ],
                },
                "response": {
                    "status": 200 if index % 17 else 500,
                    "content": {
                        "mimeType": "application/json",
                        "text": "{\"ok\":true,\"message\":\"captured response\"}",
                    },
                },
                "timings": {"send": 1, "wait": index % 31, "receive": 2},
            }
        )
    return json.dumps({"log": {"version": "1.2", "entries": entries}}, separators=(",", ":")).encode()


SCENARIOS: dict[str, bytes] = {
    "http_nested_response": make_http_nested_response(),
    "llm_tool_arguments": make_llm_tool_arguments(),
    "ndjson_logs": make_ndjson_logs(),
    "deep_nested": make_deep_nested(),
    "har_capture": make_har_capture(),
}


def jsonmodem_extract_etags(data: bytes) -> int:
    from jsonmodem import JsonModem

    parser = JsonModem()
    total = 0
    for chunk in chunk_bytes(data, 4096):
        for kind, path, payload in parser.feed(chunk):
            if kind == "string" and path.endswith("etag"):
                total += len(payload.fragment)
    for kind, path, payload in parser.finish():
        if kind == "string" and path.endswith("etag"):
            total += len(payload.fragment)
    return total


def jsonmodem_filter_extract_etags(data: bytes) -> int:
    from jsonmodem import JsonModem

    parser = JsonModem(paths="items.*.metadata.etag")
    total = 0
    for chunk in chunk_bytes(data, 4096):
        for _kind, _path, payload in parser.feed(chunk):
            total += len(payload["fragment"])
    for _kind, _path, payload in parser.finish():
        total += len(payload["fragment"])
    return total


def full_decode_extract_etags(decoder: Callable[[bytes], Any], data: bytes) -> int:
    obj = decoder(data)
    return sum(len(item["metadata"]["etag"]) for item in obj["items"])


def ijson_extract_etags(data: bytes) -> int:
    ijson = load_optional("ijson")
    if ijson is None:
        raise RuntimeError("ijson is not installed")
    return sum(len(etag) for etag in ijson.items(io.BytesIO(data), "items.item.metadata.etag"))


def jsonmodem_forward_tool_content(data: bytes) -> int:
    from jsonmodem import JsonModem

    parser = JsonModem(byte_views=True)
    total = 0
    for chunk in chunk_bytes(data, 257):
        for kind, path, payload in parser.feed(chunk):
            if kind == "string" and path[-1:] == (("key", "content"),):
                fragment = payload["fragment"]
                if payload["is_view"]:
                    total += len(fragment)
                else:
                    total += len(fragment.encode())
    for kind, path, payload in parser.finish():
        if kind == "string" and path[-1:] == (("key", "content"),):
            fragment = payload["fragment"]
            total += len(fragment) if payload["is_view"] else len(fragment.encode())
    return total


def jsonmodem_filter_forward_tool_content(data: bytes) -> int:
    from jsonmodem import JsonModem

    parser = JsonModem(paths="content", byte_views=True)
    total = 0
    for chunk in chunk_bytes(data, 257):
        for _kind, _path, payload in parser.feed(chunk):
            fragment = payload["fragment"]
            if payload["is_view"]:
                total += len(fragment)
            else:
                total += len(fragment.encode())
    for _kind, _path, payload in parser.finish():
        fragment = payload["fragment"]
        total += len(fragment) if payload["is_view"] else len(fragment.encode())
    return total


def jsonmodem_owned_tool_content(data: bytes) -> int:
    from jsonmodem import JsonModem

    parser = JsonModem()
    total = 0
    for chunk in chunk_bytes(data, 257):
        for kind, path, payload in parser.feed(chunk):
            if kind == "string" and path.endswith("content"):
                total += len(payload.fragment.encode())
    for kind, path, payload in parser.finish():
        if kind == "string" and path.endswith("content"):
            total += len(payload.fragment.encode())
    return total


def jsonmodem_values_tool_content(data: bytes) -> int:
    from jsonmodem import JsonModemValues

    parser = JsonModemValues()
    total = 0
    for chunk in chunk_bytes(data, 257):
        for _index, view, path, _is_final in parser.feed(chunk):
            if path.endswith("content"):
                total = len(view["content"])
    for _index, view, path, _is_final in parser.finish():
        if path.endswith("content"):
            total = len(view["content"])
    return total


def full_decode_tool_content(decoder: Callable[[bytes], Any], data: bytes) -> int:
    return len(decoder(data)["content"].encode())


async def _jsonriver_collect_content(chunks: list[str]) -> int:
    jsonriver = load_optional("jsonriver")
    if jsonriver is None:
        raise RuntimeError("jsonriver is not installed")

    async def stream() -> Any:
        for chunk in chunks:
            yield chunk

    total = 0
    async for value in jsonriver.parse(stream()):
        if isinstance(value, dict) and "content" in value:
            total = len(value["content"].encode())
    return total


def jsonriver_progressive_tool_content(data: bytes) -> int:
    return asyncio.run(_jsonriver_collect_content(chunk_text(data, 257)))


def partial_json_parser_tool_content(data: bytes) -> int:
    partial_json_parser = load_optional("partial_json_parser")
    if partial_json_parser is None:
        raise RuntimeError("partial-json-parser is not installed")
    total = 0
    buffer = ""
    for chunk in chunk_text(data, 257):
        buffer += chunk
        value = partial_json_parser.loads(buffer)
        if isinstance(value, dict) and "content" in value:
            total = len(value["content"].encode())
    return total


def json_streamer_tool_content(data: bytes) -> int:
    json_streamer = load_optional("json_streamer")
    if json_streamer is None:
        raise RuntimeError("json-streamer is not installed")
    parser = json_streamer.JsonParser()
    total = 0
    for chunk in chunk_text(data, 257):
        for _state, value in parser.parse_part(chunk):
            if isinstance(value, dict) and "content" in value:
                total = len(value["content"].encode())
    return total


def jsonmodem_ndjson(data: bytes) -> int:
    from jsonmodem import JsonModem, ParserOptions

    parser = JsonModem(ParserOptions(allow_multiple=True))
    warnings = 0
    for line in data.splitlines(keepends=True):
        for kind, path, payload in parser.feed(line):
            if kind == "string" and path == (("key", "level"),) and payload.fragment == "warning":
                warnings += 1
    for kind, path, payload in parser.finish():
        if kind == "string" and path == (("key", "level"),) and payload.fragment == "warning":
            warnings += 1
    return warnings


def full_decode_ndjson(decoder: Callable[[bytes], Any], data: bytes) -> int:
    warnings = 0
    for line in data.splitlines():
        if decoder(line)["level"] == "warning":
            warnings += 1
    return warnings


def jsonmodem_deep_nested(data: bytes) -> int:
    from jsonmodem import JsonModem

    parser = JsonModem()
    total = 0
    target_path_tail = (
        ("key", "outer"),
        ("key", "middle"),
        ("key", "inner"),
        ("key", "target"),
    )
    for kind, path, payload in parser.feed(data):
        if kind == "string" and path.endswith(target_path_tail):
            total += len(payload.fragment)
    for kind, path, payload in parser.finish():
        if kind == "string" and path.endswith(target_path_tail):
            total += len(payload.fragment)
    return total


def jsonmodem_filter_deep_nested(data: bytes) -> int:
    from jsonmodem import JsonModem

    parser = JsonModem(paths="records.*.outer.middle.inner.target")
    total = 0
    for chunk in chunk_bytes(data, 4096):
        for _kind, _path, payload in parser.feed(chunk):
            total += len(payload["fragment"])
    for _kind, _path, payload in parser.finish():
        total += len(payload["fragment"])
    return total


def full_decode_deep_nested(decoder: Callable[[bytes], Any], data: bytes) -> int:
    obj = decoder(data)
    return sum(len(item["outer"]["middle"]["inner"]["target"]) for item in obj["records"])


def json_stream_extract_etags(data: bytes) -> int:
    json_stream = load_optional("json_stream")
    if json_stream is None:
        raise RuntimeError("json-stream is not installed")
    root = json_stream.load(io.BytesIO(data))
    for key, items in root.items():
        if key == "items":
            return sum(len(item["metadata"]["etag"]) for item in items)
    return 0


def jsonmodem_har_urls(data: bytes) -> int:
    from jsonmodem import JsonModem

    parser = JsonModem()
    total = 0
    target_path_tail = (("key", "request"), ("key", "url"))
    for chunk in chunk_bytes(data, 4096):
        for kind, path, payload in parser.feed(chunk):
            if kind == "string" and path.endswith(target_path_tail):
                total += len(payload.fragment)
    for kind, path, payload in parser.finish():
        if kind == "string" and path.endswith(target_path_tail):
            total += len(payload.fragment)
    return total


def jsonmodem_filter_har_urls(data: bytes) -> int:
    from jsonmodem import JsonModem

    parser = JsonModem(paths="log.entries.*.request.url")
    total = 0
    for chunk in chunk_bytes(data, 4096):
        for _kind, _path, payload in parser.feed(chunk):
            total += len(payload["fragment"])
    for _kind, _path, payload in parser.finish():
        total += len(payload["fragment"])
    return total


def full_decode_har_urls(decoder: Callable[[bytes], Any], data: bytes) -> int:
    obj = decoder(data)
    return sum(len(entry["request"]["url"]) for entry in obj["log"]["entries"])


def json_stream_har_urls(data: bytes) -> int:
    json_stream = load_optional("json_stream")
    if json_stream is None:
        raise RuntimeError("json-stream is not installed")
    root = json_stream.load(io.BytesIO(data))
    for key, log in root.items():
        if key != "log":
            continue
        for log_key, entries in log.items():
            if log_key == "entries":
                return sum(len(entry["request"]["url"]) for entry in entries)
    return 0


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


def selected_items(env_name: str, default: str) -> list[str]:
    return [item for item in os.environ.get(env_name, default).split(",") if item]


def add_metadata(runner: pyperf.Runner, scenarios: dict[str, bytes]) -> None:
    optional = {
        "ijson": load_optional("ijson"),
        "json_stream": load_optional("json_stream"),
        "jsonriver": load_optional("jsonriver"),
        "partial_json_parser": load_optional("partial_json_parser"),
        "json_streamer": load_optional("json_streamer"),
        "streaming_json_parser": load_optional("streaming_json_parser"),
        "orjson": load_optional("orjson"),
        "msgspec": load_optional("msgspec"),
        "jiter": load_optional("jiter"),
        "rapidjson": load_optional("rapidjson"),
        "simdjson": load_optional("simdjson"),
        "ujson": load_optional("ujson"),
    }
    runner.metadata["python"] = sys.version.replace("\n", " ")
    runner.metadata["platform"] = platform.platform()
    runner.metadata["jsonmodem_worktree"] = str(REPO_ROOT)
    runner.metadata["benchmark_method"] = (
        "realistic fragment-stream scenarios; full-decode competitors are "
        "reference context, not the incremental optimization target"
    )
    for name, module in optional.items():
        runner.metadata[f"{name}_version"] = package_version(module)
    for name, data in scenarios.items():
        runner.metadata[f"scenario_{name}_bytes"] = str(len(data))
        runner.metadata[f"scenario_{name}_sha256"] = stable_hash(data)


def parse_args() -> tuple[argparse.Namespace, list[str]]:
    parser = argparse.ArgumentParser()
    parser.add_argument("--scenario", action="append", choices=tuple(SCENARIOS))
    parser.add_argument(
        "--group",
        action="append",
        choices=("http_extract", "llm_forward", "llm_partial", "ndjson", "deep_nested", "har_extract"),
    )
    parser.add_argument("--list", action="store_true")
    return parser.parse_known_args()


def main() -> None:
    args, pyperf_args = parse_args()

    if args.scenario:
        os.environ[SCENARIO_ENV] = ",".join(args.scenario)
    if args.group:
        os.environ[GROUP_ENV] = ",".join(args.group)
    if (args.scenario or args.group) and not any(
        item == "--copy-env" or item.startswith("--inherit-environ") for item in pyperf_args
    ):
        pyperf_args.extend(["--inherit-environ", f"{SCENARIO_ENV},{GROUP_ENV}"])

    sys.argv = [sys.argv[0], *pyperf_args]

    scenario_names = selected_items(SCENARIO_ENV, ",".join(SCENARIOS))
    groups = set(selected_items(GROUP_ENV, "http_extract,llm_forward,llm_partial,ndjson,deep_nested,har_extract"))
    scenarios = {name: SCENARIOS[name] for name in scenario_names}
    decoders = discover_decoders()
    benches: list[tuple[str, Callable[[], int]]] = []

    if "http_nested_response" in scenarios and "http_extract" in groups:
        data = scenarios["http_nested_response"]
        benches.append(("jsonmodem_events:http_nested_extract", lambda data=data: jsonmodem_extract_etags(data)))
        benches.append(("jsonmodem_pathfilter:http_nested_extract", lambda data=data: jsonmodem_filter_extract_etags(data)))
        if load_optional("ijson") is not None:
            benches.append(("ijson_items:http_nested_extract", lambda data=data: ijson_extract_etags(data)))
        if load_optional("json_stream") is not None:
            benches.append(("json_stream:http_nested_extract", lambda data=data: json_stream_extract_etags(data)))
        for name, decoder in decoders.items():
            benches.append((f"{name}:http_nested_extract", lambda decoder=decoder, data=data: full_decode_extract_etags(decoder, data)))

    if "llm_tool_arguments" in scenarios and "llm_forward" in groups:
        data = scenarios["llm_tool_arguments"]
        benches.append(("jsonmodem_pathfilter_byteviews:llm_forward_content", lambda data=data: jsonmodem_filter_forward_tool_content(data)))
        benches.append(("jsonmodem_byteviews:llm_forward_content", lambda data=data: jsonmodem_forward_tool_content(data)))
        benches.append(("jsonmodem_events:llm_forward_content", lambda data=data: jsonmodem_owned_tool_content(data)))
        for name, decoder in decoders.items():
            benches.append((f"{name}:llm_forward_content", lambda decoder=decoder, data=data: full_decode_tool_content(decoder, data)))

    if "llm_tool_arguments" in scenarios and "llm_partial" in groups:
        data = scenarios["llm_tool_arguments"]
        benches.append(("jsonmodem_pathfilter_byteviews:llm_partial_content", lambda data=data: jsonmodem_filter_forward_tool_content(data)))
        benches.append(("jsonmodem_values:llm_partial_content", lambda data=data: jsonmodem_values_tool_content(data)))
        if load_optional("jsonriver") is not None:
            benches.append(("jsonriver:llm_partial_content", lambda data=data: jsonriver_progressive_tool_content(data)))
        if load_optional("partial_json_parser") is not None:
            benches.append(("partial_json_parser:llm_partial_content", lambda data=data: partial_json_parser_tool_content(data)))
        if load_optional("json_streamer") is not None:
            benches.append(("json_streamer:llm_partial_content", lambda data=data: json_streamer_tool_content(data)))
    if "ndjson_logs" in scenarios and "ndjson" in groups:
        data = scenarios["ndjson_logs"]
        benches.append(("jsonmodem_events:ndjson_warning_count", lambda data=data: jsonmodem_ndjson(data)))
        for name, decoder in decoders.items():
            benches.append((f"{name}:ndjson_warning_count", lambda decoder=decoder, data=data: full_decode_ndjson(decoder, data)))

    if "deep_nested" in scenarios and "deep_nested" in groups:
        data = scenarios["deep_nested"]
        benches.append(("jsonmodem_events:deep_nested_target", lambda data=data: jsonmodem_deep_nested(data)))
        benches.append(("jsonmodem_pathfilter:deep_nested_target", lambda data=data: jsonmodem_filter_deep_nested(data)))
        for name, decoder in decoders.items():
            benches.append((f"{name}:deep_nested_target", lambda decoder=decoder, data=data: full_decode_deep_nested(decoder, data)))

    if "har_capture" in scenarios and "har_extract" in groups:
        data = scenarios["har_capture"]
        benches.append(("jsonmodem_events:har_request_urls", lambda data=data: jsonmodem_har_urls(data)))
        benches.append(("jsonmodem_pathfilter:har_request_urls", lambda data=data: jsonmodem_filter_har_urls(data)))
        if load_optional("json_stream") is not None:
            benches.append(("json_stream:har_request_urls", lambda data=data: json_stream_har_urls(data)))
        for name, decoder in decoders.items():
            benches.append((f"{name}:har_request_urls", lambda decoder=decoder, data=data: full_decode_har_urls(decoder, data)))

    if args.list:
        for name, _func in benches:
            print(name)
        return

    runner = pyperf.Runner()
    add_metadata(runner, scenarios)
    for name, func in benches:
        runner.bench_func(name, func)


if __name__ == "__main__":
    main()
