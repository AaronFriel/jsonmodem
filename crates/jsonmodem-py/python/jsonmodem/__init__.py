"""jsonmodem: streaming JSON parser bindings for Python."""

from . import _jsonmodem as _native

JsonModem = _native.JsonModem
PathView = _native.PathView
StringPayload = _native.StringPayload
JsonModemByteViews = _native.JsonModemByteViews
JsonModemPathFilter = _native.JsonModemPathFilter
ParserOptions = _native.ParserOptions
DecodeMode = _native.DecodeMode
JsonModemSyntaxError = _native.JsonModemSyntaxError
JsonModemStateError = _native.JsonModemStateError

__all__ = [
    "JsonModem",
    "PathView",
    "StringPayload",
    "JsonModemByteViews",
    "JsonModemPathFilter",
    "ParserOptions",
    "DecodeMode",
    "JsonModemSyntaxError",
    "JsonModemStateError",
]

__version__ = getattr(_native, "__version__", "0.0.0")
