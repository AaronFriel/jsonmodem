#![expect(clippy::inline_always)]

use alloc::{collections::VecDeque, string::String};

#[derive(Debug)]
pub(crate) struct Buffer {
    data: VecDeque<char>,
}

impl Buffer {
    pub(crate) fn new() -> Self {
        Self {
            data: VecDeque::new(),
        }
    }

    pub(crate) fn push(&mut self, text: &str) {
        // Reserve the byte length as an upper bound on additional chars
        self.data.reserve(text.len());
        self.data.extend(text.chars());
    }

    #[inline(always)]
    pub(crate) fn peek(&self) -> Option<char> {
        self.data.front().copied()
    }

    #[inline(always)]
    fn consume_char(&mut self) -> Option<char> {
        self.data.pop_front()
    }

    #[inline]
    pub(crate) fn copy_while<F>(&mut self, dst: &mut String, mut predicate: F) -> usize
    where
        F: FnMut(char) -> bool,
    {
        let mut copied = 0;
        loop {
            let (front_len, prefix) = {
                let (front, _) = self.data.as_slices();
                if front.is_empty() {
                    break;
                }

                let mut prefix = 0;
                for &ch in front {
                    if predicate(ch) {
                        prefix += 1;
                    } else {
                        break;
                    }
                }

                if prefix == 0 {
                    break;
                }

                (front.len(), prefix)
            };

            dst.extend(self.data.drain(..prefix));
            copied += prefix;

            if prefix < front_len {
                break;
            }
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
