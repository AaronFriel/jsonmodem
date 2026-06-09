#!/usr/bin/env python3
"""Customer requirements benchmark suite for incremental JSON parsing.

This file is organized around the customer benchmarks A through K in
``pasted-text.txt``.  The benchmark names include the output semantics being
measured so selected streaming deltas, cumulative-prefix partial values,
full-value decoders, byte payloads, and live values are not presented as the
same workload.
"""

from __future__ import annotations

import argparse
import gc
import hashlib
import importlib
import json
import os
import platform
import random
import subprocess
import sys
import time
import zlib
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Iterable, Iterator, Sequence

import pyperf


REPO_ROOT = Path(__file__).resolve().parents[3]
BENCH_ENV = "JSONMODEM_CUSTOMER_BENCHES"
FAMILY_ENV = "JSONMODEM_CUSTOMER_FAMILIES"
GROUP_ENV = "JSONMODEM_CUSTOMER_GROUPS"
WORKTREE_ENV = "JSONMODEM_CUSTOMER_WORKTREE"
PARSER_ENV = "JSONMODEM_CUSTOMER_PARSERS"
ALL_FAMILIES = tuple("ABCDEFGHIJK")
DEFAULT_FAMILIES = ",".join(ALL_FAMILIES)


class MissingFeature(RuntimeError):
    """Raised when a benchmark needs an API that the installed package lacks."""


@dataclass(frozen=True)
class Fixture:
    name: str
    family: str
    data: bytes
    chunks: tuple[bytes, ...]
    feed_group: int
    paths: tuple[str, ...]
    extractor: str
    description: str


@dataclass(frozen=True)
class TimedChunk:
    at_ms: float
    data: bytes


@dataclass(frozen=True)
class Benchmark:
    name: str
    group: str
    family: str
    semantics: str
    run: Callable[[], int]


class Checksum:
    def __init__(self) -> None:
        self.crc = 0
        self.count = 0

    def add(self, *parts: object) -> None:
        for part in parts:
            if isinstance(part, bytes):
                data = part
            elif isinstance(part, memoryview):
                data = part.tobytes()
            else:
                data = str(part).encode("utf-8", "surrogatepass")
            self.crc = zlib.crc32(data, self.crc)
            self.count += 1

    def result(self) -> int:
        return (self.crc & 0xFFFF_FFFF) ^ ((self.count * 1_000_003) & 0xFFFF_FFFF)


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


def command_output(args: Sequence[str], cwd: Path | None = None) -> str:
    try:
        completed = subprocess.run(
            args,
            cwd=cwd,
            check=False,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
        )
    except OSError:
        return "unavailable"
    return completed.stdout.strip().replace("\n", " ") or "unavailable"


def fixed_chunks(data: bytes, size: int) -> tuple[bytes, ...]:
    return tuple(data[index : index + size] for index in range(0, len(data), size))


def variable_chunks(data: bytes, seed: int, low: int = 1, high: int = 12) -> tuple[bytes, ...]:
    rng = random.Random(seed)
    chunks: list[bytes] = []
    index = 0
    while index < len(data):
        size = rng.randint(low, high)
        chunks.append(data[index : index + size])
        index += size
    return tuple(chunks)


def grouped(chunks: Sequence[bytes], group_size: int) -> Iterator[tuple[bytes, ...]]:
    for index in range(0, len(chunks), group_size):
        yield tuple(chunks[index : index + group_size])


def cumulative_prefixes(chunks: Sequence[bytes], repeat_every: int | None = None) -> tuple[bytes, ...]:
    pending = bytearray()
    prefixes: list[bytes] = []
    for index, chunk in enumerate(chunks, start=1):
        pending.extend(chunk)
        prefix = bytes(pending)
        prefixes.append(prefix)
        if repeat_every and index % repeat_every == 0:
            prefixes.append(prefix)
    return tuple(prefixes)


