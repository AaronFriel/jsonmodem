#![allow(clippy::inline_always)]

use alloc::string::String;

#[derive(Debug)]
pub(crate) struct Buffer {
    data: String,
    pos: usize,
}

impl Buffer {
    pub(crate) fn new() -> Self {
        Self {
            data: String::new(),
            pos: 0,
        }
    }

    pub(crate) fn push(&mut self, text: &str) {
        self.data.push_str(text);
    }

    #[inline(always)]
    pub(crate) fn peek(&self) -> Option<char> {
        self.data[self.pos..].chars().next()
    }

    #[inline(always)]
    fn consume_char(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += ch.len_utf8();
        if self.pos > 4096 && self.pos > self.data.len() / 2 {
            self.data.drain(..self.pos);
            self.pos = 0;
        }
        Some(ch)
    }

    /// Copy characters from the buffer into the provided `String` while the
    /// supplied predicate returns `true` for the next character. Stops at the
    /// first character for which the predicate returns `false` **or** when the
    /// buffer is exhausted.
    ///
    /// Returns the number of characters that have been copied.
    pub(crate) fn copy_while<F>(&mut self, dst: &mut String, mut predicate: F) -> usize
    where
        F: FnMut(char) -> bool,
    {
        let start = self.pos;
        let mut bytes_end = self.pos;
        let mut count = 0;
        for (offset, ch) in self.data[start..].char_indices() {
            if predicate(ch) {
                bytes_end = start + offset + ch.len_utf8();
                count += 1;
            } else {
                break;
            }
        }
        dst.push_str(&self.data[start..bytes_end]);
        self.pos = bytes_end;
        if self.pos > 4096 && self.pos > self.data.len() / 2 {
            self.data.drain(..self.pos);
            self.pos = 0;
        }
        count
    }
}

impl Iterator for Buffer {
    type Item = char;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        self.consume_char()
    }
}
