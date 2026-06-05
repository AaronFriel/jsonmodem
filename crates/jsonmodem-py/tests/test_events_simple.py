import pytest

from jsonmodem import (
    JsonModem,
    ParserOptions,
)


def collect_events(parser: JsonModem, text: str):
    events = list(parser.feed(text))
    events.extend(parser.finish())
    return events


def test_events_cover_scalars_and_structures():
    parser = JsonModem(ParserOptions())
    events = collect_events(parser, '{"a": [1, true, null], "b": "hi"}')

    assert events == [
        ("object_begin", (), None),
        ("array_begin", (("key", "a"),), None),
        ("number", (("key", "a"), ("index", 0)), 1.0),
        ("bool", (("key", "a"), ("index", 1)), True),
        ("null", (("key", "a"), ("index", 2)), None),
        ("array_end", (("key", "a"),), None),
        (
            "string",
            (("key", "b"),),
            {"fragment": "hi", "is_initial": True, "is_final": True},
        ),
        ("object_end", (), None),
    ]


def test_feed_accepts_bytes_bytearray_and_memoryview_chunks():
    parser = JsonModem(ParserOptions())

    events = []
    events.extend(parser.feed(b'{"a":'))
    events.extend(parser.feed(bytearray(b' "')))
    events.extend(parser.feed(memoryview(b'hi"')))
    events.extend(parser.feed(b"}"))
    events.extend(parser.finish())

    assert events == [
        ("object_begin", (), None),
        (
            "string",
            (("key", "a"),),
            {"fragment": "hi", "is_initial": True, "is_final": True},
        ),
        ("object_end", (), None),
    ]


def test_feed_accepts_iterable_chunks_for_fragmented_string():
    chunks = [b'{"content":"', b"hello", b" world", b'","done":true}']

    repeated = JsonModem(ParserOptions())
    repeated_events = []
    for chunk in chunks:
        repeated_events.extend(repeated.feed(chunk))
    repeated_events.extend(repeated.finish())

    batched = JsonModem(ParserOptions())
    batched_events = list(batched.feed(chunks))
    batched_events.extend(batched.finish())

    assert batched_events == repeated_events
    assert [
        event[2]
        for event in batched_events
        if event[0] == "string" and event[1] == (("key", "content"),)
    ] == [
        {"fragment": "hello", "is_initial": True, "is_final": False},
        {"fragment": " world", "is_initial": False, "is_final": False},
        {"fragment": "", "is_initial": False, "is_final": True},
    ]


def test_feed_returns_path_and_payload_views_that_unpack_cleanly():
    parser = JsonModem(ParserOptions())

    events = list(parser.feed(b'{"a":"hi"}'))
    events.extend(parser.finish())

    assert events == [
        ("object_begin", (), None),
        (
            "string",
            (("key", "a"),),
            {"fragment": "hi", "is_initial": True, "is_final": True},
        ),
        ("object_end", (), None),
    ]

    kind, path, payload = events[1]
    assert kind == "string"
    assert path == (("key", "a"),)
    assert path.as_tuple() == (("key", "a"),)
    assert path.endswith("a")
    assert payload == {"fragment": "hi", "is_initial": True, "is_final": True}
    assert payload.fragment == "hi"
    assert payload.is_initial is True
    assert payload.is_final is True
    assert payload.as_dict() == {"fragment": "hi", "is_initial": True, "is_final": True}


def test_feed_rejects_invalid_utf8_bytes():
    parser = JsonModem(ParserOptions())

    try:
        parser.feed(b'{"bad":"\xff"}')
    except TypeError as exc:
        assert "valid UTF-8" in str(exc)
    else:
        raise AssertionError("feed() accepted invalid UTF-8 bytes")


def test_byte_views_return_memoryview_for_borrowed_string_payload():
    parser = JsonModem(ParserOptions(), byte_views=True)
    data = b'{"a": "hi"}'

    events = list(parser.feed(data))
    events.extend(parser.finish())

    assert events[0] == ("object_begin", (), None)
    kind, path, payload = events[1]
    assert kind == "string"
    assert path == (("key", "a"),)
    assert payload["is_initial"] is True
    assert payload["is_final"] is True
    assert payload["is_view"] is True
    assert isinstance(payload["fragment"], memoryview)
    assert payload["fragment"].obj is data
    assert bytes(payload["fragment"]) == b"hi"
    assert payload["fragment"].tobytes().decode() == "hi"


