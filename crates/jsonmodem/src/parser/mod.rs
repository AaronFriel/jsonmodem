//! The JSON streaming parser implementation.
//!
//! This module provides the incremental streaming parser that processes input
//! in chunks and emits `ParseEvent`s. The core does not build composite values
//! or buffer full strings; adapters are responsible for those behaviors.

#![expect(clippy::single_match_else)]
#![expect(clippy::struct_excessive_bools)]
#![expect(clippy::inline_always)]

mod buffer;
mod error;
mod escape_buffer;
mod event_builder;
mod literal_buffer;
mod numbers;
mod options;
mod parse_event;
mod path;
mod scanner;

use alloc::{
    format,
    string::{String, ToString},
};
use core::mem::{ManuallyDrop, MaybeUninit};

use buffer::Buffer;
pub use error::{ErrorSource, ParserError, SyntaxError};
use escape_buffer::UnicodeEscapeBuffer;
pub use event_builder::EventBuilder;
use literal_buffer::ExpectedLiteralBuffer;
pub use options::ParserOptions;
pub use parse_event::ParseEvent;
pub use path::{Path, PathItem, PathItemFrom, PathLike};

use crate::backend::{EventCtx, PathCtx, PathKind, RustContext};
use crate::parser::scanner::Scanner;

// ------------------------------------------------------------------------------------------------
// Lexer - internal tokens & states
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) enum Token {
    Eof,
    PropertyName {
        value: String,
    },
    String {
        fragment: String,
    },
    Boolean(bool),
    Null,
    Number(String),
    /// Must be one of: `{` `}` `[` `]` `:` `,`
    Punctuator(u8),
}

impl Token {
    /// Returns `true` if the token value is [`Eof`].
    ///
    /// [`Eof`]: TokenValue::Eof
    #[must_use]
    fn is_eof(&self) -> bool {
        matches!(self, Self::Eof)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Represents a peeked character from the input buffer.
enum PeekedChar {
    /// None if the buffer is empty
    Empty,
    /// Some character
    Char(char),
    /// End of input, the input stream is closed.
    EndOfInput,
}

use PeekedChar::*;

/// ------------------------------------------------------------------------------------------------
/// State machines (1‑for‑1 with TS enums)
/// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParseState {
    Start,
    BeforePropertyName,
    AfterPropertyName,
    BeforePropertyValue,
    BeforeArrayValue,
    AfterPropertyValue,
    AfterArrayValue,
    End,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LexState {
    Default,
    Value,
    ValueLiteral,
    Sign,
    Zero,
    DecimalInteger,
    DecimalPoint,
    DecimalFraction,
    DecimalExponent,
    DecimalExponentSign,
    DecimalExponentInteger,
    String,
    Start,
    StringEscape,
    StringEscapeUnicode,
    BeforePropertyName,
    AfterPropertyName,
    BeforePropertyValue,
    BeforeArrayValue,
    AfterPropertyValue,
    AfterArrayValue,
    End,
    Error,
}

impl From<ParseState> for LexState {
    fn from(state: ParseState) -> Self {
        match state {
            ParseState::Start => LexState::Start,
            ParseState::BeforePropertyName => LexState::BeforePropertyName,
            ParseState::AfterPropertyName => LexState::AfterPropertyName,
            ParseState::BeforePropertyValue => LexState::BeforePropertyValue,
            ParseState::BeforeArrayValue => LexState::BeforeArrayValue,
            ParseState::AfterPropertyValue => LexState::AfterPropertyValue,
            ParseState::AfterArrayValue => LexState::AfterArrayValue,
            ParseState::End => LexState::End,
            ParseState::Error => LexState::Error,
        }
    }
}

/// The streaming JSON parser. Uses the default `Value` type and path
/// representation.
type DefaultStreamingParser = StreamingParserImpl<RustContext>;

///
/// `StreamingParser` can be fed partial or complete JSON input in chunks.
/// It implements `Iterator` to yield `ParseEvent`s representing JSON tokens
/// and structural events.
pub struct StreamingParserImpl<B: PathCtx + EventCtx> {
    // Raw source buffer (always grows then gets truncated after each “round”).
    source: Buffer,
    // Carryover for Scanner (ring + positions + scratch), used in parallel wiring.
    tape: scanner::Tape,
    end_of_input: bool,

    /// Current *global* character position.
    pos: usize,
    line: usize,
    column: usize,

    /// Current parse / lex states
    parse_state: ParseState,
    lex_state: LexState,

    /// Lexer helpers
    buffer: String, // reused for numbers / literals / strings
    unicode_escape_buffer: UnicodeEscapeBuffer,
    expected_literal: ExpectedLiteralBuffer,
    partial_lex: bool,

    path: MaybeUninit<B::Frozen>,
    /// Indicates if a we've started parsing a string value and have not yet
    /// emitted a parse event. Determines the value of `is_initial` on
    /// [`ParseEvent::String`].
    initialized_string: bool,
    /// Indicates if a key is pending, i.e.: we have opened an object but have
    /// not pushed a key yet.
    pending_key: bool,

    /// Options

    /// Allow multiple JSON values in a single input (support transition from
    /// end state to a new value start state)
    multiple_values: bool,

    /// Panic on syntax errors instead of returning them
    #[cfg(test)]
    panic_on_error: bool,

    /// Sequence of tokens produced by the lexer.
    #[cfg(test)]
    lexed_tokens: alloc::vec::Vec<Token>,
}

pub struct StreamingParserIteratorWith<'p, 'src, B: PathCtx + EventCtx> {
    parser: &'p mut StreamingParserImpl<B>,
    path: ManuallyDrop<B::Thawed>,
    pub(crate) factory: B,
    _marker: core::marker::PhantomData<&'src ()>,
    // Scanner for this iteration (not yet used by parser logic; finalized on drop).
    scanner: Scanner<'src>,
}

impl<'p, 'src, B: PathCtx + EventCtx> Drop for StreamingParserIteratorWith<'p, 'src, B> {
    fn drop(&mut self) {
        // SAFETY: ManuallyDrop::take moves out without running Drop,
        // so the later field-drop won’t double-drop it.
        let thawed = unsafe { ManuallyDrop::take(&mut self.path) };
        self.parser.path = MaybeUninit::new(self.factory.freeze(thawed));
        // Finalize scanner and write back carryover state for next feed
        let tape = core::mem::take(&mut self.scanner).finish();
        self.parser.tape = tape;
    }
}

impl<'src, B: PathCtx + EventCtx> Iterator for StreamingParserIteratorWith<'_, 'src, B> {
    type Item = Result<ParseEvent<'src, B>, ParserError<B>>;

    fn next(&mut self) -> Option<Self::Item> {
        self.parser
            .next_event_with(&mut self.factory, &mut self.path, &mut self.scanner)
    }
}

/// A `StreamingParser` that has been closed to further input.
///
/// Returned by [`StreamingParser::finish`], this parser will process any
/// remaining input and then end. It implements `Iterator` to yield
/// `ParseEvent` results.
pub struct ClosedStreamingParser<'src, B: PathCtx + EventCtx> {
    parser: StreamingParserImpl<B>,
    path: ManuallyDrop<B::Thawed>,
    pub(crate) factory: B,
    _marker: core::marker::PhantomData<&'src ()>,
    // Scanner for closed iteration (finalizes to tape on drop)
    scanner: Scanner<'src>,
}

