import pytest

from jsonmodem import JsonModem, JsonModemStateError


def test_finish_transitions_parser_state():
    parser = JsonModem()
    assert parser.is_finished is False

    list(parser.feed("{}"))
    assert parser.is_finished is False

    list(parser.finish())
    assert parser.is_finished is True

    with pytest.raises(JsonModemStateError):
        parser.feed("{}")

    with pytest.raises(JsonModemStateError):
        parser.finish()
