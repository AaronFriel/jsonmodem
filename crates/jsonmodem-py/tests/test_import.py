import importlib


def test_import():
    mod = importlib.import_module("jsonmodem")
    assert hasattr(mod, "__doc__")
    # version is forwarded from the native module
    assert isinstance(getattr(mod, "__version__", None), str)