impl<'src, B: PathCtx + EventCtx> Drop for ClosedStreamingParser<'src, B> {
    fn drop(&mut self) {
        // SAFETY: ManuallyDrop::take moves out without running Drop,
        // so the later field-drop won’t double-drop it.
        let thawed = unsafe { ManuallyDrop::take(&mut self.path) };
        self.parser.path = MaybeUninit::new(self.factory.freeze(thawed));
        let tape = core::mem::take(&mut self.scanner).finish();
        self.parser.tape = tape;
    }
}

impl<'src, B: PathCtx + EventCtx> ClosedStreamingParser<'src, B> {
    #[cfg(test)]
    pub(crate) fn get_lexed_tokens(&self) -> &[Token] {
        self.parser.get_lexed_tokens()
    }
}

impl<'src, B: PathCtx + EventCtx> Iterator for ClosedStreamingParser<'src, B> {
    type Item = Result<ParseEvent<'src, B>, ParserError<B>>;

    fn next(&mut self) -> Option<Self::Item> {
        self.parser
            .next_event_with(&mut self.factory, &mut self.path, &mut self.scanner)
    }
}

impl<B: PathCtx + EventCtx> StreamingParserImpl<B> {
    #[must_use]
    /// Creates a new `StreamingParser` with the given event factory and
    /// options.
    pub fn new_with_factory(f: &mut B, options: ParserOptions) -> StreamingParserImpl<B> {
        Self {
            source: Buffer::new(),
            tape: scanner::Tape::default(),
            end_of_input: false,
            partial_lex: false,

            pos: 0,
            line: 1,
            column: 1,

            lex_state: LexState::Default,
            parse_state: ParseState::Start,

            buffer: String::new(),
            unicode_escape_buffer: UnicodeEscapeBuffer::new(),
            expected_literal: ExpectedLiteralBuffer::none(),

            path: MaybeUninit::new(f.frozen_new()),
            initialized_string: false,
            pending_key: false,

            multiple_values: options.allow_multiple_json_values,
            #[cfg(test)]
            panic_on_error: options.panic_on_error,
            #[cfg(test)]
            lexed_tokens: alloc::vec::Vec::new(),
        }
    }

    pub(crate) fn feed_str(&mut self, text: &str) {
        self.source.push(text);
    }

