"""Exercise buffer mutation during CPython 3.9-3.11 synchronous collection."""

import gc
import sys

from jsonmodem import JsonModem


def check(use_view):
    gc.disable()
    source = bytearray(b"[" + b",".join([b'"abc"'] * 200) + b"]")
    data = memoryview(source) if use_view else source
    parser = JsonModem()
    feed = parser.feed
    callbacks_during_feed = 0

    def mutate(phase, info):
        nonlocal callbacks_during_feed
        if phase == "start":
            try:
                parser.is_finished
            except RuntimeError:
                # PyO3 rejects this access while feed holds its mutable borrow.
                callbacks_during_feed += 1
                source[-3] = ord("z")

    gc.callbacks.append(mutate)
    gc.collect()
    gc.set_threshold(20, 10000, 10000)
    gc.enable()
    try:
        events = list(feed(data))
    finally:
        gc.disable()
        gc.callbacks.remove(mutate)
    print(f"memoryview={use_view}: callbacks during feed={callbacks_during_feed}")
    if sys.version_info < (3, 12):
        assert callbacks_during_feed > 0, "regression did not exercise synchronous GC"
    assert events[-2][2].fragment == "abc", "callback changed the text being parsed"


check(False)
check(True)
