import pytest

from jsonmodem import JsonModemCompletedSubtrees, ParserOptions


def test_completed_subtrees_emit_selected_items_in_document_order():
    parser = JsonModemCompletedSubtrees(paths="items.*")

    records = list(
        parser.feed_many(
            [
                b'{"items":[{"id":1,"text":"a"},',
                b'{"id":2,"text":"b"}],"ignored":{"id":3}}',
            ]
        )
    )
    records.extend(parser.finish())

    assert [(path.as_tuple(), value, released) for path, value, released in records] == [
        ((("key", "items"), ("index", 0)), {"id": 1.0, "text": "a"}, True),
        ((("key", "items"), ("index", 1)), {"id": 2.0, "text": "b"}, True),
    ]


def test_completed_subtrees_carry_incomplete_utf8_between_byte_chunks():
    parser = JsonModemCompletedSubtrees(paths="items.*")

    records = list(parser.feed_many([b'{"items":[{"text":"caf\xc3', b'\xa9"}]}']))
    records.extend(parser.finish())

    assert [(path.as_tuple(), value, released) for path, value, released in records] == [
        ((("key", "items"), ("index", 0)), {"text": "café"}, True)
    ]


def test_completed_subtrees_do_not_emit_truncated_final_item():
    parser = JsonModemCompletedSubtrees(paths="items.*")

    records = list(parser.feed_many([b'{"items":[{"id":1},', b'{"id":2']))
    records.extend(parser.finish())

    assert [(path.as_tuple(), value, released) for path, value, released in records] == [
        ((("key", "items"), ("index", 0)), {"id": 1.0}, True)
    ]


def test_completed_subtrees_support_multiple_roots_when_enabled():
    parser = JsonModemCompletedSubtrees(
        ParserOptions(allow_multiple=True),
        paths="items.*",
    )

    records = list(parser.feed_many([b'{"items":[{"id":1}]}', b'{"items":[{"id":2}]}']))
    records.extend(parser.finish())

    assert [(path.as_tuple(), value) for path, value, _released in records] == [
        ((("key", "items"), ("index", 0)), {"id": 1.0}),
        ((("key", "items"), ("index", 0)), {"id": 2.0}),
    ]


def test_completed_subtrees_report_retained_null_placeholders_after_release():
    parser = JsonModemCompletedSubtrees(paths="items.*", release_after_emit=True)

    records = list(parser.feed_many([b'{"items":[{"id":1},{"id":2}]}']))

    assert len(records) == 2
    retained = parser.retained_state()
    assert retained["array_slots"] >= 2
    assert retained["released_array_entries_retained_as_null"] == 2


def test_completed_subtrees_do_not_count_json_nulls_as_released_placeholders():
    parser = JsonModemCompletedSubtrees(paths="items.*", release_after_emit=True)

    records = list(parser.feed_many([b'{"items":[null,{"id":1}],"other":null}']))

    assert len(records) == 1
    retained = parser.retained_state()
    assert retained["nulls"] == 3
    assert retained["released_array_entries_retained_as_null"] == 1


def test_completed_subtrees_retained_state_has_no_placeholders_without_release():
    parser = JsonModemCompletedSubtrees(paths="items.*", release_after_emit=False)

    records = list(parser.feed_many([b'{"items":[{"id":1},null],"other":null}']))

    assert len(records) == 1
    retained = parser.retained_state()
    assert retained["nulls"] == 2
    assert retained["released_array_entries_retained_as_null"] == 0


def test_completed_subtrees_feed_many_rejects_scalar_input_and_reset_reuses_config():
    parser = JsonModemCompletedSubtrees(paths="items.*")

    with pytest.raises(TypeError, match="use feed"):
        list(parser.feed_many(b'{"items":[{"id":1}]}'))

    list(parser.feed_many([b'{"items":[{"id":1}]}']))
    list(parser.finish())
    assert parser.is_finished is True

    parser.reset()
    assert parser.is_finished is False
    records = list(parser.feed_many([b'{"items":[{"id":2}]}']))
    assert records[0][1] == {"id": 2.0}
