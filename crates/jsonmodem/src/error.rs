use alloc::string::String;
use core::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct ParserError {
    msg: String,
    pub line: usize,
    pub column: usize,
}

impl fmt::Display for ParserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.msg.fmt(f)
    }
}

impl core::error::Error for ParserError {}