def ascii_text(size: int) -> str:
    pattern = "abcdefghijklmnopqrstuvwxyz0123456789 "
    return (pattern * ((size // len(pattern)) + 1))[:size]


def mixed_text(size: int) -> str:
    pattern = 'alpha café 🙂 newline\nquote " slash \\ snowman ☃ tab\t'
    text = pattern * ((size // len(pattern)) + 1)
    return text[:size]


def content_doc(content: str) -> bytes:
    doc = {
        "metadata": {"id": 1, "source": "fixture"},
        "content": content,
        "tail": {"done": True},
    }
    return json.dumps(doc, ensure_ascii=False, separators=(",", ":")).encode("utf-8")


def items_doc(count: int, text_len: int, duplicate_keys: bool = False) -> bytes:
    if not duplicate_keys:
        items = [
            {"text": ascii_text(text_len), "id": index, "kind": "row"}
            for index in range(count)
        ]
        return json.dumps({"items": items}, separators=(",", ":")).encode()

    entries = []
    value = json.dumps(ascii_text(text_len), separators=(",", ":"))
    for index in range(count):
        entries.append(f'{{"text":{value},"id":{index},"text":{value}}}')
    return ('{"items":[' + ",".join(entries) + "]}").encode()


def completed_items_doc(count: int, item_size: int, truncated: bool = False) -> bytes:
    filler = ascii_text(max(item_size - 64, 0))
    items = [
        {
            "id": index,
            "kind": "record",
            "payload": filler,
            "nested": {"ok": True, "rank": index % 17},
        }
        for index in range(count)
    ]
    data = json.dumps({"items": items, "tail": "complete"}, separators=(",", ":")).encode()
    if truncated:
        return data[:-17]
    return data


def live_value_doc(count: int = 256) -> bytes:
    rows = []
    for index in range(count):
        rows.append(
            {
                "id": index,
                "status": "ready" if index % 7 else "pending",
                "message": f"value-{index:06d}",
            }
        )
    return json.dumps({"rows": rows, "summary": {"count": count}}, separators=(",", ":")).encode()


def no_match_doc(count: int = 2_000) -> bytes:
    rows = []
    for index in range(count):
        rows.append(
            {
                "metadata": {"id": index, "etag": f"etag-{index:06d}"},
                "body": ascii_text(96),
            }
        )
    return json.dumps({"items": rows, "next": None}, separators=(",", ":")).encode()


def make_timed_trace(chunks: Sequence[bytes], mean_ms: float, jitter: bool) -> tuple[TimedChunk, ...]:
    rng = random.Random(0xC0FFEE)
    at_ms = 0.0
    trace: list[TimedChunk] = []
    for chunk in chunks:
        if jitter:
            at_ms += rng.uniform(mean_ms * 0.25, mean_ms * 1.75)
        else:
            at_ms += mean_ms
        trace.append(TimedChunk(at_ms=at_ms, data=chunk))
    return tuple(trace)


def batch_trace(
    trace: Sequence[TimedChunk],
    window_ms: float,
    max_bytes: int,
    max_chunks: int,
) -> tuple[tuple[bytes, ...], ...]:
    batches: list[list[bytes]] = []
    current: list[bytes] = []
    current_bytes = 0
    batch_start = 0.0
    for item in trace:
        if not current:
            batch_start = item.at_ms
        would_exceed_time = current and item.at_ms - batch_start > window_ms
        would_exceed_bytes = current and current_bytes + len(item.data) > max_bytes
        would_exceed_chunks = current and len(current) >= max_chunks
        if would_exceed_time or would_exceed_bytes or would_exceed_chunks:
            batches.append(current)
            current = []
            current_bytes = 0
            batch_start = item.at_ms
        current.append(item.data)
        current_bytes += len(item.data)
    if current:
        batches.append(current)
    return tuple(tuple(batch) for batch in batches)


def build_fixtures() -> dict[str, Fixture]:
    fixtures: list[Fixture] = []

    data = content_doc(ascii_text(64 * 1024))
    fixtures.append(
        Fixture(
            "A_long_ascii_64k_8b_g10",
            "A",
            data,
            fixed_chunks(data, 8),
            10,
            ("content",),
            "content",
            "64 KiB selected ASCII string, 8-byte chunks, 10 chunks per feed operation.",
        )
    )

    data = content_doc(mixed_text(64 * 1024))
    fixtures.append(
        Fixture(
            "A_long_mixed_64k_var_g10",
            "A",
            data,
            variable_chunks(data, seed=41),
            10,
            ("content",),
            "content",
            "64 KiB selected mixed Unicode and escaped string, variable 1-12 byte chunks.",
        )
    )

    data = content_doc(ascii_text(1024 * 1024))
    fixtures.append(
        Fixture(
            "A_long_ascii_1m_64b_g50",
            "A",
            data,
            fixed_chunks(data, 64),
            50,
            ("content",),
            "content",
            "1 MiB selected ASCII string, 64-byte chunks, 50 chunks per feed operation.",
        )
    )

    data = items_doc(10, 64)
    fixtures.append(
        Fixture(
            "B_items_10_text64_16b_g4",
            "B",
            data,
            fixed_chunks(data, 16),
            4,
            ("items.*.text",),
            "items_text",
            "Ten selected item strings; proves separate lexical strings are not merged.",
        )
    )

    data = items_doc(1_000, 64)
    fixtures.append(
        Fixture(
            "B_items_1000_text64_64b_g10",
            "B",
            data,
            fixed_chunks(data, 64),
            10,
            ("items.*.text",),
            "items_text",
            "One thousand selected item strings for path matching and event-count costs.",
        )
    )

    data = items_doc(1_000, 8, duplicate_keys=True)
    fixtures.append(
        Fixture(
            "B_duplicate_keys_1000_text8_32b_g10",
            "B",
            data,
            fixed_chunks(data, 32),
            10,
            ("items.*.text",),
            "items_text",
            "Duplicate object keys must remain separate streaming events.",
        )
    )

    data = content_doc(ascii_text(16 * 1024))
    fixtures.append(
        Fixture(
            "C_prefix_growing_string_4b_repeat",
            "C",
            data,
            fixed_chunks(data, 4),
            1,
            ("content",),
            "content",
            "Cumulative prefixes for one growing string, including repeated identical prefixes.",
        )
    )

    data = live_value_doc()
    fixtures.append(
        Fixture(
            "D_live_values_nested_256_64b_g8",
            "D",
            data,
            fixed_chunks(data, 64),
            8,
            tuple(),
            "rows_messages",
            "Nested live value updates with several changed paths per feed operation.",
        )
    )

    data = completed_items_doc(1_000, 1024)
    fixtures.append(
        Fixture(
            "E_completed_items_1000_1k_128b_g16",
            "E",
            data,
            fixed_chunks(data, 128),
            16,
            ("items.*",),
            "items",
            "Completed selected subtrees for a large item array.",
        )
    )

    data = completed_items_doc(1_000, 1024, truncated=True)
    fixtures.append(
        Fixture(
            "E_completed_items_1000_1k_truncated_128b_g16",
            "E",
            data,
            fixed_chunks(data, 128),
            16,
            ("items.*",),
            "items",
            "Completed subtrees with a truncated final document.",
        )
    )

    data = content_doc(ascii_text(64 * 1024))
    fixtures.append(
        Fixture(
            "F_byteviews_contiguous_64k",
            "F",
            data,
            (data,),
            1,
            ("content",),
            "content",
            "Selected string contained in one immutable input buffer.",
        )
    )

    fixtures.append(
        Fixture(
            "F_byteviews_cross_buffer_257b_g10",
            "F",
            data,
            fixed_chunks(data, 257),
            10,
            ("content",),
            "content",
            "Selected string spanning many immutable byte buffers.",
        )
    )

    data = content_doc(mixed_text(16 * 1024))
    fixtures.append(
        Fixture(
            "F_byteviews_escaped_unicode_17b_g10",
            "F",
            data,
            fixed_chunks(data, 17),
            10,
            ("content",),
            "content",
            "Escaped and Unicode selected string payloads for owned fallback behavior.",
        )
    )

    data = b'{"items":[{"text":"complete"},{"text":"truncated}'
    fixtures.append(
        Fixture(
            "G_truncated_string_after_complete",
            "G",
            data,
            fixed_chunks(data, 7),
            4,
            ("items.*.text",),
            "items_text",
            "One complete selected string followed by a truncated selected string.",
        )
    )

    data = b'{"content":"valid","bad":\xff}'
    fixtures.append(
        Fixture(
            "G_invalid_byte_middle",
            "G",
            data,
            fixed_chunks(data, 5),
            4,
            ("content",),
            "content",
            "Invalid byte after a complete selected value.",
        )
    )

    data = no_match_doc()
    fixtures.append(
        Fixture(
            "H_no_match_items_2000_512b_g20",
            "H",
            data,
            fixed_chunks(data, 512),
            20,
            ("missing.never",),
            "none",
            "Large document with a selected path that never occurs.",
        )
    )

    data = content_doc(ascii_text(64 * 1024))
    fixtures.append(
        Fixture(
            "I_feed_overhead_64k_8b_g10",
            "I",
            data,
            fixed_chunks(data, 8),
            10,
            ("content",),
            "content",
            "Validated input for repeated feed, iterable feed, tuple feed, and joined feed.",
        )
    )

    data = completed_items_doc(10_000, 128)
    fixtures.append(
        Fixture(
            "J_memory_items_10000_128b_256b_g20",
            "J",
            data,
            fixed_chunks(data, 256),
            20,
            ("items.*",),
            "items",
            "Memory scaling fixture for immediate-discard and retained-output consumers.",
        )
    )

    data = content_doc(ascii_text(32 * 1024))
    fixtures.append(
        Fixture(
            "K_batching_llm_trace_32k_8b",
            "K",
            data,
            fixed_chunks(data, 8),
            1,
            ("content",),
            "content",
            "Timestamped caller-owned batching trace over a growing selected string.",
        )
    )

    return {fixture.name: fixture for fixture in fixtures}


FIXTURES = build_fixtures()


def path_signature(path: Any) -> str:
    if hasattr(path, "as_tuple"):
        path = path.as_tuple()
    if isinstance(path, tuple):
        parts: list[str] = []
        for component in path:
            if isinstance(component, tuple) and len(component) == 2:
                kind, value = component
                parts.append(f"{kind}:{value}")
            else:
                parts.append(str(component))
        return "/".join(parts)
    return str(path)


def path_has_tail(path: Any, tail: str) -> bool:
    if hasattr(path, "endswith"):
        try:
            return bool(path.endswith(tail))
        except TypeError:
            pass
    if hasattr(path, "as_tuple"):
        path = path.as_tuple()
    if isinstance(path, tuple) and path:
        last = path[-1]
        return isinstance(last, tuple) and len(last) == 2 and last[0] == "key" and last[1] == tail
    return False


def payload_details(payload: Any) -> tuple[int, bool, bool, bool, str, str]:
    if payload is None or isinstance(payload, (bool, float, int)):
        return 0, False, False, False, "", ""
    if isinstance(payload, dict):
        fragment = payload.get("fragment", b"")
        return (
            payload_length(fragment),
            bool(payload.get("is_initial", False)),
            bool(payload.get("is_final", False)),
            bool(payload.get("is_view", False)),
            str(payload.get("payload_kind", "")),
            str(payload.get("ownership", "")),
        )
    fragment = getattr(payload, "fragment", "")
    return (
        payload_length(fragment),
        bool(getattr(payload, "is_initial", False)),
        bool(getattr(payload, "is_final", False)),
        False,
        "",
        "",
    )


def payload_length(fragment: Any) -> int:
    if isinstance(fragment, memoryview):
        return fragment.nbytes
    if isinstance(fragment, bytes):
        return len(fragment)
    if isinstance(fragment, str):
        return len(fragment.encode("utf-8", "surrogatepass"))
    return len(fragment) if hasattr(fragment, "__len__") else 0


def iter_result(result: Any) -> Iterable[Any]:
    events = getattr(result, "events", result)
    if callable(events):
        events = events()
    return events


def consume_events(result: Any, checksum: Checksum) -> tuple[int, int, int]:
    event_count = 0
    borrowed = 0
    owned = 0
    for kind, path, payload in iter_result(result):
        length, is_initial, is_final, is_view, payload_kind, ownership = payload_details(payload)
        checksum.add(
            "event",
            kind,
            path_signature(path),
            length,
            is_initial,
            is_final,
            is_view,
            payload_kind,
            ownership,
        )
        event_count += 1
        if str(kind) == "string":
            if is_view:
                borrowed += 1
            else:
                owned += 1
    return event_count, borrowed, owned


def consume_finish(parser: Any, checksum: Checksum) -> tuple[int, int, int]:
    result = parser.finish()
    return consume_events(result, checksum)


def jsonmodem_features() -> dict[str, bool]:
    jsonmodem = importlib.import_module("jsonmodem")
    from jsonmodem import JsonModem

    features = {
        "feed_many": hasattr(JsonModem(), "feed_many"),
        "string_events": False,
        "feed_result": hasattr(jsonmodem, "JsonModemFeed"),
        "selected_strings_adapter": hasattr(jsonmodem, "JsonModemSelectedStrings"),
        "selected_strings_prefix": False,
        "live_values_no_notify": hasattr(jsonmodem, "JsonModemLiveValuesNoNotify")
        or hasattr(getattr(jsonmodem, "JsonModemValues", object), "update"),
        "completed_subtrees": hasattr(jsonmodem, "JsonModemCompletedSubtrees")
        or hasattr(jsonmodem, "JsonModemSubtrees"),
    }
    selected_strings = getattr(jsonmodem, "JsonModemSelectedStrings", None)
    if selected_strings is not None:
        features["selected_strings_prefix"] = hasattr(selected_strings, "update_prefix")
    try:
        JsonModem(paths="content", string_events="per_feed")
    except TypeError:
        pass
    else:
        features["string_events"] = True
    return features


def create_jsonmodem(
    *,
    paths: Sequence[str] | None = None,
    byte_views: bool = False,
    string_events: str | None = None,
    allow_multiple: bool = False,
) -> Any:
    from jsonmodem import JsonModem, ParserOptions

    kwargs: dict[str, Any] = {}
    if paths:
        kwargs["paths"] = paths[0] if len(paths) == 1 else list(paths)
    if byte_views:
        kwargs["byte_views"] = True
    if string_events is not None:
        kwargs["string_events"] = string_events
    options = ParserOptions(allow_multiple=allow_multiple)
    try:
        return JsonModem(options, **kwargs)
    except TypeError as exc:
        if string_events is not None:
            raise MissingFeature("JsonModem string_events option is unavailable") from exc
        raise


def feed_parser(
    parser: Any,
    chunks: Sequence[bytes],
    method: str,
    checksum: Checksum,
) -> tuple[int, int, int]:
    if method == "per_chunk":
        totals = [0, 0, 0]
        for chunk in chunks:
            event_count, borrowed, owned = consume_events(parser.feed(chunk), checksum)
            totals[0] += event_count
            totals[1] += borrowed
            totals[2] += owned
        return totals[0], totals[1], totals[2]
    if method == "iterable_list":
        return consume_events(parser.feed(list(chunks)), checksum)
    if method == "iterable_tuple":
        return consume_events(parser.feed(tuple(chunks)), checksum)
    if method == "feed_many":
        if not hasattr(parser, "feed_many"):
            raise MissingFeature("feed_many is unavailable")
        return consume_events(parser.feed_many(tuple(chunks)), checksum)
    if method == "joined":
        return consume_events(parser.feed(b"".join(chunks)), checksum)
    raise ValueError(method)


def run_jsonmodem_selected(fixture: Fixture, method: str, string_events: str | None = None) -> int:
    parser = create_jsonmodem(paths=fixture.paths, string_events=string_events)
    checksum = Checksum()
    for chunk_group in grouped(fixture.chunks, fixture.feed_group):
        feed_parser(parser, chunk_group, method, checksum)
        checksum.add("parser_call", method, len(chunk_group))
    consume_finish(parser, checksum)
    return checksum.result()


def run_feed_result_selected(fixture: Fixture) -> int:
    jsonmodem = importlib.import_module("jsonmodem")
    cls = getattr(jsonmodem, "JsonModemFeed", None)
    if cls is None:
        raise MissingFeature("JsonModemFeed is unavailable")
    parser = cls(paths=fixture.paths[0] if len(fixture.paths) == 1 else list(fixture.paths))
    checksum = Checksum()
    for chunk_group in grouped(fixture.chunks, fixture.feed_group):
        result = parser.feed_many(list(chunk_group))
        consume_events(result, checksum)
        checksum.add("parser_call", "feed_result", len(chunk_group))
    consume_finish(parser, checksum)
    return checksum.result()


def run_selected_strings_adapter(fixture: Fixture) -> int:
    jsonmodem = importlib.import_module("jsonmodem")
    cls = getattr(jsonmodem, "JsonModemSelectedStrings", None)
    if cls is None:
        raise MissingFeature("JsonModemSelectedStrings is unavailable")
    parser = cls(paths=fixture.paths[0] if len(fixture.paths) == 1 else list(fixture.paths))
    checksum = Checksum()
    for chunk_group in grouped(fixture.chunks, fixture.feed_group):
        consume_events(parser.feed_many(list(chunk_group)), checksum)
        checksum.add("parser_call", "selected_strings_adapter", len(chunk_group))
    consume_finish(parser, checksum)
    return checksum.result()


def run_jsonmodem_unfiltered(fixture: Fixture, method: str) -> int:
    parser = create_jsonmodem()
    checksum = Checksum()
    for chunk_group in grouped(fixture.chunks, fixture.feed_group):
        feed_parser(parser, chunk_group, method, checksum)
    consume_finish(parser, checksum)
    return checksum.result()


def run_jsonmodem_byteviews(fixture: Fixture, method: str) -> int:
    parser = create_jsonmodem(paths=fixture.paths, byte_views=True)
    checksum = Checksum()
    events = 0
    borrowed = 0
    owned = 0
    for chunk_group in grouped(fixture.chunks, fixture.feed_group):
        before = checksum.count
        group_events, group_borrowed, group_owned = feed_parser(parser, chunk_group, method, checksum)
        events += group_events
        borrowed += group_borrowed
        owned += group_owned
        checksum.add("group_events", checksum.count - before)
    finish_events, finish_borrowed, finish_owned = consume_finish(parser, checksum)
    events += finish_events
    borrowed += finish_borrowed
    owned += finish_owned
    checksum.add("byte_payload_ownership", "borrowed", borrowed, "owned", owned, "events", events)
    return checksum.result()


def selected_value_measure(value: Any, extractor: str) -> tuple[int, int]:
    if extractor == "none":
        return 0, 0
    if extractor == "content":
        if isinstance(value, dict) and "content" in value:
            content = value["content"]
            if isinstance(content, str):
                return 1, len(content.encode("utf-8", "surrogatepass"))
        return 0, 0
    if extractor == "items_text":
        if not isinstance(value, dict):
            return 0, 0
        items = value.get("items", [])
        count = 0
        total = 0
        if isinstance(items, list):
            for item in items:
                if isinstance(item, dict) and isinstance(item.get("text"), str):
                    count += 1
                    total += len(item["text"].encode("utf-8", "surrogatepass"))
        return count, total
    if extractor == "rows_messages":
        if not isinstance(value, dict):
            return 0, 0
        rows = value.get("rows", [])
        count = 0
        total = 0
        if isinstance(rows, list):
            for row in rows:
                if isinstance(row, dict) and isinstance(row.get("message"), str):
                    count += 1
                    total += len(row["message"])
        return count, total
    if extractor == "items":
        if isinstance(value, dict) and isinstance(value.get("items"), list):
            return len(value["items"]), len(repr(value["items"]))
        return 0, 0
    raise ValueError(extractor)


def update_value_checksum(checksum: Checksum, value: Any, extractor: str) -> None:
    count, total = selected_value_measure(value, extractor)
    checksum.add("value", extractor, count, total)


def run_full_decode(fixture: Fixture, decoder_name: str) -> int:
    decoder = discover_decoders()[decoder_name]
    value = decoder(fixture.data)
    checksum = Checksum()
    update_value_checksum(checksum, value, fixture.extractor)
    return checksum.result()


def run_complete_prefix_fallback(fixture: Fixture, decoder_name: str, prepared_prefixes: Sequence[bytes]) -> int:
    decoder = discover_decoders()[decoder_name]
    checksum = Checksum()
    for prefix in prepared_prefixes:
        try:
            value = decoder(prefix)
        except Exception as exc:
            checksum.add("error", type(exc).__name__)
            continue
        update_value_checksum(checksum, value, fixture.extractor)
    return checksum.result()


def run_jiter_cumulative_partial(fixture: Fixture, prepared_prefixes: Sequence[bytes]) -> int:
    jiter = load_optional("jiter")
    if jiter is None:
        raise MissingFeature("jiter is not installed")
    checksum = Checksum()
    for prefix in prepared_prefixes:
        value = jiter.from_json(prefix, partial_mode=True)
        update_value_checksum(checksum, value, fixture.extractor)
    return checksum.result()


def run_partial_json_parser_prefix(fixture: Fixture, chunks: Sequence[bytes]) -> int:
    partial_json_parser = load_optional("partial_json_parser")
    if partial_json_parser is None:
        raise MissingFeature("partial-json-parser is not installed")
    checksum = Checksum()
    buffer = ""
    for chunk in chunks:
        buffer += chunk.decode("utf-8", "surrogatepass")
        try:
            value = partial_json_parser.loads(buffer)
        except Exception as exc:
            checksum.add("error", type(exc).__name__)
            continue
        update_value_checksum(checksum, value, fixture.extractor)
    return checksum.result()


def run_jsonmodem_prefix_adapter(fixture: Fixture, prepared_prefixes: Sequence[bytes], validate: bool) -> int:
    parser = create_jsonmodem(paths=fixture.paths)
    checksum = Checksum()
    accepted = 0
    previous = b""
    for prefix in prepared_prefixes:
        if len(prefix) == accepted:
            checksum.add("prefix", "repeat", 0)
            previous = prefix
            continue
        if len(prefix) < accepted:
            checksum.add("prefix", "shorter", len(prefix))
            raise ValueError("prefix is shorter than the accepted input")
        if validate and not prefix.startswith(previous):
            checksum.add("prefix", "rejected", len(prefix))
            raise ValueError("prefix is not an append-only extension")
        new_bytes = prefix[accepted:]
        consume_events(parser.feed(new_bytes), checksum)
        checksum.add("prefix", "validated_append" if validate else "trusted_append", len(new_bytes))
        accepted = len(prefix)
        previous = prefix
    consume_finish(parser, checksum)
    return checksum.result()


def run_native_prefix_adapter_if_available(
    fixture: Fixture,
    prepared_prefixes: Sequence[bytes],
    validate: bool,
) -> int:
    jsonmodem = importlib.import_module("jsonmodem")
    cls = getattr(jsonmodem, "JsonModemSelectedStrings", None)
    if cls is None or not hasattr(cls, "update_prefix"):
        raise MissingFeature("JsonModemSelectedStrings.update_prefix is unavailable")
    paths = fixture.paths[0] if len(fixture.paths) == 1 else list(fixture.paths)
    parser = cls(paths=paths)
    checksum = Checksum()
    for prefix in prepared_prefixes:
        result = parser.update_prefix(prefix, validate=validate)
        checksum.add(
            "native_prefix",
            result.get("status"),
            result.get("accepted_len"),
            result.get("validation_bytes"),
        )
        consume_events(result.get("events", ()), checksum)
    consume_finish(parser, checksum)
    return checksum.result()


def run_prefix_rewrite_cases(fixture: Fixture) -> int:
    parser = create_jsonmodem(paths=fixture.paths)
    checksum = Checksum()
    accepted = 0
    previous = b""
    prefixes = cumulative_prefixes(fixture.chunks[:16])
    bad_end = prefixes[-1][:-1] + (b"X" if prefixes[-1][-1:] != b"X" else b"Y")
    bad_begin = (b"X" if prefixes[-1][:1] != b"X" else b"Y") + prefixes[-1][1:]
    shorter = prefixes[-1][: max(0, len(prefixes[-1]) // 2)]
    for prefix in (*prefixes, prefixes[-1], bad_end, bad_begin, shorter):
        try:
            if len(prefix) == accepted:
                checksum.add("prefix", "repeat")
            elif len(prefix) < accepted or not prefix.startswith(previous):
                checksum.add("prefix", "rejected")
            else:
                consume_events(parser.feed(prefix[accepted:]), checksum)
                checksum.add("prefix", "append", len(prefix) - accepted)
                accepted = len(prefix)
                previous = prefix
        except Exception as exc:
            checksum.add("error", type(exc).__name__)
    return checksum.result()


def run_jsonmodem_values(fixture: Fixture, snapshot_each_feed: bool, read_view_each_feed: bool) -> int:
    from jsonmodem import JsonModemValues

    parser = JsonModemValues()
    checksum = Checksum()
    for chunk_group in grouped(fixture.chunks, fixture.feed_group):
        for index, view, path, is_final in parser.feed(list(chunk_group)):
            checksum.add("update", index, getattr(view, "kind", ""), path_signature(path), is_final)
        if read_view_each_feed:
            view = parser.view()
            checksum.add("view", getattr(view, "kind", ""), len(repr(view)))
        if snapshot_each_feed:
            snapshot = parser.view().snapshot()
            update_value_checksum(checksum, snapshot, fixture.extractor)
    for index, view, path, is_final in parser.finish():
        checksum.add("finish", index, getattr(view, "kind", ""), path_signature(path), is_final)
    return checksum.result()


def run_live_values_no_notify_if_available(
    fixture: Fixture,
    snapshot_each_feed: bool,
    read_view_each_feed: bool,
) -> int:
    jsonmodem = importlib.import_module("jsonmodem")
    cls = getattr(jsonmodem, "JsonModemLiveValuesNoNotify", None)
    if cls is None:
        cls = getattr(jsonmodem, "JsonModemValues", None)
    if cls is None or not hasattr(cls, "update"):
        raise MissingFeature("JsonModemValues.update is unavailable")
    parser = cls()
    checksum = Checksum()
    root_view = parser.view()
    for chunk_group in grouped(fixture.chunks, fixture.feed_group):
        view = parser.update(list(chunk_group))
        checksum.add("same_view", view is root_view)
        if read_view_each_feed:
            try:
                count = view["summary"]["count"].snapshot()
            except Exception as exc:
                checksum.add("read_error", type(exc).__name__)
            else:
                checksum.add("read", "summary.count", count)
        if snapshot_each_feed:
            snapshot = view.snapshot()
            update_value_checksum(checksum, snapshot, fixture.extractor)
    try:
        final = parser.finish(changed_paths=False)
    except TypeError:
        final = parser.finish()
    checksum.add("finish_same_view", final is root_view)
    update_value_checksum(checksum, final.snapshot(), fixture.extractor)
    return checksum.result()


def run_completed_subtree_markers(fixture: Fixture) -> int:
    parser = create_jsonmodem()
    checksum = Checksum()
    for chunk_group in grouped(fixture.chunks, fixture.feed_group):
        for kind, path, payload in parser.feed(list(chunk_group)):
            if kind == "object_end" and item_object_path(path):
                checksum.add("completed_marker", path_signature(path), payload_details(payload))
    try:
        for kind, path, payload in parser.finish():
            if kind == "object_end" and item_object_path(path):
                checksum.add("completed_marker", path_signature(path), payload_details(payload))
    except Exception as exc:
        checksum.add("finish_error", type(exc).__name__)
    return checksum.result()


def item_object_path(path: Any) -> bool:
    if hasattr(path, "as_tuple"):
        path = path.as_tuple()
    if not isinstance(path, tuple) or len(path) != 2:
        return False
    first, second = path
    return (
        isinstance(first, tuple)
        and first == ("key", "items")
        and isinstance(second, tuple)
        and second[0] == "index"
    )


def run_jsonmodem_subtrees_if_available(fixture: Fixture) -> int:
    jsonmodem = importlib.import_module("jsonmodem")
    cls = getattr(jsonmodem, "JsonModemCompletedSubtrees", None)
    if cls is None:
        cls = getattr(jsonmodem, "JsonModemSubtrees", None)
    if cls is None:
        raise MissingFeature("completed-subtree API is unavailable")
    paths = fixture.paths[0] if len(fixture.paths) == 1 else list(fixture.paths)
    parser = cls(paths=paths)
    checksum = Checksum()
    for chunk_group in grouped(fixture.chunks, fixture.feed_group):
        result = parser.feed_many(list(chunk_group)) if hasattr(parser, "feed_many") else parser.feed(list(chunk_group))
        subtrees = getattr(result, "completed_subtrees", result)
        for subtree in subtrees:
            if isinstance(subtree, tuple) and len(subtree) == 3:
                path, value, released = subtree
                checksum.add("subtree", path_signature(path), len(repr(value)), released)
            else:
                checksum.add("subtree", len(repr(subtree)))
    if hasattr(parser, "finish"):
        try:
            result = parser.finish()
            subtrees = getattr(result, "completed_subtrees", result)
            for subtree in subtrees:
                if isinstance(subtree, tuple) and len(subtree) == 3:
                    path, value, released = subtree
                    checksum.add("finish_subtree", path_signature(path), len(repr(value)), released)
                else:
                    checksum.add("finish_subtree", len(repr(subtree)))
        except Exception as exc:
            checksum.add("finish_error", type(exc).__name__)
    return checksum.result()


def run_error_case(fixture: Fixture) -> int:
    parser = create_jsonmodem(paths=fixture.paths)
    checksum = Checksum()
    for chunk in fixture.chunks:
        try:
            consume_events(parser.feed(chunk), checksum)
        except Exception as exc:
            checksum.add("feed_error", type(exc).__name__, str(exc)[:120])
            break
    try:
        consume_finish(parser, checksum)
    except Exception as exc:
        checksum.add("finish_error", type(exc).__name__, str(exc)[:120])
    return checksum.result()


def run_batching_policy(fixture: Fixture, mean_ms: float, window_multiplier: float, jitter: bool) -> int:
    trace = make_timed_trace(fixture.chunks, mean_ms=mean_ms, jitter=jitter)
    batches = batch_trace(
        trace,
        window_ms=mean_ms * window_multiplier,
        max_bytes=4096,
        max_chunks=max(1, int(4 * window_multiplier)),
    )
    parser = create_jsonmodem(paths=fixture.paths)
    checksum = Checksum()
    first_output_batch: int | None = None
    for index, batch in enumerate(batches):
        before = checksum.count
        consume_events(parser.feed(list(batch)), checksum)
        if first_output_batch is None and checksum.count != before:
            first_output_batch = index
        checksum.add("batch", index, len(batch), sum(len(chunk) for chunk in batch))
    consume_finish(parser, checksum)
    checksum.add("batch_summary", len(batches), first_output_batch)
    return checksum.result()


def discover_decoders() -> dict[str, Callable[[bytes], Any]]:
    decoders: dict[str, Callable[[bytes], Any]] = {
        "stdlib_json": lambda data: json.loads(data),
    }
    orjson = load_optional("orjson")
    if orjson is not None:
        decoders["orjson"] = orjson.loads
    jiter = load_optional("jiter")
    if jiter is not None and hasattr(jiter, "from_json"):
        decoders["jiter"] = jiter.from_json
    return decoders


def benchmark_fixtures(families: set[str], names: set[str] | None) -> list[Fixture]:
    fixtures = list(FIXTURES.values())
    if names is not None:
        fixtures = [fixture for fixture in fixtures if fixture.name in names]
    else:
        fixtures = [fixture for fixture in fixtures if fixture.family in families]
    return fixtures


def add_benchmark(
    benches: list[Benchmark],
    fixture: Fixture,
    group: str,
    semantics: str,
    parser_name: str,
    func: Callable[[], int],
) -> None:
    benches.append(
        Benchmark(
            name=f"{parser_name}:{fixture.name}:{semantics}",
            group=group,
            family=fixture.family,
            semantics=semantics,
            run=func,
        )
    )


def build_benchmarks(fixtures: Sequence[Fixture], groups: set[str]) -> list[Benchmark]:
    features = jsonmodem_features()
    decoders = discover_decoders()
    benches: list[Benchmark] = []

    for fixture in fixtures:
        if fixture.family in {"A", "B"} and "selected_stream" in groups:
            add_benchmark(
                benches,
                fixture,
                "selected_stream",
                "selected_fragment_per_chunk",
                "jsonmodem",
                lambda fixture=fixture: run_jsonmodem_selected(fixture, "per_chunk"),
            )
            add_benchmark(
                benches,
                fixture,
                "selected_stream",
                "selected_iterable_group",
                "jsonmodem",
                lambda fixture=fixture: run_jsonmodem_selected(fixture, "iterable_list"),
            )
            add_benchmark(
                benches,
                fixture,
                "selected_stream",
                "selected_joined_group",
                "jsonmodem",
                lambda fixture=fixture: run_jsonmodem_selected(fixture, "joined"),
            )
            if features["feed_many"]:
                add_benchmark(
                    benches,
                    fixture,
                    "selected_stream",
                    "selected_feed_many_group",
                    "jsonmodem",
                    lambda fixture=fixture: run_jsonmodem_selected(fixture, "feed_many"),
                )
            if features["string_events"]:
                add_benchmark(
                    benches,
                    fixture,
                    "selected_stream",
                    "selected_per_feed_native_compaction",
                    "jsonmodem",
                    lambda fixture=fixture: run_jsonmodem_selected(
                        fixture, "feed_many" if features["feed_many"] else "iterable_list", "per_feed"
                    ),
                )
            if features["feed_result"]:
                add_benchmark(
                    benches,
                    fixture,
                    "selected_stream",
                    "selected_per_feed_result",
                    "jsonmodem_feedresult",
                    lambda fixture=fixture: run_feed_result_selected(fixture),
                )
            if features["selected_strings_adapter"]:
                add_benchmark(
                    benches,
                    fixture,
                    "selected_stream",
                    "selected_native_adapter",
                    "jsonmodem_selectedstrings",
                    lambda fixture=fixture: run_selected_strings_adapter(fixture),
                )
            prefixes = cumulative_prefixes(fixture.chunks, repeat_every=97)
            if load_optional("jiter") is not None:
                add_benchmark(
                    benches,
                    fixture,
                    "selected_stream",
                    "cumulative_partial_value_prefixes",
                    "jiter",
                    lambda fixture=fixture, prefixes=prefixes: run_jiter_cumulative_partial(fixture, prefixes),
                )
            if load_optional("partial_json_parser") is not None:
                add_benchmark(
                    benches,
                    fixture,
                    "selected_stream",
                    "cumulative_partial_value_prefixes",
                    "partial_json_parser",
                    lambda fixture=fixture: run_partial_json_parser_prefix(fixture, fixture.chunks),
                )
            for decoder_name in decoders:
                add_benchmark(
                    benches,
                    fixture,
                    "selected_stream",
                    "full_value_reference",
                    decoder_name,
                    lambda fixture=fixture, decoder_name=decoder_name: run_full_decode(fixture, decoder_name),
                )

        if fixture.family == "C" and "prefix" in groups:
            prefixes = cumulative_prefixes(fixture.chunks, repeat_every=31)
            add_benchmark(
                benches,
                fixture,
                "prefix",
                "direct_delta_reference",
                "jsonmodem",
                lambda fixture=fixture: run_jsonmodem_selected(fixture, "per_chunk"),
            )
            add_benchmark(
                benches,
                fixture,
                "prefix",
                "trusted_append_only_prefix_adapter",
                "jsonmodem",
                lambda fixture=fixture, prefixes=prefixes: run_jsonmodem_prefix_adapter(fixture, prefixes, validate=False),
            )
            add_benchmark(
                benches,
                fixture,
                "prefix",
                "validated_append_only_prefix_adapter",
                "jsonmodem",
                lambda fixture=fixture, prefixes=prefixes: run_jsonmodem_prefix_adapter(fixture, prefixes, validate=True),
            )
            if features["selected_strings_prefix"]:
                add_benchmark(
                    benches,
                    fixture,
                    "prefix",
                    "native_trusted_prefix_update",
                    "jsonmodem_selectedstrings",
                    lambda fixture=fixture, prefixes=prefixes: run_native_prefix_adapter_if_available(
                        fixture, prefixes, validate=False
                    ),
                )
                add_benchmark(
                    benches,
                    fixture,
                    "prefix",
                    "native_validated_prefix_update",
                    "jsonmodem_selectedstrings",
                    lambda fixture=fixture, prefixes=prefixes: run_native_prefix_adapter_if_available(
                        fixture, prefixes, validate=True
                    ),
                )
            add_benchmark(
                benches,
                fixture,
                "prefix",
                "rewrite_rejection_cases",
                "jsonmodem",
                lambda fixture=fixture: run_prefix_rewrite_cases(fixture),
            )
            if load_optional("jiter") is not None:
                add_benchmark(
                    benches,
                    fixture,
                    "prefix",
                    "cumulative_partial_value_prefixes",
                    "jiter",
                    lambda fixture=fixture, prefixes=prefixes: run_jiter_cumulative_partial(fixture, prefixes),
                )
            if load_optional("partial_json_parser") is not None:
                add_benchmark(
                    benches,
                    fixture,
                    "prefix",
                    "cumulative_partial_value_prefixes",
                    "partial_json_parser",
                    lambda fixture=fixture: run_partial_json_parser_prefix(fixture, fixture.chunks),
                )
            for decoder_name in ("stdlib_json", "orjson"):
                if decoder_name in decoders:
                    add_benchmark(
                        benches,
                        fixture,
                        "prefix",
                        "complete_prefix_try_decode",
                        decoder_name,
                        lambda fixture=fixture, decoder_name=decoder_name, prefixes=prefixes: run_complete_prefix_fallback(
                            fixture, decoder_name, prefixes
                        ),
                    )

        if fixture.family == "D" and "live_values" in groups:
            add_benchmark(
                benches,
                fixture,
                "live_values",
                "per_mutation_updates",
                "jsonmodem_values",
                lambda fixture=fixture: run_jsonmodem_values(fixture, snapshot_each_feed=False, read_view_each_feed=False),
            )
            add_benchmark(
                benches,
                fixture,
                "live_values",
                "view_read_after_feed",
                "jsonmodem_values",
                lambda fixture=fixture: run_jsonmodem_values(fixture, snapshot_each_feed=False, read_view_each_feed=True),
            )
            add_benchmark(
                benches,
                fixture,
                "live_values",
                "snapshot_after_feed",
                "jsonmodem_values",
                lambda fixture=fixture: run_jsonmodem_values(fixture, snapshot_each_feed=True, read_view_each_feed=True),
            )
            if features["live_values_no_notify"]:
                add_benchmark(
                    benches,
                    fixture,
                    "live_values",
                    "no_notification_update",
                    "jsonmodem_live_values",
                    lambda fixture=fixture: run_live_values_no_notify_if_available(
                        fixture, snapshot_each_feed=False, read_view_each_feed=False
                    ),
                )
                add_benchmark(
                    benches,
                    fixture,
                    "live_values",
                    "no_notification_view_read_after_feed",
                    "jsonmodem_live_values",
                    lambda fixture=fixture: run_live_values_no_notify_if_available(
                        fixture, snapshot_each_feed=False, read_view_each_feed=True
                    ),
                )
                add_benchmark(
                    benches,
                    fixture,
                    "live_values",
                    "no_notification_snapshot_after_feed",
                    "jsonmodem_live_values",
                    lambda fixture=fixture: run_live_values_no_notify_if_available(
                        fixture, snapshot_each_feed=True, read_view_each_feed=True
                    ),
                )
            prefixes = cumulative_prefixes(fixture.chunks, repeat_every=43)
            if load_optional("jiter") is not None:
                add_benchmark(
                    benches,
                    fixture,
                    "live_values",
                    "cumulative_partial_value_prefixes",
                    "jiter",
                    lambda fixture=fixture, prefixes=prefixes: run_jiter_cumulative_partial(fixture, prefixes),
                )

        if fixture.family == "E" and "subtrees" in groups:
            add_benchmark(
                benches,
                fixture,
                "subtrees",
                "completed_subtree_markers_only",
                "jsonmodem",
                lambda fixture=fixture: run_completed_subtree_markers(fixture),
            )
            if features["completed_subtrees"]:
                add_benchmark(
                    benches,
                    fixture,
                    "subtrees",
                    "completed_subtree_values",
                    "jsonmodem_subtrees",
                    lambda fixture=fixture: run_jsonmodem_subtrees_if_available(fixture),
                )
            for decoder_name in decoders:
                add_benchmark(
                    benches,
                    fixture,
                    "subtrees",
                    "full_value_reference",
                    decoder_name,
                    lambda fixture=fixture, decoder_name=decoder_name: run_full_decode(fixture, decoder_name),
                )

        if fixture.family == "F" and "byte_payloads" in groups:
            add_benchmark(
                benches,
                fixture,
                "byte_payloads",
                "byteviews_iterable_group",
                "jsonmodem",
                lambda fixture=fixture: run_jsonmodem_byteviews(fixture, "iterable_list"),
            )
            add_benchmark(
                benches,
                fixture,
                "byte_payloads",
                "decoded_str_iterable_group",
                "jsonmodem",
                lambda fixture=fixture: run_jsonmodem_selected(fixture, "iterable_list"),
            )
            add_benchmark(
                benches,
                fixture,
                "byte_payloads",
                "caller_joined_bytes",
                "jsonmodem",
                lambda fixture=fixture: run_jsonmodem_byteviews(fixture, "joined"),
            )

        if fixture.family == "G" and "errors" in groups:
            add_benchmark(
                benches,
                fixture,
                "errors",
                "incremental_error_reporting",
                "jsonmodem",
                lambda fixture=fixture: run_error_case(fixture),
            )

        if fixture.family == "H" and "no_match" in groups:
            add_benchmark(
                benches,
                fixture,
                "no_match",
                "selected_no_match",
                "jsonmodem",
                lambda fixture=fixture: run_jsonmodem_selected(fixture, "iterable_list"),
            )
            add_benchmark(
                benches,
                fixture,
                "no_match",
                "unfiltered_event_stream",
                "jsonmodem",
                lambda fixture=fixture: run_jsonmodem_unfiltered(fixture, "iterable_list"),
            )
            for decoder_name in decoders:
                add_benchmark(
                    benches,
                    fixture,
                    "no_match",
                    "full_value_reference",
                    decoder_name,
                    lambda fixture=fixture, decoder_name=decoder_name: run_full_decode(fixture, decoder_name),
                )

        if fixture.family == "I" and "feed_overhead" in groups:
            for method, semantics in (
                ("per_chunk", "repeated_feed_chunk"),
                ("iterable_list", "feed_iterable_list_groups"),
                ("iterable_tuple", "feed_iterable_tuple_groups"),
                ("joined", "caller_joined_group"),
            ):
                add_benchmark(
                    benches,
                    fixture,
                    "feed_overhead",
                    semantics,
                    "jsonmodem",
                    lambda fixture=fixture, method=method: run_jsonmodem_selected(fixture, method),
                )
            if features["feed_many"]:
                add_benchmark(
                    benches,
                    fixture,
                    "feed_overhead",
                    "feed_many_groups",
                    "jsonmodem",
                    lambda fixture=fixture: run_jsonmodem_selected(fixture, "feed_many"),
                )

        if fixture.family == "J" and "memory_scaling" in groups:
            add_benchmark(
                benches,
                fixture,
                "memory_scaling",
                "completed_subtree_markers_only",
                "jsonmodem",
                lambda fixture=fixture: run_completed_subtree_markers(fixture),
            )
            if features["completed_subtrees"]:
                add_benchmark(
                    benches,
                    fixture,
                    "memory_scaling",
                    "completed_subtree_values",
                    "jsonmodem_subtrees",
                    lambda fixture=fixture: run_jsonmodem_subtrees_if_available(fixture),
                )
            for decoder_name in decoders:
                add_benchmark(
                    benches,
                    fixture,
                    "memory_scaling",
                    "full_value_reference",
                    decoder_name,
                    lambda fixture=fixture, decoder_name=decoder_name: run_full_decode(fixture, decoder_name),
                )

        if fixture.family == "K" and "batching" in groups:
            for mean_ms in (1.0, 5.0, 20.0):
                for multiplier in (0.0, 1.0, 2.0, 4.0):
                    window = max(mean_ms * multiplier, 0.0)
                    semantics = f"mean{mean_ms:g}ms_window{window:g}ms"
                    add_benchmark(
                        benches,
                        fixture,
                        "batching",
                        semantics,
                        "jsonmodem",
                        lambda fixture=fixture, mean_ms=mean_ms, multiplier=multiplier: run_batching_policy(
                            fixture, mean_ms, multiplier, jitter=True
                        ),
                    )

    return [bench for bench in benches if bench.group in groups]


def default_groups_for(families: set[str]) -> set[str]:
    groups: set[str] = set()
    if families & {"A", "B"}:
        groups.add("selected_stream")
    if "C" in families:
        groups.add("prefix")
    if "D" in families:
        groups.add("live_values")
    if "E" in families:
        groups.add("subtrees")
    if "F" in families:
        groups.add("byte_payloads")
    if "G" in families:
        groups.add("errors")
    if "H" in families:
        groups.add("no_match")
    if "I" in families:
        groups.add("feed_overhead")
    if "J" in families:
        groups.add("memory_scaling")
    if "K" in families:
        groups.add("batching")
    return groups


def selected_items(env_name: str, default: str) -> list[str]:
    return [item for item in os.environ.get(env_name, default).split(",") if item]


def add_metadata(runner: pyperf.Runner, fixtures: Sequence[Fixture], groups: set[str], worktree: Path) -> None:
    optional = {
        "orjson": load_optional("orjson"),
        "jiter": load_optional("jiter"),
        "partial_json_parser": load_optional("partial_json_parser"),
        "ijson": load_optional("ijson"),
        "json_stream": load_optional("json_stream"),
        "jsonriver": load_optional("jsonriver"),
        "json_streamer": load_optional("json_streamer"),
        "streaming_json_parser": load_optional("streaming_json_parser"),
    }
    metadata = {
        "python": sys.version.replace("\n", " "),
        "platform": platform.platform(),
        "machine": platform.machine(),
        "processor": platform.processor() or cpu_model(),
        "jsonmodem_worktree": str(worktree),
        "jsonmodem_commit": command_output(["git", "rev-parse", "HEAD"], cwd=worktree),
        "benchmark_worktree": str(REPO_ROOT),
        "benchmark_commit": command_output(["git", "rev-parse", "HEAD"], cwd=REPO_ROOT),
        "rustc": command_output(["rustc", "--version"]),
        "cargo": command_output(["cargo", "--version"]),
        "benchmark_method": (
            "customer requirements A-K; output semantics are encoded in benchmark names; "
            "full-value decoders are reference context unless the name says full_value_reference"
        ),
        "groups": ",".join(sorted(groups)),
    }
    for name, module in optional.items():
        metadata[f"{name}_version"] = package_version(module)
    try:
        import jsonmodem
    except ImportError:
        metadata["jsonmodem_version"] = "missing"
    else:
        metadata["jsonmodem_version"] = package_version(jsonmodem)
        metadata["jsonmodem_features"] = json.dumps(jsonmodem_features(), sort_keys=True)
    for fixture in fixtures:
        metadata[f"fixture_{fixture.name}_family"] = fixture.family
        metadata[f"fixture_{fixture.name}_bytes"] = str(len(fixture.data))
        metadata[f"fixture_{fixture.name}_chunks"] = str(len(fixture.chunks))
        metadata[f"fixture_{fixture.name}_feed_group"] = str(fixture.feed_group)
        metadata[f"fixture_{fixture.name}_sha256"] = stable_hash(fixture.data)
        metadata[f"fixture_{fixture.name}_description"] = fixture.description
    for key, value in metadata.items():
        runner.metadata[key] = value


def cpu_model() -> str:
    cpuinfo = Path("/proc/cpuinfo")
    if not cpuinfo.exists():
        return "unavailable"
    for line in cpuinfo.read_text(errors="ignore").splitlines():
        if line.lower().startswith("model name"):
            return line.split(":", 1)[1].strip()
    return "unavailable"


def parse_args() -> tuple[argparse.Namespace, list[str]]:
    parser = argparse.ArgumentParser()
    parser.add_argument("--family", action="append", choices=ALL_FAMILIES)
    parser.add_argument("--fixture", action="append", choices=tuple(FIXTURES))
    parser.add_argument(
        "--group",
        action="append",
        choices=(
            "selected_stream",
            "prefix",
            "live_values",
            "subtrees",
            "byte_payloads",
            "errors",
            "no_match",
            "feed_overhead",
            "memory_scaling",
            "batching",
        ),
    )
    parser.add_argument("--list-fixtures", action="store_true")
    parser.add_argument("--list", action="store_true")
    parser.add_argument(
        "--parser",
        action="append",
        help="Run only benchmarks whose name starts with this parser label. May be repeated.",
    )
    parser.add_argument("--smoke", action="store_true", help="Run selected functions once without pyperf.")
    parser.add_argument("--smoke-limit", type=int, default=0)
    parser.add_argument("--smoke-output", type=Path)
    parser.add_argument(
        "--jsonmodem-worktree",
        type=Path,
        default=Path(os.environ.get(WORKTREE_ENV, REPO_ROOT)),
        help=(
            "Path recorded as the jsonmodem implementation under test. "
            "Run this script with that worktree's Python environment when comparing branches."
        ),
    )
    return parser.parse_known_args()


def configure_environment(args: argparse.Namespace, pyperf_args: list[str]) -> None:
    if args.family:
        os.environ[FAMILY_ENV] = ",".join(args.family)
    if args.fixture:
        os.environ[BENCH_ENV] = ",".join(args.fixture)
    if args.group:
        os.environ[GROUP_ENV] = ",".join(args.group)
    if args.parser:
        os.environ[PARSER_ENV] = ",".join(args.parser)
    os.environ[WORKTREE_ENV] = str(args.jsonmodem_worktree)
    if (args.family or args.fixture or args.group or args.parser or args.jsonmodem_worktree) and not any(
        item == "--copy-env" or item.startswith("--inherit-environ") for item in pyperf_args
    ):
        pyperf_args.extend(["--inherit-environ", f"{FAMILY_ENV},{BENCH_ENV},{GROUP_ENV},{PARSER_ENV},{WORKTREE_ENV}"])


def selected_configuration(args: argparse.Namespace) -> tuple[list[Fixture], set[str]]:
    family_names = set(selected_items(FAMILY_ENV, DEFAULT_FAMILIES))
    fixture_names = set(selected_items(BENCH_ENV, "")) or None
    fixtures = benchmark_fixtures(family_names, fixture_names)
    if args.group:
        groups = set(args.group)
    else:
        groups = set(selected_items(GROUP_ENV, "")) or default_groups_for({fixture.family for fixture in fixtures})
    return fixtures, groups


def run_smoke(benches: Sequence[Benchmark], output: Path | None, limit: int) -> None:
    rows = []
    selected = benches[:limit] if limit else benches
    for bench in selected:
        gc.collect()
        started = time.perf_counter()
        try:
            result = bench.run()
        except MissingFeature as exc:
            row = {
                "name": bench.name,
                "group": bench.group,
                "family": bench.family,
                "semantics": bench.semantics,
                "status": "missing_feature",
                "error": str(exc),
            }
        except Exception as exc:
            row = {
                "name": bench.name,
                "group": bench.group,
                "family": bench.family,
                "semantics": bench.semantics,
                "status": "error",
                "error_type": type(exc).__name__,
                "error": str(exc),
            }
        else:
            row = {
                "name": bench.name,
                "group": bench.group,
                "family": bench.family,
                "semantics": bench.semantics,
                "status": "ok",
                "checksum": result,
                "seconds": time.perf_counter() - started,
            }
        rows.append(row)
        print(json.dumps(row, sort_keys=True))
    if output is not None:
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(json.dumps(rows, indent=2, sort_keys=True) + "\n")


def main() -> None:
    args, pyperf_args = parse_args()

    if args.list_fixtures:
        for fixture in FIXTURES.values():
            print(f"{fixture.name}\tfamily={fixture.family}\tbytes={len(fixture.data)}\tchunks={len(fixture.chunks)}\t{fixture.description}")
        return

    configure_environment(args, pyperf_args)
    fixtures, groups = selected_configuration(args)
    benches = build_benchmarks(fixtures, groups)
    parser_filters = set(selected_items(PARSER_ENV, ""))
    if parser_filters:
        benches = [bench for bench in benches if bench.name.split(":", 1)[0] in parser_filters]

    if args.list:
        for bench in benches:
            print(f"{bench.name}\tgroup={bench.group}\tfamily={bench.family}\tsemantics={bench.semantics}")
        return

    if args.smoke:
        run_smoke(benches, args.smoke_output, args.smoke_limit)
        return

    sys.argv = [sys.argv[0], *pyperf_args]
    runner = pyperf.Runner()
    add_metadata(runner, fixtures, groups, args.jsonmodem_worktree)
    for bench in benches:
        runner.bench_func(bench.name, bench.run)


if __name__ == "__main__":
    main()
