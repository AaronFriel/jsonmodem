"""Buffer ownership and Python object lifetime checks for the streaming APIs."""

import gc
import json
import os
from pathlib import Path
import subprocess
import sys

import pytest

from jsonmodem import JsonModem, JsonModemSyntaxError, JsonModemValues


def test_gc_callback_cannot_change_text_being_parsed():
    runner = os.environ.get("JSONMODEM_MEMORY_RUNNER")
    command = [runner] if runner else []
    script = Path(__file__).resolve().parents[3] / "scripts" / "gc_buffer_regression.py"
    result = subprocess.run(
        [*command, sys.executable, str(script)], capture_output=True, text=True, timeout=30
    )
    assert result.returncode == 0, result.stdout + result.stderr


@pytest.mark.parametrize("factory", [bytes, bytearray, memoryview])
@pytest.mark.parametrize("parser_type", [JsonModem, JsonModemValues])
def test_empty_and_invalid_utf8_release_input(factory, parser_type):
    parser = parser_type()
    assert list(parser.feed(factory(b""))) == []
    with pytest.raises(TypeError, match="valid UTF-8"):
        list(parser.feed(factory(b'"\xff"')))
    list(parser.feed(factory(b'["ok"]')))
    list(parser.finish())


@pytest.mark.parametrize("parser_type", [JsonModem, JsonModemValues])
def test_mutable_input_can_resize_after_success_and_error(parser_type):
    for data in [b'["ok"]', b'"\xff"', b'[?]']:
        source = bytearray(data)
        parser = parser_type()
        try:
            events = list(parser.feed(source))
        except (TypeError, JsonModemSyntaxError):
            events = []
        # A leaked Py_buffer would keep bytearray resizing disabled.
        source.extend(b"extra")
        del events, parser
        source.clear()


@pytest.mark.parametrize("parser_type", [JsonModem, JsonModemValues])
def test_released_and_noncontiguous_views_are_rejected(parser_type):
    released = memoryview(b'["ok"]')
    released.release()
    for source in [released, memoryview(b'["ok"]')[::2]]:
        with pytest.raises((TypeError, ValueError, BufferError)):
            list(parser_type().feed(source))


@pytest.mark.parametrize("byte_views", [False, True])
def test_sliced_readonly_buffer_and_payload_outlive_input(byte_views):
    source = bytes(bytearray(b'prefix{"outer":[{"text":"value"}]}suffix'))
    view = memoryview(source)[6:-6]
    parser = JsonModem(byte_views=byte_views)
    events = list(parser.feed(view))
    list(parser.finish())
    view.release()
    del source, view, parser
    gc.collect()
    kind, path, payload = next(event for event in events if event[0] == "string")
    assert kind == "string"
    expected = (("key", "outer"), ("index", 0), ("key", "text"))
    assert tuple(path) == expected
    assert path[::-1] == expected[::-1]
    assert path[::2] == expected[::2]
    assert path[-100:100] == expected
    assert path[1:1] == ()
    with pytest.raises(ValueError):
        path[::0]
    if byte_views:
        assert payload["fragment"].tobytes() == b"value"
    else:
        assert path.as_tuple() == expected
        assert payload.fragment == "value"


def test_owned_events_do_not_change_after_bytearray_mutation():
    source = bytearray(b'{"text":"original"}')
    parser = JsonModem()
    events = list(parser.feed(source))
    source[:] = b"replaced"
    assert events[1][2].fragment == "original"
    assert events[1][1].as_tuple() == (("key", "text"),)


def test_values_outlive_parser_and_input():
    source = bytearray(b'{"items":[{"text":"value"}]}')
    parser = JsonModemValues()
    events = list(parser.feed(source))
    list(parser.finish())
    value = events[-1][1]
    child = value["items"][0]["text"]
    del parser, source, events
    gc.collect()
    assert child.snapshot() == "value"
    assert value.snapshot() == {"items": [{"text": "value"}]}


