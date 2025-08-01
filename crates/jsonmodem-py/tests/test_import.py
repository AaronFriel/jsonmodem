import importlib

def test_import():
    mod = importlib.import_module("jsonmodem")
    assert hasattr(mod, "__doc__")
