#![allow(clippy::inline_always)]

use alloc::{string::String, vec::Vec};

#[derive(Debug)]
pub(crate) struct Buffer {
    data: Vec<char>,
    pos: usize,
}

impl Buffer {
    pub(crate) fn new() -> Self {
        Self {
            data: Vec::new(),
            pos: 0,
        }
    }

    pub(crate) fn push(&mut self, text: &str) {
        self.data.extend(text.chars());
    }

    #[inline(always)]
    pub(crate) fn peek(&self) -> Option<char> {
        self.data.get(self.pos).copied()
    }

    #[inline(always)]
    fn consume_char(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += 1;
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
        while let Some(&ch) = self.data.get(self.pos) {
            if predicate(ch) {
                self.pos += 1;
            } else {
                break;
            }
        }
        dst.extend(self.data[start..self.pos].iter());
        let copied = self.pos - start;
        if self.pos > 4096 && self.pos > self.data.len() / 2 {
            self.data.drain(..self.pos);
            self.pos = 0;
        }
        copied
    }
}

impl Iterator for Buffer {
    type Item = char;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        self.consume_char()
    }
}
