use thiserror::Error;

use crate::{backend::PathError, context::EventCtx};

/// Error returned while parsing JSON input.
#[derive(Error, Debug, PartialEq)]
#[error("{source} at {line}:{column}")]
pub struct ParserError<Ctx: EventCtx> {
    pub(crate) source: ErrorSource<Ctx>,
    pub(crate) line: usize,
    pub(crate) column: usize,
}

impl<Ctx: EventCtx> ParserError<Ctx> {
    /// Line number where the error occurred (1-based).
    #[must_use]
    pub fn line(&self) -> usize {
        self.line
    }

    /// Column number where the error occurred (1-based).
    #[must_use]
    pub fn column(&self) -> usize {
        self.column
    }

    /// Underlying error source (context or syntax failure).
    #[must_use]
    pub fn source(&self) -> &ErrorSource<Ctx> {
        &self.source
    }
}

#[derive(Error, Debug, PartialEq)]
pub enum ErrorSource<Ctx: EventCtx> {
    #[error("context error: {0}")]
    EventContextError(Ctx::Error),
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
    #[error("unexpected end of input")]
    UnexpectedEndOfInput,
    #[error(transparent)]
    PathError(PathError),
}
