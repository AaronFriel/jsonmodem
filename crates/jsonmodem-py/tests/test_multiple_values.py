import pytest

from jsonmodem import DecodeMode, JsonModem, JsonModemSyntaxError, ParserOptions


def test_multiple_values_disabled_raises_error():
    parser = JsonModem(ParserOptions(allow_multiple=False))

    with pytest.raises(JsonModemSyntaxError):
        list(parser.feed("{}{}"))


def test_multiple_values_enabled_streams_each_value():
    parser = JsonModem(
        ParserOptions(allow_multiple=True, decode_mode=DecodeMode.StrictUnicode)
    )

    events = list(parser.feed('{"x": 1} {"y": 2}'))
    events.extend(parser.finish())

    numbers = [event for event in events if event[0] == "number"]
    assert [item[1] for item in numbers] == [
        (("key", "x"),),
        (("key", "y"),),
    ]
    assert [item[2] for item in numbers] == [1.0, 2.0]
