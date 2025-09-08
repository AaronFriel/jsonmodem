import pytest

from jsonmodem import JsonModem, JsonModemSyntaxError


def test_invalid_character_reports_line_and_column():
    parser = JsonModem()

    with pytest.raises(JsonModemSyntaxError) as exc_info:
        list(parser.feed("!"))

    err = exc_info.value
    assert err.line == 1
    assert err.column == 1
    assert "InvalidCharacter" in str(err)
