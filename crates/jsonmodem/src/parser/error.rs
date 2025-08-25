use thiserror::Error;

use crate::backend::EventCtx;

#[derive(Error, Debug, PartialEq)]
#[error("{source} at {line}:{column}")]
pub struct ParserError<B: EventCtx> {
    pub(crate) source: ErrorSource<B>,
    pub(crate) line: usize,
    pub(crate) column: usize,
}

#[derive(Error, Debug, PartialEq)]
pub enum ErrorSource<B: EventCtx> {
    #[error("context error: {0}")]
    EventContextError(B::Error),
    #[error("syntax error: {0}")]
    SyntaxError(#[from] SyntaxError),
}

#[derive(Debug, Error, PartialEq)]
pub enum SyntaxError {
    #[error("invalid character '{0}'")]
    InvalidCharacter(char),
    #[error("invalid unicode escape sequence at character: '{0}'")]
    InvalidUnicodeEscapeChar(char),
    #[error("invalid unicode escape sequence \\u{0:X}")]
    InvalidUnicodeEscapeSequence(u32),
    #[error("{0}")]
    SyntaxError(&'static str),
    #[error("unexpected end of input")]
    UnexpectedEndOfInput,
}
