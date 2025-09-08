from jsonmodem import JsonModem


def _string_events(events):
    return [event for event in events if event[0] == "string"]


def test_string_spanning_multiple_chunks_emits_fragments():
    parser = JsonModem()

    part_one = list(parser.feed('{"s":"hel'))
    fragments_one = _string_events(part_one)
    assert fragments_one
    first_fragment = fragments_one[0]
    assert first_fragment[1] == (("key", "s"),)
    assert first_fragment[2]["fragment"] == "hel"
    assert first_fragment[2]["is_initial"] is True
    assert first_fragment[2]["is_final"] is False

    part_two = list(parser.feed('lo"}'))
    fragments_two = _string_events(part_two)
    assert fragments_two
    second_fragment = fragments_two[0]
    assert second_fragment[1] == (("key", "s"),)
    assert second_fragment[2]["fragment"] == "lo"
    assert second_fragment[2]["is_initial"] is False
    assert second_fragment[2]["is_final"] is True
    assert part_two[-1][0] == "object_end"

    assert list(parser.finish()) == []
