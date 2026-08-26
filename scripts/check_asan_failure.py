"""Require a sanitizer diagnostic from the deliberately invalid test library."""

import pathlib
import subprocess
import sys


runner, python, library, output = sys.argv[1:]
result = subprocess.run(
    [runner, python, "-c", "import ctypes, sys; ctypes.CDLL(sys.argv[1]).check_address_sanitizer()", library],
    stdout=subprocess.PIPE,
    stderr=subprocess.STDOUT,
    timeout=60,
)
pathlib.Path(output).write_bytes(result.stdout)
if result.returncode == 0 or b"ERROR: AddressSanitizer: heap-buffer-overflow" not in result.stdout:
    sys.stderr.buffer.write(result.stdout)
    raise SystemExit("AddressSanitizer did not detect the deliberate out-of-bounds read")
print("AddressSanitizer detected the deliberate out-of-bounds read")
