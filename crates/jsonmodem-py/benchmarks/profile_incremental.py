#!/usr/bin/env python3
from __future__ import annotations

import argparse
from time import perf_counter
from typing import Callable

import jiter
from jsonmodem import JsonModem


def make_doc(chunk_count: int, chunk_size: int) -> bytes:
    overhead = len(b'{"content":"","done":true}')
    payload_size = max(chunk_count * chunk_size - overhead, 0)
    return b'{"content":"' + (b"a" * payload_size) + b'","done":true}'


def chunk_bytes(data: bytes, size: int) -> list[bytes]:
    return [data[index : index + size] for index in range(0, len(data), size)]


def jsonmodem_events(chunks: list[bytes]) -> int:
    parser = JsonModem()
    count = 0
    for chunk in chunks:
        for _event in parser.feed(chunk):
            count += 1
    for _event in parser.finish():
        count += 1
    return count


def jsonmodem_events_unpack(chunks: list[bytes]) -> int:
    parser = JsonModem()
    total = 0
    for chunk in chunks:
        for kind, path, payload in parser.feed(chunk):
            total += len(kind) + len(path)
            if kind == "string":
                total += len(payload.fragment)
    for kind, path, payload in parser.finish():
        total += len(kind) + len(path)
        if kind == "string":
            total += len(payload.fragment)
    return total


def jsonmodem_feed_chunks_events(chunks: list[bytes]) -> int:
    parser = JsonModem()
    count = 0
    for _event in parser.feed(chunks):
        count += 1
    for _event in parser.finish():
        count += 1
    return count


def jsonmodem_feed_chunks_events_unpack(chunks: list[bytes]) -> int:
    parser = JsonModem()
    total = 0
    for kind, path, payload in parser.feed(chunks):
        total += len(kind) + len(path)
        if kind == "string":
            total += len(payload.fragment)
    for kind, path, payload in parser.finish():
        total += len(kind) + len(path)
        if kind == "string":
            total += len(payload.fragment)
    return total


def jsonmodem_pathfilter_byteviews(chunks: list[bytes]) -> int:
    parser = JsonModem(paths="content", byte_views=True)
    total = 0
    for chunk in chunks:
        for _kind, _path, payload in parser.feed(chunk):
            fragment = payload["fragment"]
            total += len(fragment) if payload["is_view"] else len(fragment.encode())
    for _kind, _path, payload in parser.finish():
        fragment = payload["fragment"]
        total += len(fragment) if payload["is_view"] else len(fragment.encode())
    return total


def jiter_cumulative_partial(chunks: list[bytes]) -> int:
    """Parse every cumulative prefix with jiter partial mode.

    This is the fair comparison for jsonmodem's per-fragment feed path: both
    libraries process the same stream of fragments and produce partial progress
    after each fragment.
    """

    pending = bytearray()
    total = 0
    for chunk in chunks:
        pending.extend(chunk)
        value = jiter.from_json(bytes(pending), partial_mode=True)
        if isinstance(value, dict) and "content" in value:
            total = len(value["content"])
    return total


MODES: dict[str, Callable[[list[bytes]], int]] = {
    "jsonmodem_events": jsonmodem_events,
    "jsonmodem_events_unpack": jsonmodem_events_unpack,
    "jsonmodem_feed_chunks_events": jsonmodem_feed_chunks_events,
    "jsonmodem_feed_chunks_events_unpack": jsonmodem_feed_chunks_events_unpack,
    "jsonmodem_pathfilter_byteviews": jsonmodem_pathfilter_byteviews,
    "jiter_cumulative_partial": jiter_cumulative_partial,
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--mode", choices=tuple(MODES), required=True)
    parser.add_argument("--chunks", type=int, default=50_000)
    parser.add_argument("--chunk-size", type=int, default=8)
    parser.add_argument("--repeats", type=int, default=1)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    data = make_doc(args.chunks, args.chunk_size)
    chunks = chunk_bytes(data, args.chunk_size)
    func = MODES[args.mode]
    result = 0
    start = perf_counter()
    for _ in range(args.repeats):
        result = func(chunks)
    elapsed = perf_counter() - start
    print(
        f"mode={args.mode} chunks={len(chunks)} chunk_size={args.chunk_size} "
        f"bytes={len(data)} repeats={args.repeats} result={result} elapsed={elapsed:.6f}s"
    )


if __name__ == "__main__":
    main()
