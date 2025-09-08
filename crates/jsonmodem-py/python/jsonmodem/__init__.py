"""jsonmodem: streaming JSON parser bindings for Python."""

from . import _jsonmodem as _native

JsonModem = _native.JsonModem
ParserOptions = _native.ParserOptions
DecodeMode = _native.DecodeMode
JsonModemSyntaxError = _native.JsonModemSyntaxError
JsonModemStateError = _native.JsonModemStateError

__all__ = [
    "JsonModem",
    "ParserOptions",
    "DecodeMode",
    "JsonModemSyntaxError",
    "JsonModemStateError",
]

__version__ = getattr(_native, "__version__", "0.0.0")