    #[doc(hidden)]
    pub fn feed_with<'p, 'src>(
        &'p mut self,
        mut factory: B,
        text: &'src str,
    ) -> StreamingParserIteratorWith<'p, 'src, B> {
        self.feed_str(text);
        let path = unsafe { factory.thaw(core::mem::take(self.path.assume_init_mut())) };
        let path = ManuallyDrop::new(path);
        let scanner = Scanner::from_carryover(core::mem::take(&mut self.tape), text);
        StreamingParserIteratorWith {
            parser: self,
            factory,
            path,
            _marker: core::marker::PhantomData,
            scanner,
        }
    }

    pub(crate) fn close(&mut self) {
        self.end_of_input = true;
    }

    #[must_use]
    /// Marks the end of input and returns a closed parser to consume pending
    /// events.
    ///
    /// After calling `finish_with`, no further input can be fed. The returned
    /// `ClosedStreamingParser` implements `Iterator` yielding `ParseEvent`s
    /// and then ends.
    pub fn finish_with<'src>(mut self, mut context: B) -> ClosedStreamingParser<'src, B> {
        self.close();
        let path = unsafe { context.thaw(core::mem::take(self.path.assume_init_mut())) };
        let path = ManuallyDrop::new(path);
        let scanner = Scanner::from_carryover(core::mem::take(&mut self.tape), "");
        ClosedStreamingParser {
            parser: self,
            factory: context,
            path,
            _marker: core::marker::PhantomData,
            scanner,
        }
    }