def test_byte_views_accept_readonly_memoryview_input():
    parser = JsonModem(ParserOptions(), byte_views=True)

    events = list(parser.feed(memoryview(b'["ok"]')))
    events.extend(parser.finish())

    kind, path, payload = events[1]
    assert kind == "string"
    assert path == (("index", 0),)
    assert payload["is_view"] is True
    assert bytes(payload["fragment"]) == b"ok"


def test_byte_views_reject_non_byte_memoryview_input():
    parser = JsonModem(ParserOptions(), byte_views=True)
    data = memoryview(b'["AB"]').cast("H")

    with pytest.raises(TypeError, match="itemsize 1"):
        list(parser.feed(data))


def test_byte_views_materialize_escaped_fragments_as_text():
    parser = JsonModem(ParserOptions(), byte_views=True)

    events = list(parser.feed(b'["\\u0042"]'))
    events.extend(parser.finish())

    kind, path, payload = events[1]
    assert kind == "string"
    assert path == (("index", 0),)
    assert payload == {
        "fragment": "B",
        "is_initial": True,
        "is_final": True,
        "is_view": False,
    }


def test_byte_views_reject_str_and_mutable_input():
    parser = JsonModem(ParserOptions(), byte_views=True)

    with pytest.raises(TypeError, match="str input"):
        list(parser.feed('{"a": "hi"}'))

    with pytest.raises(TypeError, match="read-only"):
        list(parser.feed(bytearray(b'{"a": "hi"}')))

    mutable_owner = bytearray(b'{"a": "hi"}')
    with pytest.raises(TypeError, match="backed by bytes"):
        list(parser.feed(memoryview(mutable_owner).toreadonly()))


def test_path_filter_matches_wildcard_path():
    parser = JsonModem(paths="items.*.metadata.etag")
    data = b'{"items":[{"metadata":{"etag":"a"}},{"metadata":{"etag":"b"}}]}'

    events = list(parser.feed(data))
    events.extend(parser.finish())

    assert events == [
        (
            "string",
            (("key", "items"), ("index", 0), ("key", "metadata"), ("key", "etag")),
            {"fragment": "a", "is_initial": True, "is_final": True},
        ),
        (
            "string",
            (("key", "items"), ("index", 1), ("key", "metadata"), ("key", "etag")),
            {"fragment": "b", "is_initial": True, "is_final": True},
        ),
    ]


def test_path_filter_accepts_multiple_patterns():
    parser = JsonModem(paths=["content", "metadata.model"])
    data = b'{"content":"hello","metadata":{"model":"example"},"ignored":"x"}'

    events = list(parser.feed(data))
    events.extend(parser.finish())

    assert [event[0] for event in events] == ["string", "string"]
    assert events[0][1] == (("key", "content"),)
    assert events[0][2]["fragment"] == "hello"
    assert events[1][1] == (("key", "metadata"), ("key", "model"))
    assert events[1][2]["fragment"] == "example"


def test_path_filter_byte_views_only_materializes_matching_events():
    parser = JsonModem(paths="content", byte_views=True)
    data = b'{"ignored":"skip","content":"hello"}'

    events = list(parser.feed(data))
    events.extend(parser.finish())

    assert len(events) == 1
    kind, path, payload = events[0]
    assert kind == "string"
    assert path == (("key", "content"),)
    assert payload["is_view"] is True
    assert isinstance(payload["fragment"], memoryview)
    assert payload["fragment"].obj is data
    assert bytes(payload["fragment"]) == b"hello"


def test_path_filter_byte_views_rejects_str_input():
    parser = JsonModem(paths="content", byte_views=True)

    with pytest.raises(TypeError, match="str input"):
        list(parser.feed('{"content":"hello"}'))


def test_byte_views_accept_iterable_readonly_byte_chunks():
    parser = JsonModem(byte_views=True)

    events = list(parser.feed([b'{"a":"', b"hi", b'"}']))
    events.extend(parser.finish())

    fragments = [
        payload["fragment"]
        for kind, path, payload in events
        if kind == "string" and path == (("key", "a"),)
    ]
    assert b"".join(
        bytes(fragment) if isinstance(fragment, memoryview) else fragment.encode()
        for fragment in fragments
    ) == b"hi"
