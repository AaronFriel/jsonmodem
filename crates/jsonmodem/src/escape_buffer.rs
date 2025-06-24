//! Utilities for buffering and decoding four-digit Unicode escape sequences.
//!
//! The [`UnicodeEscapeBuffer`] type accumulates up to four ASCII hexadecimal
//! digits (`0-9`, `A-F`, `a-f`) representing a Unicode code point and converts
//! them to a [`char`] once exactly four digits have been provided. After a
//! successful conversion, the buffer resets automatically to begin a new escape
//! sequence.
//!
//! # Errors
//!
//! - Feeding a non-hexadecimal character returns an `Err` with a descriptive
//!   message.
//! - If more than four digits are provided without a successful conversion
//!   (buffer overflow), an `Err` is returned.
//! - If parsing the four-digit hexadecimal string into a `u32` fails, an `Err`
//!   is returned.
//!
//! # Panics
//!
//! If the parsed code point is not a valid Unicode scalar value (for example,
//! a surrogate or out of range), this will panic.
use alloc::{
    format,
    string::{String, ToString},
};

#[derive(Debug)]
/// Buffer for accumulating up to four hexadecimal digits (`0-9`, `A-F`, `a-f`)
/// and decoding them into a Unicode character.
///
/// This type is useful for JSON parsers or similar, where Unicode escapes
/// (e.g. `"\u0041"`) must be interpreted as `char` values.
pub(crate) struct UnicodeEscapeBuffer {
    buffer: [u8; 4],
    len: u8,
}

impl UnicodeEscapeBuffer {
    /// Creates a new, empty `UnicodeEscapeBuffer`.
    ///
    /// The buffer will accept up to four hexadecimal digits before decoding.
    pub fn new() -> Self {
        Self {
            buffer: [0; 4],
            len: 0,
        }
    }

    /// Clears any accumulated digits, returning the buffer to its initial
    /// state.
    pub fn reset(&mut self) {
        self.len = 0;
    }

    /// Feeds a single ASCII hexadecimal digit (`0-9`, `A-F`, `a-f`) into the
    /// buffer.
    ///
    /// - Returns `Ok(None)` if fewer than four digits have been provided so
    ///   far.
    /// - Returns `Ok(Some(ch))` once exactly four digits have been accumulated,
    ///   decoding them to the corresponding `char` and resetting the buffer.
    /// - Returns `Err` if `c` is not an ASCII hex digit, if more than four
    ///   digits are provided before a reset, or if parsing the digits into a
    ///   `u32` fails.
    pub fn feed(&mut self, c: char) -> Result<Option<char>, String> {
        if !c.is_ascii_hexdigit() {
            return Err(format!("Invalid unicode escape character: {c}"));
        }

        if self.len >= 4 {
            return Err("Unicode escape buffer overflow".to_string());
        }
        self.buffer[self.len as usize] = c as u8;
        self.len += 1;

        if self.len == 4 {
            let hex_str = core::str::from_utf8(&self.buffer).unwrap();
            match u32::from_str_radix(hex_str, 16) {
                Ok(code) => {
                    self.reset(); // Reset after successful conversion
                    Ok(Some(
                        core::char::from_u32(code)
                            .ok_or(format!("Invalid Unicode scalar value: {code}"))?,
                    ))
                }
                Err(e) => Err(format!("Failed to parse unicode escape: {e}")),
            }
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::UnicodeEscapeBuffer;

    #[test]
    fn basic_decoding() {
        let mut buf = UnicodeEscapeBuffer::new();
        assert_eq!(buf.feed('0').unwrap(), None);
        assert_eq!(buf.feed('0').unwrap(), None);
        assert_eq!(buf.feed('4').unwrap(), None);
        assert_eq!(buf.feed('1').unwrap(), Some('A'));
    }

    #[test]
    fn mixed_case_hex() {
        let mut buf = UnicodeEscapeBuffer::new();
        for ch in "AbCd".chars() {
            let res = buf.feed(ch).unwrap();
            if ch == 'd' {
                assert_eq!(res, Some(char::from_u32(0xABCD).unwrap()));
            } else {
                assert!(res.is_none());
            }
        }
    }

    #[test]
    fn reset_clears_buffer() {
        let mut buf = UnicodeEscapeBuffer::new();
        assert!(buf.feed('F').unwrap().is_none());
        buf.reset();
        // After reset, previous input is discarded
        assert_eq!(buf.feed('0').unwrap(), None);
    }

    #[test]
    fn invalid_hex_error() {
        let mut buf = UnicodeEscapeBuffer::new();
        let err = buf.feed('G').unwrap_err();
        assert!(err.contains("Invalid unicode escape character"));
    }

    #[test]
    #[should_panic(expected = "called `Option::unwrap()` on a `None` value")]
    fn invalid_scalar_panics() {
        // 'D800' is a surrogate range high half and not a valid scalar
        let mut buf = UnicodeEscapeBuffer::new();
        for ch in "D800".chars() {
            let _ = buf.feed(ch).unwrap();
        }
    }
}
