"""jsonmodem: high-performance streaming JSON parser (Rust bindings).

This package exposes a native extension module built with pyo3/maturin.
"""

# Re-export everything from the native extension and surface __version__
from ._jsonmodem import *  # noqa: F401,F403
from ._jsonmodem import __version__ as __version__  # noqa: F401