    /// Drive the parser until we either
    ///   * produce one `ParseEvent`, or
    ///   * reach "need more data / end‑of‑input"
    ///   * encounter a syntax error
    ///
    /// Returns:
    /// * `Some(Ok(event))`      – one event ready
    /// * `Some(Err(err))`       - the parser has errored, and no more events
    ///   can be produced
    /// * `None`                 – the parser has no events.
    pub(crate) fn next_event_with<'a, 'cx: 'a, 'src: 'cx>(
        &mut self,
        f: &'cx mut B,
        path: &mut B::Thawed,
        _scanner: &mut Scanner<'src>,
    ) -> Option<Result<ParseEvent<'src, B>, ParserError<B>>> {
        match self.next_event_internal(f, path, _scanner) {
            None => None,
            Some(Ok(event)) => Some(Ok(event)),
            Some(Err(err)) => {
                // #[cfg(test)]
                // assert!(                //     !self.panic_on_error,
                //     "Syntax error at {}:{}: {err}",
                //     self.line, self.column
                // );
                self.parse_state = ParseState::Error;
                self.lex_state = LexState::Error;
                Some(Err(err))
            }
        }
    }

    fn next_event_internal<'a, 'cx: 'a, 'src: 'cx>(
        &'a mut self,
        f: &'cx mut B,
        path: &mut B::Thawed,
        _scanner: &mut Scanner<'src>,
    ) -> Option<Result<ParseEvent<'src, B>, ParserError<B>>> {
        if self.parse_state == ParseState::Error {
            return None;
        }

        loop {
            if self.multiple_values && matches!(self.parse_state, ParseState::End) {
                // No internal builder; adapters build values externally.
                self.lex_state = LexState::Default;
                self.parse_state = ParseState::Start;
                self.path = MaybeUninit::new(f.frozen_new());
            }

            let token = match self.lex() {
                Ok(tok) => tok,
                Err(err) => {
                    #[cfg(test)]
                    assert!(
                        !self.panic_on_error,
                        "Syntax error at {}:{}: {err}",
                        self.line, self.column
                    );
                    return Some(Err(err));
                }
            };
            let is_eof = token.is_eof();
            match self.dispatch_parse_state(token, f, path) {
                Ok(Some(evt)) => {
                    return Some(Ok(evt));
                }
                Ok(None) => {}
                Err(err) => {
                    #[cfg(test)]
                    assert!(
                        !self.panic_on_error,
                        "Syntax error at {}:{}: {err}",
                        self.line, self.column
                    );
                    return Some(Err(err));
                }
            }

            if is_eof || self.partial_lex {
                break;
            }
        }

        None
    }

    // ------------------------------------------------------------------------------------------------
    // Lexer
    // ------------------------------------------------------------------------------------------------

    #[inline(always)]
    fn lex(&mut self) -> Result<Token, ParserError<B>> {
        if !self.partial_lex {
            self.lex_state = LexState::Default;
        }

        loop {
            let next_char = self.peek_char();
            if let Some(tok) = self.lex_state_step(self.lex_state, next_char)? {
                #[cfg(test)]
                self.lexed_tokens.push(tok.clone());
                return Ok(tok);
            }
        }
    }

    /// Convenience – TS uses `undefined | eof` sentinel.  We return `None` for
    /// buffer depleted, `Some(EOI)` for forced end‑of‑input, else
    /// `Some(ch)`.
    #[inline(always)]
    fn peek_char(&mut self) -> PeekedChar {
        if let Some(ch) = self.source.peek() {
            return Char(ch);
        }

        if self.end_of_input {
            return EndOfInput;
        }

        Empty
    }

    fn read_and_invalid_char(&mut self, c: PeekedChar) -> ParserError<B> {
        self.invalid_char(c)
    }

    #[inline(always)]
    fn advance_char(&mut self) {
        if let Some(ch) = self.source.next() {
            if ch == '\n' {
                self.line += 1;
                self.column = 1;
            } else {
                self.column += 1;
            }
            self.pos += 1;
        }
    }

    #[inline(always)]
    fn new_token(&mut self, value: Token, partial: bool) -> Token {
        self.partial_lex = partial;
        value
    }

    #[inline(always)]
    fn produce_string(&mut self, partial: bool) -> Token {
        use Token::{Eof, PropertyName, String};

        #[cfg(test)]
        std::eprintln!(
            "produce_string: partial = {}, buffer = {:?}",
            partial,
            self.buffer
        );

        self.partial_lex = partial;

        if self.parse_state == ParseState::BeforePropertyName {
            if partial {
                return Eof;
            }

            let value = core::mem::take(&mut self.buffer);
            return PropertyName { value };
        }

        let fragment = core::mem::take(&mut self.buffer);
        String { fragment }
    }

    #[expect(clippy::too_many_lines)]
    #[inline(always)]
    fn lex_state_step(
        &mut self,
        lex_state: LexState,
        next_char: PeekedChar,
    ) -> Result<Option<Token>, ParserError<B>> {
        use LexState::*;
        match lex_state {
            Error => Ok(None),
            Default => {
                match next_char {
                    Char(
                        '\t' | '\u{0B}' | '\u{0C}' | ' ' | '\u{00A0}' | '\u{FEFF}' | '\n' | '\r'
                        | '\u{2028}' | '\u{2029}',
                    ) => {
                        // Skip whitespace
                        self.advance_char();
                        Ok(None)
                    }
                    Char(c) if c.is_whitespace() => {
                        self.advance_char();
                        Ok(None)
                    }
                    Empty => Ok(Some(self.new_token(Token::Eof, true))),
                    EndOfInput => {
                        self.advance_char();
                        Ok(Some(self.new_token(Token::Eof, false)))
                    }

                    Char(_) => self.lex_state_step(self.parse_state.into(), next_char),
                }
            }

            // -------------------------- VALUE entry --------------------------
            Value => match next_char {
                Char(c) if matches!(c, '{' | '[') => {
                    self.advance_char();
                    Ok(Some(self.new_token(Token::Punctuator(c as u8), false)))
                }
                Char(c) if matches!(c, 'n' | 't' | 'f') => {
                    self.buffer.clear();
                    self.advance_char();
                    self.buffer.push(c);
                    self.lex_state = ValueLiteral;
                    self.expected_literal = ExpectedLiteralBuffer::new(c);
                    Ok(None)
                }
                Char(c @ '-') => {
                    self.buffer.clear();
                    self.advance_char();
                    self.buffer.push(c);
                    self.lex_state = Sign;
                    Ok(None)
                }
                Char(c @ '0') => {
                    self.buffer.clear();
                    self.advance_char();
                    self.buffer.push(c);
                    self.lex_state = Zero;
                    Ok(None)
                }
                Char(c) if c.is_ascii_digit() => {
                    self.buffer.clear();
                    self.advance_char();
                    self.buffer.push(c);
                    self.lex_state = DecimalInteger;
                    Ok(None)
                }
                Char('"') => {
                    self.advance_char(); // consume quote
                    self.buffer.clear();
                    self.lex_state = LexState::String;
                    self.initialized_string = true;
                    Ok(None)
                }
                c => Err(self.invalid_char(c)),
            },

            // -------------------------- LITERALS -----------------------------
            ValueLiteral => match next_char {
                Empty => Ok(Some(self.new_token(Token::Eof, true))),
                Char(c) => match self.expected_literal.step(c) {
                    literal_buffer::Step::NeedMore => {
                        self.advance_char();
                        self.buffer.push(c);
                        Ok(None)
                    }
                    literal_buffer::Step::Done(tok) => {
                        self.advance_char();
                        self.buffer.push(c);
                        Ok(Some(self.new_token(tok, false)))
                    }
                    literal_buffer::Step::Reject => Err(self.read_and_invalid_char(Char(c))),
                },
                c @ EndOfInput => Err(self.read_and_invalid_char(c)),
            },

            // -------------------------- NUMBERS -----------------------------
            Sign => match next_char {
                Empty => Ok(Some(self.new_token(Token::Eof, true))),
                Char(c @ '0') => {
                    self.advance_char();
                    self.buffer.push(c);
                    self.lex_state = Zero;
                    Ok(None)
                }
                Char(c) if c.is_ascii_digit() => {
                    self.advance_char();
                    self.buffer.push(c);
                    self.lex_state = DecimalInteger;
                    Ok(None)
                }
                c => Err(self.read_and_invalid_char(c)),
            },

            Zero => match next_char {
                Empty => Ok(Some(self.new_token(Token::Eof, true))),
                Char(c @ '.') => {
                    self.advance_char();
                    self.buffer.push(c);
                    self.lex_state = DecimalPoint;
                    Ok(None)
                }
                Char(c) if matches!(c, 'e' | 'E') => {
                    self.advance_char();
                    self.buffer.push(c);
                    self.lex_state = DecimalExponent;
                    Ok(None)
                }
                _ => {
                    let value = core::mem::take(&mut self.buffer);
                    Ok(Some(self.new_token(Token::Number(value), false)))
                }
            },

            DecimalInteger => match next_char {
                Empty => Ok(Some(self.new_token(Token::Eof, true))),
                Char(c @ '.') => {
                    self.advance_char();
                    self.buffer.push(c);
                    self.lex_state = DecimalPoint;
                    Ok(None)
                }
                Char(c) if matches!(c, 'e' | 'E') => {
                    self.advance_char();
                    self.buffer.push(c);
                    self.lex_state = DecimalExponent;
                    Ok(None)
                }
                Char(c) if c.is_ascii_digit() => {
                    self.advance_char();
                    self.buffer.push(c);

                    let copied = self
                        .source
                        .copy_while(&mut self.buffer, |d| d.is_ascii_digit());

                    self.column += copied;
                    self.pos += copied;

                    Ok(None)
                }
                _ => {
                    let value = core::mem::take(&mut self.buffer);
                    Ok(Some(self.new_token(Token::Number(value), false)))
                }
            },

            DecimalPoint => match next_char {
                Empty => Ok(Some(self.new_token(Token::Eof, true))),
                Char(c) if matches!(c, 'e' | 'E') => {
                    self.advance_char();
                    self.buffer.push(c);
                    self.lex_state = DecimalExponent;
                    Ok(None)
                }
                Char(c) if c.is_ascii_digit() => {
                    self.advance_char();
                    self.buffer.push(c);
                    self.lex_state = DecimalFraction;

                    let copied = self
                        .source
                        .copy_while(&mut self.buffer, |d| d.is_ascii_digit());

                    self.column += copied;
                    self.pos += copied;

                    Ok(None)
                }
                c => Err(self.read_and_invalid_char(c)),
            },

            DecimalFraction => match next_char {
                Empty => Ok(Some(self.new_token(Token::Eof, true))),
                Char(c) if matches!(c, 'e' | 'E') => {
                    self.advance_char();
                    self.buffer.push(c);
                    self.lex_state = DecimalExponent;
                    Ok(None)
                }
                Char(c) if c.is_ascii_digit() => {
                    self.advance_char();
                    self.buffer.push(c);

                    let copied = self
                        .source
                        .copy_while(&mut self.buffer, |d| d.is_ascii_digit());

                    self.column += copied;
                    self.pos += copied;

                    Ok(None)
                }
                _ => {
                    let value = core::mem::take(&mut self.buffer);
                    Ok(Some(self.new_token(Token::Number(value), false)))
                }
            },

            DecimalExponent => match next_char {
                Empty => Ok(Some(self.new_token(Token::Eof, true))),
                Char(c) if matches!(c, '+' | '-') => {
                    self.advance_char();
                    self.buffer.push(c);
                    self.lex_state = DecimalExponentSign;
                    Ok(None)
                }
                Char(c) if c.is_ascii_digit() => {
                    self.advance_char();
                    self.buffer.push(c);
                    self.lex_state = DecimalExponentInteger;

                    let copied = self
                        .source
                        .copy_while(&mut self.buffer, |d| d.is_ascii_digit());

                    self.column += copied;
                    self.pos += copied;

                    Ok(None)
                }
                c => Err(self.read_and_invalid_char(c)),
            },

            DecimalExponentSign => match next_char {
                Empty => Ok(Some(self.new_token(Token::Eof, true))),
                Char(c) if c.is_ascii_digit() => {
                    self.advance_char();
                    self.buffer.push(c);
                    self.lex_state = DecimalExponentInteger;

                    let copied = self
                        .source
                        .copy_while(&mut self.buffer, |d| d.is_ascii_digit());

                    self.column += copied;
                    self.pos += copied;

                    Ok(None)
                }
                c => Err(self.read_and_invalid_char(c)),
            },

            DecimalExponentInteger => match next_char {
                Empty => Ok(Some(self.new_token(Token::Eof, true))),
                Char(c) if c.is_ascii_digit() => {
                    self.advance_char();
                    self.buffer.push(c);

                    let copied = self
                        .source
                        .copy_while(&mut self.buffer, |d| d.is_ascii_digit());

                    self.column += copied;
                    self.pos += copied;

                    Ok(None)
                }
                _ => {
                    let value = core::mem::take(&mut self.buffer);
                    Ok(Some(self.new_token(Token::Number(value), false)))
                }
            },

            // -------------------------- STRING -----------------------------
            LexState::String => match next_char {
                // escape sequence
                Char('\\') => {
                    self.advance_char();
                    self.lex_state = LexState::StringEscape;
                    Ok(None)
                }
                // closing quote -> complete string
                Char('"') => {
                    self.advance_char();
                    Ok(Some(self.produce_string(false)))
                }
                Char(c @ '\0'..='\x1F') => {
                    // JSON spec allows 0x20 .. 0x10FFFF unescaped.
                    Err(self.read_and_invalid_char(Char(c)))
                }
                Empty => Ok(Some(self.produce_string(true))),
                Char(_c) => {
                    // Fast-path: copy as many consecutive non-escaped, non-terminating
                    // characters as possible in a single pass.
                    let copied = self.source.copy_while(&mut self.buffer, |ch| {
                        ch != '\\' && ch != '"' && ch >= '\u{20}'
                    });

                    // Update lexer coordinates – the copied characters cannot contain
                    // a newline (0x0A) as it is < 0x20 and thus rejected by the
                    // predicate above, so we only need to move the column/pos counters.
                    self.column += copied;
                    self.pos += copied;

                    Ok(None)
                }
                EndOfInput => Err(self.read_and_invalid_char(EndOfInput)),
            },

            StringEscape => match next_char {
                Empty => Ok(Some(self.produce_string(true))),
                Char(ch) if matches!(ch, '"' | '\\' | '/') => {
                    self.advance_char();
                    self.buffer.push(ch);
                    self.lex_state = LexState::String;
                    Ok(None)
                }
                Char('b') => {
                    self.advance_char();
                    self.buffer.push('\u{0008}');
                    self.lex_state = LexState::String;
                    Ok(None)
                }
                Char('f') => {
                    self.advance_char();
                    self.buffer.push('\u{000C}');
                    self.lex_state = LexState::String;
                    Ok(None)
                }
                Char('n') => {
                    self.advance_char();
                    self.buffer.push('\n');
                    self.lex_state = LexState::String;
                    Ok(None)
                }
                Char('r') => {
                    self.advance_char();
                    self.buffer.push('\r');
                    self.lex_state = LexState::String;
                    Ok(None)
                }
                Char('t') => {
                    self.advance_char();
                    self.buffer.push('\t');
                    self.lex_state = LexState::String;
                    Ok(None)
                }
                Char('u') => {
                    self.advance_char();
                    self.unicode_escape_buffer.reset();
                    self.lex_state = LexState::StringEscapeUnicode;
                    Ok(None)
                }
                c => Err(self.read_and_invalid_char(c)),
            },

            StringEscapeUnicode => {
                match next_char {
                    Empty => Ok(Some(self.produce_string(true))),
                    Char(c) if c.is_ascii_hexdigit() => {
                        self.advance_char();
                        match self.unicode_escape_buffer.feed(c) {
                            Ok(Some(char)) => {
                                self.buffer.push(char);
                                self.lex_state = LexState::String;
                                Ok(None)
                            }
                            Ok(None) => {
                                // Still waiting for more hex digits
                                Ok(None)
                            }
                            Err(err) => Err(self.syntax_error(err)),
                        }
                    }
                    EndOfInput => {
                        // consume EOF sentinel and advance column to match TS behavior
                        self.advance_char();
                        self.column += 1;
                        Err(self.invalid_eof())
                    }
                    c @ Char(_) => Err(self.read_and_invalid_char(c)),
                }
            }

            Start => match next_char {
                Char(c) if matches!(c, '{' | '[') => {
                    self.advance_char();
                    Ok(Some(self.new_token(Token::Punctuator(c as u8), false)))
                }
                _ => {
                    self.lex_state = LexState::Value;
                    Ok(None)
                }
            },

            BeforePropertyName => match next_char {
                Char('}') => {
                    self.advance_char();
                    Ok(Some(self.new_token(Token::Punctuator(b'}'), false)))
                }

                Char('"') => {
                    self.advance_char();
                    self.buffer.clear();
                    self.lex_state = LexState::String;
                    Ok(None)
                }
                c => Err(self.read_and_invalid_char(c)),
            },

            AfterPropertyName => match next_char {
                Char(c @ ':') => {
                    self.advance_char();
                    Ok(Some(self.new_token(Token::Punctuator(c as u8), false)))
                }
                c => Err(self.read_and_invalid_char(c)),
            },

            BeforePropertyValue => {
                self.lex_state = LexState::Value;
                Ok(None)
            }

            AfterPropertyValue => match next_char {
                Char(c) if matches!(c, ',' | '}') => {
                    self.advance_char();
                    Ok(Some(self.new_token(Token::Punctuator(c as u8), false)))
                }
                c => Err(self.read_and_invalid_char(c)),
            },

            BeforeArrayValue => match next_char {
                Char(']') => {
                    self.advance_char();
                    Ok(Some(self.new_token(Token::Punctuator(b']'), false)))
                }
                _ => {
                    self.lex_state = LexState::Value;
                    Ok(None)
                }
            },

            AfterArrayValue => match next_char {
                Char(c) if matches!(c, ',' | ']') => {
                    self.advance_char();
                    Ok(Some(self.new_token(Token::Punctuator(c as u8), false)))
                }
                c => Err(self.read_and_invalid_char(c)),
            },

            End => {
                let c = self.peek_char();
                Err(self.invalid_char(c))
            }
        }
    }

    // ------------------------------------------------------------------------------------------------
    // Parse state dispatcher (translation of TS parseStates method)
    // ------------------------------------------------------------------------------------------------
    #[inline(always)]
    fn dispatch_parse_state<'p, 'cx: 'p, 'src: 'cx>(
        &'p mut self,
        token: Token,
        ctx: &'cx mut B,
        path: &mut B::Thawed,
    ) -> Result<Option<ParseEvent<'src, B>>, ParserError<B>> {
        use ParseState::*;

        match self.parse_state {
            // In single-value mode, EOF at start when end_of_input indicates unexpected end.
            Start => match token {
                Token::Eof if self.end_of_input && !self.multiple_values => Err(self.invalid_eof()),
                Token::Eof => Ok(None),
                _ => self.push(token, ctx, path),
            },

            BeforePropertyName => match token {
                Token::Eof if self.end_of_input => Err(self.invalid_eof()),
                Token::PropertyName { value } => {
                    if !self.pending_key {
                        ctx.pop_kind(path);
                    }
                    ctx.push_key_from_str(path, &value);
                    self.pending_key = false;
                    self.parse_state = AfterPropertyName;
                    Ok(None)
                }
                Token::Punctuator(_) => Ok(self.pop(ctx, path)),
                _ => Ok(None),
            },

            AfterPropertyName => match token {
                Token::Eof if self.end_of_input => Err(self.invalid_eof()),
                Token::Eof => Ok(None),
                _ => {
                    self.parse_state = BeforePropertyValue;

                    Ok(None)
                }
            },

            BeforePropertyValue => match token {
                Token::Eof => Ok(None),
                _ => self.push(token, ctx, path),
            },

            BeforeArrayValue => match token {
                Token::Eof => Ok(None),
                Token::Punctuator(b']') => Ok(self.pop(ctx, path)),
                _ => self.push(token, ctx, path),
            },

            AfterPropertyValue => match token {
                Token::Eof if self.end_of_input => Err(self.invalid_eof()),
                Token::Punctuator(b',') => {
                    self.parse_state = BeforePropertyName;
                    Ok(None)
                }
                Token::Punctuator(b'}') => Ok(self.pop(ctx, path)),
                _ => Ok(None),
            },

            AfterArrayValue => match token {
                Token::Eof if self.end_of_input => Err(self.invalid_eof()),
                Token::Punctuator(b',') => {
                    match ctx.bump_last_index(path) {
                        Ok(path) => path,
                        Err(_) => {
                            unreachable!(); // TODO
                        }
                    }

                    self.parse_state = BeforeArrayValue;
                    Ok(None)
                }
                Token::Punctuator(b']') => Ok(self.pop(ctx, path)),
                _ => Ok(None),
            },
            End | Error => Ok(None),
        }
    }

    #[inline(always)]
    fn pop<'a, 'cx: 'a, 'src: 'cx>(
        &'a mut self,
        f: &'cx mut B,
        path: &mut B::Thawed,
    ) -> Option<ParseEvent<'src, B>> {
        // #[cfg(test)]
        // std::eprintln!(
        //     "pop: pending_key = {}, path = {:?}",
        //     self.pending_key,
        //     path
        // );

        let evt = if self.pending_key {
            Some(ParseEvent::ObjectEnd { path: path.clone() })
        } else {
            match f.pop_kind(path) {
                Some(PathKind::Index) => Some(ParseEvent::ArrayEnd { path: path.clone() }),
                Some(PathKind::Key) => Some(ParseEvent::ObjectEnd { path: path.clone() }),
                None => unreachable!(),
            }
        };

        // We actually need to peek at the new last frame and restore the parse state
        // now:
        if let Some(last_frame) = f.last_kind(path) {
            self.parse_state = match last_frame {
                PathKind::Index => ParseState::AfterArrayValue,
                PathKind::Key => ParseState::AfterPropertyValue,
            };
        } else {
            self.parse_state = ParseState::End;
        }

        evt
    }

    #[inline(always)]
    fn push<'a, 'cx: 'a, 'src: 'cx>(
        &'a mut self,
        token: Token,
        f: &'cx mut B,
        path: &mut B::Thawed,
    ) -> Result<Option<ParseEvent<'src, B>>, ParserError<B>> {
        let evt: Option<ParseEvent<'_, B>> = match token {
            Token::Punctuator(b'{') => {
                self.pending_key = true;
                self.parse_state = ParseState::BeforePropertyName;
                return Ok(Some(ParseEvent::ObjectBegin { path: path.clone() }));
            }
            Token::Punctuator(b'[') => {
                let output_path = path.clone();
                f.push_index_zero(path);
                self.parse_state = ParseState::BeforeArrayValue;
                return Ok(Some(ParseEvent::ArrayBegin { path: output_path }));
            }

            Token::Null => Some(ParseEvent::Null { path: path.clone() }),
            Token::Boolean(b) => {
                let value = f.new_bool(b).map_err(|e| self.event_context_error(e))?;
                Some(ParseEvent::Boolean {
                    path: path.clone(),
                    value,
                })
            }
            Token::Number(n) => {
                let value = f
                    .new_number_owned(n)
                    .map_err(|e| self.event_context_error(e))?;
                Some(ParseEvent::Number {
                    path: path.clone(),
                    value,
                })
            }
            Token::String { fragment } => {
                let fragment = f
                    .new_str_owned(fragment)
                    .map_err(|e| self.event_context_error(e))?;
                let is_initial = self.initialized_string;
                let is_final = !self.partial_lex;
                self.initialized_string = false;
                Some(ParseEvent::String {
                    path: path.clone(),
                    fragment,
                    is_initial,
                    is_final,
                })
            }
            Token::PropertyName { .. } => {
                unreachable!();
                // return Err(
                //     self.syntax_error("Unexpected property name outside of
                // object".to_string()) );
            }
            _ => None,
        };

        // 3. Adjust parse state exactly once, using `parent_kind`
        if !self.partial_lex {
            self.parse_state = match f.last_kind(path) {
                None => ParseState::End,
                Some(PathKind::Index) => ParseState::AfterArrayValue,
                Some(PathKind::Key) => ParseState::AfterPropertyValue,
            };
        }

        Ok(evt)
    }

    // ------------------------------------------------------------------------------------------------
    // Errors
    // ------------------------------------------------------------------------------------------------
    fn invalid_char(&self, c: PeekedChar) -> ParserError<B> {
        match c {
            EndOfInput | Empty => self.syntax_error(SyntaxError::UnexpectedEndOfInput),
            Char(c) => self.syntax_error(SyntaxError::InvalidCharacter(c)),
        }
    }

    fn invalid_eof(&self) -> ParserError<B> {
        self.syntax_error(SyntaxError::UnexpectedEndOfInput)
    }

    fn event_context_error(&self, err: B::Error) -> ParserError<B> {
        self.parser_error(ErrorSource::EventContextError(err))
    }

    fn syntax_error(&self, err: SyntaxError) -> ParserError<B> {
        self.parser_error(ErrorSource::SyntaxError(err))
    }

    fn parser_error(&self, err: ErrorSource<B>) -> ParserError<B> {
        let err = ParserError {
            source: err,
            line: self.line,
            column: self.column,
        };
        #[cfg(test)]
        assert!(!self.panic_on_error, "{err}");
        err
    }

    fn format_char(c: char) -> String {
        match c {
            '"' => "\\\"".into(),
            '\'' => "\\'".into(),
            '\\' => "\\\\".into(),
            '\u{0008}' /* \b */=> "\\b".into(),
            '\u{000C}' /* \f */ => "\\f".into(),
            '\n' => "\\n".into(),
            '\r' => "\\r".into(),
            '\t' => "\\t".into(),
            '\u{0000b}' /* \v */ => "\\v".into(),
            '\0' => "\\0".into(),
            '\u{2028}' => "\\u{2028}".into(),
            '\u{2029}' => "\\u{2029}".into(),
            c if c.is_control() => {
              format!("\\u{:04X}", c as u32)
            }
            c if c.is_whitespace() && !c.is_ascii_whitespace() => {
                format!("\\u{:04X}", c as u32)
            }
            c => c.to_string(),
        }
    }

    #[cfg(test)]
    pub(crate) fn get_lexed_tokens(&self) -> &[Token] {
        &self.lexed_tokens
    }
}

impl StreamingParserImpl<RustContext> {
    pub fn new(options: ParserOptions) -> Self {
        Self::new_with_factory(&mut RustContext, options)
    }

    /// Feeds a chunk of JSON text into the parser.
    ///
    /// The parser buffers the input and parses it incrementally,
    /// yielding events when complete JSON tokens or structures are
    /// recognized.
    pub fn feed<'p, 'src>(
        &'p mut self,
        text: &'src str,
    ) -> StreamingParserIteratorWith<'p, 'src, RustContext> {
        self.feed_with(RustContext, text)
    }

    #[must_use]
    pub fn finish(self) -> ClosedStreamingParser<'static, RustContext> {
        self.finish_with(RustContext)
    }
}

#[cfg(test)]
mod tests;
