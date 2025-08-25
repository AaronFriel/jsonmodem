//! Utilities for decoding four-digit Unicode escape sequences without buffering
//! bytes.
//!
//! The [`UnicodeEscapeBuffer`] type accumulates exactly four ASCII hexadecimal
//! digits (`0-9`, `A-F`, `a-f`) into a `u32` as they arrive, and converts that
//! to a [`char`] when the fourth digit is provided. After a successful
//! conversion, the accumulator resets automatically to begin a new escape.
//!
//! # Errors
//!
//! - Feeding a non-hexadecimal character returns an `Err` with a descriptive
//!   message.
//! - If more than four digits are provided without a successful conversion, an
//!   `Err` is returned (unreachable in this implementation; we guard).
//! - If the resulting code point is not a valid Unicode scalar value (e.g. a
//!   surrogate), an `Err` is returned.
//!
//! # Panics
//!
//! None in normal operation.

use crate::parser::error::SyntaxError;

#[derive(Debug)]
/// Accumulates up to four hexadecimal digits and decodes them into a Unicode
/// character.
///
/// This type is useful for JSON parsers or similar, where Unicode escapes
/// (e.g. `"\u0041"`) must be interpreted as `char` values.
pub(crate) struct UnicodeEscapeBuffer {
    acc: u32,
    len: u8,
}

impl UnicodeEscapeBuffer {
    /// Creates a new, empty `UnicodeEscapeBuffer`.
    ///
    /// The buffer will accept up to four hexadecimal digits before decoding.
    pub fn new() -> Self {
        Self { acc: 0, len: 0 }
    }

    /// Clears any accumulated digits, returning the buffer to its initial
    /// state.
    pub fn reset(&mut self) {
        self.acc = 0;
        self.len = 0;
    }

    /// Convert a single ASCII hex digit into its 0..=15 value.
    #[inline]
    fn hex_val(c: char) -> Option<u32> {
        match c {
            '0'..='9' => Some((c as u32) - ('0' as u32)),
            'a'..='f' => Some((c as u32) - ('a' as u32) + 10),
            'A'..='F' => Some((c as u32) - ('A' as u32) + 10),
            _ => None,
        }
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
    pub fn feed(&mut self, c: char) -> Result<Option<char>, SyntaxError> {
        let d = Self::hex_val(c).ok_or(SyntaxError::InvalidUnicodeEscapeChar(c))?;

        // Guard against overflow of our fixed-size 4-digit escape
        if self.len >= 4 {
            unreachable!();
        }

        // acc = (acc << 4) | d
        self.acc = (self.acc << 4) | d;
        self.len += 1;

        if self.len < 4 {
            return Ok(None);
        }

        // Exactly 4 digits accumulated. Validate and produce a char, then reset.
        let code = self.acc;

        // Reset immediately so state is clean regardless of outcome.
        self.reset();

        match core::char::from_u32(code) {
            Some(ch) => Ok(Some(ch)),
            None => Err(SyntaxError::InvalidUnicodeEscapeSequence(code)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::UnicodeEscapeBuffer;
    use crate::parser::error::SyntaxError;

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
        assert_eq!(err, SyntaxError::InvalidUnicodeEscapeChar('G'));
    }

    #[test]
    fn invalid_scalar_errors() {
        // 'D800' is a surrogate range high half and not a valid scalar
        let mut buf = UnicodeEscapeBuffer::new();
        for ch in "D80".chars() {
            let _ = buf.feed(ch).unwrap();
        }
        let _ = buf.feed('0').unwrap_err();
    }
}
