import pytest

from jsonmodem import JsonModemStateError, JsonModemValues, ParserOptions


def test_values_reuse_root_view_and_report_path_views():
    parser = JsonModemValues()

    first = list(parser.feed(b'{"message":"hel'))
    root_view = first[-1][1]
    second = list(parser.feed(b'lo"'))
    final = list(parser.feed(b"}"))

    assert first[-1][0] == 0
    assert first[-1][1] is root_view
    assert first[-1][2].as_tuple() == (("key", "message"),)
    assert first[-1][2].endswith("message")
    assert root_view["message"].snapshot() == "hello"

    assert second[-1][1] is root_view
    assert second[-1][2].as_tuple() == (("key", "message"),)
    assert final[-1] == (0, root_view, final[-1][2], True)
    assert final[-1][2].as_tuple() == ()
    assert parser.view() is root_view


def test_values_feed_accepts_iterable_chunks():
    parser = JsonModemValues()

    updates = list(parser.feed([b'{"items":[', b"1,", b"2", b"]}"]))
    updates.extend(parser.finish())

    root_view = updates[-1][1]
    assert updates[-1][0] == 0
    assert updates[-1][2].as_tuple() == ()
    assert updates[-1][3] is True
    assert root_view.snapshot() == {"items": [1.0, 2.0]}
    assert len(root_view["items"]) == 2


def test_values_string_view_len_matches_python_string_len():
    parser = JsonModemValues()

    updates = list(parser.feed('{"emoji":"🙂","word":"café"}'))
    updates.extend(parser.finish())

    root_view = updates[-1][1]
    assert root_view["emoji"].snapshot() == "🙂"
    assert len(root_view["emoji"]) == len("🙂") == 1
    assert root_view["word"].snapshot() == "café"
    assert len(root_view["word"]) == len("café") == 4


def test_values_carry_incomplete_utf8_between_byte_chunks():
    parser = JsonModemValues()

    updates = list(parser.feed([b'{"word":"caf\xc3', b'\xa9"}']))
    updates.extend(parser.finish())

    root_view = updates[-1][1]
    assert root_view.snapshot() == {"word": "café"}
    assert len(root_view["word"]) == 4


def test_values_update_rejects_finish_with_incomplete_utf8():
    parser = JsonModemValues()

    parser.update(b'{"word":"caf\xc3')

    with pytest.raises(TypeError, match="incomplete UTF-8"):
        parser.finish(changed_paths=False)


def test_values_view_before_input_is_empty():
    parser = JsonModemValues()

    view = parser.view()

    assert view.kind == "empty"
    assert view.snapshot() is None


def test_values_stream_multiple_roots():
    parser = JsonModemValues(ParserOptions(allow_multiple=True))

    finals = []
    for index, view, _path, is_final in parser.feed(b'{"a":1}'):
        if is_final:
            finals.append((index, view.snapshot()))
    for index, view, _path, is_final in parser.feed(b' {"b":2}'):
        if is_final:
            finals.append((index, view.snapshot()))
    for index, view, _path, is_final in parser.finish():
        if is_final:
            finals.append((index, view.snapshot()))

    assert finals == [
        (0, {"a": 1.0}),
        (1, {"b": 2.0}),
    ]


def test_values_reject_feed_after_finish():
    parser = JsonModemValues()

    list(parser.feed(b'{"ok":true}'))
    list(parser.finish())

    with pytest.raises(JsonModemStateError):
        list(parser.feed(b'{"again": true}'))

    with pytest.raises(JsonModemStateError):
        list(parser.finish())


def test_values_update_reuses_one_readonly_view_without_notifications():
    parser = JsonModemValues()
    view = parser.view()

    returned = parser.update([b'{"message":"h', b'i","items":[1', b",2]}"])
    finished = parser.finish(changed_paths=False)

    assert returned is view
    assert finished is view
    assert view.snapshot() == {"message": "hi", "items": [1.0, 2.0]}
    assert view["message"].snapshot() == "hi"


def test_values_update_changed_paths_are_optional_and_include_duplicate_keys():
    parser = JsonModemValues()

    summary = parser.update(b'{"a":1,"a":2}', changed_paths=True)
    finished = parser.finish(changed_paths=True)

    assert summary["view"] is parser.view()
    assert finished["view"] is parser.view()
    assert summary["view"].snapshot() == {"a": 2.0}

    changed = [path.as_tuple() for path in summary["changed_paths"]]
    assert changed[0] == ()
    assert changed.count((("key", "a"),)) == 2
    assert changed[-1] == ()
    assert finished["changed_paths"] == ()


def test_values_reset_preserves_outstanding_view_object_as_live_empty_view():
    parser = JsonModemValues()
    view = parser.update(b'{"a":1}')
    parser.finish(changed_paths=False)

    assert parser.is_finished is True
    assert view.snapshot() == {"a": 1.0}

    parser.reset()

    assert parser.is_finished is False
    assert view is parser.view()
    assert view.snapshot() is None
    assert parser.update(b'{"b":2}') is view
    assert view.snapshot() == {"b": 2.0}
