import json
import struct

import pytest

from jsonmodem import loads, string_range_table, string_ranges


def test_loads_matches_stdlib_for_nested_value():
    text = b'{"a":[1,true,null],"b":{"c":"hello","d":2.5}}'

    assert loads(text) == json.loads(text)
    assert loads(text.decode()) == json.loads(text)


def test_loads_rejects_multiple_values():
    with pytest.raises(Exception, match="InvalidCharacter"):
        loads(b"{}{}")


def test_loads_rejects_invalid_utf8():
    with pytest.raises(TypeError, match="valid UTF-8"):
        loads(b'{"bad":"\xff"}')


def test_string_ranges_reports_payload_offsets_for_unescaped_values():
    data = b'{"a":"hello","b":["x","yz"]}'

    ranges = string_ranges(data)

    assert ranges == [(6, 11), (19, 20), (23, 25)]
    assert [data[start:end] for start, end in ranges] == [b"hello", b"x", b"yz"]


def test_string_ranges_marks_escaped_values_materialized():
    data = b'["plain","line\\nfeed"]'

    assert string_ranges(data) == [(2, 7), None]


def test_string_ranges_requires_bytes():
    with pytest.raises(TypeError, match="expected bytes"):
        string_ranges('["text"]')


def test_string_range_table_packs_offsets_without_per_value_tuples():
    data = b'["plain","line\\nfeed"]'

    table = string_range_table(data)

    assert len(table) == 16
    rows = struct.unpack("<IIII", table)
    assert rows == (2, 7, 2**32 - 1, 2**32 - 1)


def test_string_range_table_allows_empty_containers():
    assert string_range_table(b"[]") == b""
    assert string_range_table(b"{}") == b""
    assert string_range_table(b'{"empty":[],"nested":{}}') == b""


@pytest.mark.parametrize("data", [b'["x",]', b'{"a":1,}', b'{"a":["x",]}'])
def test_string_range_table_rejects_trailing_commas(data):
    with pytest.raises(Exception, match="not valid|invalid"):
        string_range_table(data)