@pytest.mark.parametrize("byte_views", [False, True])
def test_iterator_drop_and_repeated_allocation_preserve_saved_events(byte_views):
    saved = []
    for index in range(128):
        parser = JsonModem(byte_views=byte_views)
        events = parser.feed(f'{{"text":"value{index}"}}'.encode())
        assert next(events)[0] == "object_begin"
        saved.append(next(events))
        del events, parser
    gc.collect()
    for index, (_, path, payload) in enumerate(saved):
        assert tuple(path) == (("key", "text"),)
        fragment = payload["fragment"] if byte_views else payload.fragment
        if isinstance(fragment, memoryview):
            fragment = fragment.tobytes().decode()
        assert fragment == f"value{index}"


def test_chunk_generator_exception_releases_buffer():
    source = bytearray(b'{"text":"value"}')

    def chunks():
        yield source
        raise RuntimeError("chunk generator failed")

    with pytest.raises(RuntimeError, match="chunk generator failed"):
        list(JsonModem().feed(chunks()))
    source.clear()


def test_all_single_byte_string_mutations_release_input():
    for byte in range(256):
        source = bytearray(b'["a' + bytes([byte]) + b'"]')
        try:
            expected = json.loads(source)
        except (ValueError, UnicodeDecodeError):
            expected = None
        parser = JsonModemValues()
        try:
            events = list(parser.feed(source))
            events.extend(parser.finish())
        except (TypeError, JsonModemSyntaxError):
            assert expected is None
        else:
            assert expected is not None
            assert events[-1][1].snapshot() == expected
        source.clear()


def test_contiguous_multidimensional_buffer_preserves_byte_order():
    source = memoryview(b'["ok"]').cast("B", shape=[2, 3])
    parser = JsonModem()
    events = list(parser.feed(source))
    list(parser.finish())
    assert events[1][2].fragment == "ok"


@pytest.mark.skipif(sys.version_info < (3, 12), reason="Python buffer methods require 3.12")
@pytest.mark.parametrize("data", [b'["ok"]', b'"\xff"'])
def test_python_exporter_releases_every_acquired_buffer(data):
    class Exporter:
        """Counts live views without fabricating native buffer metadata."""

        def __init__(self):
            self.active = 0

        def __buffer__(self, flags):
            self.active += 1
            return memoryview(data)

        def __release_buffer__(self, buffer):
            self.active -= 1

    source = Exporter()
    parser = JsonModem()
    if b"\xff" in data:
        with pytest.raises(TypeError, match="valid UTF-8"):
            list(parser.feed(source))
    else:
        list(parser.feed(source))
    assert source.active == 0


@pytest.mark.skipif(sys.version_info < (3, 12), reason="Python buffer methods require 3.12")
def test_byte_payload_snapshots_unknown_python_exporter():
    class Exporter:
        """Owns storage whose immutability cannot be established by the caller."""

        def __init__(self):
            self.active = 0
            self.data = bytearray(b'["value"]')

        def __buffer__(self, flags):
            self.active += 1
            return memoryview(self.data).toreadonly()

        def __release_buffer__(self, buffer):
            self.active -= 1

    source = Exporter()
    parser = JsonModem(byte_views=True)
    events = list(parser.feed(source))
    payload = events[1][2]["fragment"]
    del parser, events
    gc.collect()
    assert payload.tobytes() == b"value"
    assert source.active == 0
    assert isinstance(payload.obj, bytes)
    source.data[:] = b"changed"
    assert payload.tobytes() == b"value"
    payload.release()
    gc.collect()
    assert source.active == 0


@pytest.mark.skipif(sys.version_info < (3, 12), reason="Python buffer methods require 3.12")
def test_failed_export_does_not_call_release():
    class Exporter:
        """Fails before returning storage to CPython."""

        def __buffer__(self, flags):
            raise BufferError("export failed")

        def __release_buffer__(self, buffer):
            pytest.fail("release called without a successful acquisition")

    with pytest.raises(TypeError):
        list(JsonModem().feed(Exporter()))


@pytest.mark.skipif(sys.version_info < (3, 12), reason="Python buffer methods require 3.12")
def test_byte_views_parse_the_same_export_as_the_retained_payload():
    class Exporter:
        """Returns different immutable storage on a later acquisition."""

        def __init__(self):
            self.calls = 0

        def __buffer__(self, flags):
            self.calls += 1
            return memoryview(b'["first"]' if self.calls <= 2 else b'["second"]')

    source = Exporter()
    parser = JsonModem(byte_views=True)
    events = list(parser.feed(source))
    assert events[1][2]["fragment"].tobytes() == b"first"
