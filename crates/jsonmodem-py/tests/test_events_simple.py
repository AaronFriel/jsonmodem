from jsonmodem import JsonModem, ParserOptions


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
