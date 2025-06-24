use alloc::string::{String, ToString};
use core::{iter::Peekable, str::Chars};

use ouroboros::self_referencing;

#[self_referencing]
#[derive(Debug)]
struct OwnedPeekableString {
    buffer: alloc::string::String,
    #[borrows(buffer)]
    #[not_covariant]
    peekable: Peekable<Chars<'this>>,
}

#[derive(Debug)]
pub(crate) struct Buffer {
    tail: Option<String>,
    head: Option<OwnedPeekableString>,
}

impl Buffer {
    pub(crate) fn new() -> Self {
        Self {
            tail: None,
            head: None,
        }
    }

    pub(crate) fn push(&mut self, text: &str) {
        match self.tail {
            Some(ref mut tail) => {
                tail.push_str(text);
            }
            None => {
                self.tail = Some(text.to_string());
            }
        }
    }

    pub(crate) fn peek(&mut self) -> Option<char> {
        loop {
            match self.head {
                Some(ref mut head) => {
                    if let Some(c) = head.with_peekable_mut(|p| p.peek()) {
                        return Some(*c);
                    }

                    self.head = None; // Clear the head if it's exhausted
                }
                None => {
                    if let Some(next) = core::mem::take(&mut self.tail) {
                        self.head = Some(
                            OwnedPeekableStringBuilder {
                                buffer: next,
                                peekable_builder: |buffer| buffer.chars().peekable(),
                            }
                            .build(),
                        );
                    } else {
                        return None; // No more characters to peek
                    }
                }
            }
        }
    }
}

impl Iterator for Buffer {
    type Item = char;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.head {
                Some(ref mut head) => {
                    if let Some(c) = head.with_peekable_mut(|p| p.next()) {
                        return Some(c);
                    }

                    self.head = None; // Clear the head if it's exhausted
                }
                None => {
                    if let Some(next) = core::mem::take(&mut self.tail) {
                        self.head = Some(
                            OwnedPeekableStringBuilder {
                                buffer: next,
                                peekable_builder: |buffer| buffer.chars().peekable(),
                            }
                            .build(),
                        );
                    } else {
                        return None;
                    }
                }
            }
        }
    }
}
