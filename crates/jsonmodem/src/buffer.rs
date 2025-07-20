#![allow(clippy::inline_always)]

#[cfg(feature = "vecdeque-buffer")]
mod imp {
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

        pub(crate) fn copy_while<F>(&mut self, dst: &mut String, mut predicate: F) -> usize
        where
            F: FnMut(char) -> bool,
        {
            let mut copied = 0;
            loop {
                let (front, _) = self.data.as_slices();
                if front.is_empty() {
                    break;
                }

                let front_len = front.len();
                let prefix = front.iter().take_while(|&&ch| predicate(ch)).count();
                if prefix == 0 {
                    break;
                }

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

    pub(crate) use Buffer as Repr;
}

#[cfg(all(not(feature = "vecdeque-buffer"), feature = "string-buffer"))]
mod imp {
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

        pub(crate) fn copy_while<F>(&mut self, dst: &mut String, mut predicate: F) -> usize
        where
            F: FnMut(char) -> bool,
        {
            let mut count = 0;
            while let Some(ch) = self.peek() {
                if predicate(ch) {
                    self.consume_char();
                    dst.push(ch);
                    count += 1;
                } else {
                    break;
                }
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

    pub(crate) use Buffer as Repr;
}

#[cfg(all(not(feature = "vecdeque-buffer"), not(feature = "string-buffer")))]
mod imp {
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

    pub(crate) use Buffer as Repr;
}

pub(crate) use imp::Repr as Buffer;
