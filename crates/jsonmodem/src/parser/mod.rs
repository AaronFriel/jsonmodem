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
    sync::Arc,
    vec::Vec,
};
use core::mem::{ManuallyDrop, MaybeUninit};

// buffer is no longer used directly by the parser core; Scanner owns input state.
pub use error::{ErrorSource, ParserError, SyntaxError};
use escape_buffer::UnicodeEscapeBuffer;
pub use event_builder::EventBuilder;
use literal_buffer::ExpectedLiteralBuffer;
pub use options::ParserOptions;
use options::DecodeMode;
pub use parse_event::ParseEvent;
pub use path::{Path, PathItem, PathItemFrom, PathLike};

use crate::{
    backend::{EventCtx, PathCtx, PathKind, RustContext},
    parser::scanner::{Scanner, ScannerState},
};

// ------------------------------------------------------------------------------------------------
// Lexer - internal tokens & states
// ------------------------------------------------------------------------------------------------

#[derive(Debug)]
pub(crate) enum Token<'src> {
    Eof,
    PropertyName(String),
    PropertyNameRaw(Vec<u8>),
    StringBorrowed(&'src str),
    StringOwned(String),
    StringRaw(Vec<u8>),
    Boolean(bool),
    Null,
    NumberBorrowed(&'src str),
    Number(String),
    /// Must be one of: `{` `}` `[` `]` `:` `,`
    Punctuator(u8),
}

impl Token<'_> {
    fn to_owned(&self) -> Token<'static> {
        match self {
            Token::Eof => Token::Eof,
            Token::PropertyName(name) => Token::PropertyName(name.clone()),
            Token::PropertyNameRaw(bytes) => Token::PropertyNameRaw(bytes.clone()),
            Token::StringBorrowed(s) => Token::StringOwned((*s).into()),
            Token::StringOwned(s) => Token::StringOwned(s.clone()),
            Token::StringRaw(bytes) => Token::StringRaw(bytes.clone()),
            Token::Boolean(b) => Token::Boolean(*b),
            Token::Null => Token::Null,
            Token::NumberBorrowed(s) => Token::Number((*s).into()),
            Token::Number(s) => Token::Number(s.clone()),
            Token::Punctuator(c) => Token::Punctuator(*c),
        }
    }
}

impl Token<'_> {
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
    end_of_input: bool,

    /// Current *global* character position.
    pos: usize,
    line: usize,
    column: usize,

    /// Current parse / lex states
    scanner_state: ScannerState,
    parse_state: ParseState,
    lex_state: LexState,

    /// Lexer helpers
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
    allow_unicode_whitespace: bool,

    /// Allow multiple JSON values in a single input (support transition from
    /// end state to a new value start state)
    multiple_values: bool,

    /// Unicode escape decoding behavior
    decode_mode: DecodeMode,

    /// Panic on syntax errors instead of returning them
    #[cfg(test)]
    panic_on_error: bool,

    /// Sequence of tokens produced by the lexer.
    #[cfg(test)]
    lexed_tokens: Vec<Token<'static>>,

    /// Tracks a pending high surrogate (0xD800..=0xDBFF) seen via \u escapes
    /// awaiting a following low surrogate to form a single code point.
    pending_high_surrogate: Option<u16>,
    /// Compatibility knob: accept uppercase 'U' for Unicode escapes
    /// (e.g., "\\UD83D\\UDE00").
    allow_uppercase_u: bool,
}

pub struct StreamingParserIteratorWith<'p, 'src, B: PathCtx + EventCtx> {
    parser: &'p mut StreamingParserImpl<B>,
    path: ManuallyDrop<B::Thawed>,
    pub(crate) factory: B,
    scanner: Scanner<'src>,
}

impl<'p, 'src, B: PathCtx + EventCtx> Drop for StreamingParserIteratorWith<'p, 'src, B> {
    fn drop(&mut self) {
        // SAFETY: ManuallyDrop::take moves out without running Drop,
        // so the later field-drop won’t double-drop it.
        let thawed = unsafe { ManuallyDrop::take(&mut self.path) };
        self.parser.path = MaybeUninit::new(self.factory.freeze(thawed));

        // Persist scanner carryover (unread tail + token scratch + positions)
        let carry = core::mem::take(&mut self.scanner).finish();
        self.parser.scanner_state = carry;
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
    scanner: Scanner<'src>,
}

impl<'src, B: PathCtx + EventCtx> Drop for ClosedStreamingParser<'src, B> {
    fn drop(&mut self) {
        // SAFETY: ManuallyDrop::take moves out without running Drop,
        // so the later field-drop won’t double-drop it.
        let thawed = unsafe { ManuallyDrop::take(&mut self.path) };
        self.parser.path = MaybeUninit::new(self.factory.freeze(thawed));

        // Persist scanner carryover (unread tail + token scratch + positions)
        let carry = core::mem::take(&mut self.scanner).finish();
        self.parser.scanner_state = carry;
    }
}

impl<B: PathCtx + EventCtx> ClosedStreamingParser<'_, B> {
    #[cfg(test)]
    pub(crate) fn get_lexed_tokens(&self) -> &[Token<'static>] {
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
            end_of_input: false,
            partial_lex: false,

            pos: 0,
            line: 1,
            column: 1,

            scanner_state: ScannerState::default(),
            parse_state: ParseState::Start,
            lex_state: LexState::Default,

            unicode_escape_buffer: UnicodeEscapeBuffer::new(),
            expected_literal: ExpectedLiteralBuffer::none(),

            path: MaybeUninit::new(f.frozen_new()),
            initialized_string: false,
            pending_key: false,

            multiple_values: options.allow_multiple_json_values,
            decode_mode: options.decode_mode,
            allow_uppercase_u: options.allow_uppercase_u,
            allow_unicode_whitespace: options.allow_unicode_whitespace,
            #[cfg(test)]
            panic_on_error: options.panic_on_error,
            #[cfg(test)]
            lexed_tokens: Vec::new(),
            pending_high_surrogate: None,
        }
    }

    #[doc(hidden)]
    pub fn feed_with<'p, 'src>(
        &'p mut self,
        mut factory: B,
        text: &'src str,
    ) -> StreamingParserIteratorWith<'p, 'src, B> {
        let path = unsafe { factory.thaw(core::mem::take(self.path.assume_init_mut())) };
        let path = ManuallyDrop::new(path);
        let scanner = Scanner::from_state(core::mem::take(&mut self.scanner_state), text);
        StreamingParserIteratorWith {
            parser: self,
            factory,
            path,
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
        let scanner = Scanner::from_state(core::mem::take(&mut self.scanner_state), "");
        ClosedStreamingParser {
            parser: self,
            factory: context,
            path,
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
        scanner: &mut Scanner<'src>,
    ) -> Option<Result<ParseEvent<'src, B>, ParserError<B>>> {
        match self.next_event_internal(f, path, scanner) {
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
        scanner: &mut Scanner<'src>,
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

            let token = match self.lex(scanner) {
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
    fn lex<'src>(&mut self, scanner: &mut Scanner<'src>) -> Result<Token<'src>, ParserError<B>> {
        if !self.partial_lex {
            self.lex_state = LexState::Default;
        }

        loop {
            if let Some(tok) = self.lex_state_step(self.lex_state, scanner)? {
                #[cfg(test)]
                self.lexed_tokens.push(tok.to_owned());
                return Ok(tok);
            }
        }
    }

    /// Convenience – TS uses `undefined | eof` sentinel.  We return `None` for
    /// buffer depleted, `Some(EOI)` for forced end‑of‑input, else
    /// `Some(ch)`.
    #[inline(always)]
    fn peek_char(&mut self, scanner: &Scanner<'_>) -> PeekedChar {
        if let Some(unit) = scanner.peek() {
            return Char(unit.ch);
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
    fn advance_char(&mut self, scanner: &mut Scanner<'_>, consume: bool) {
        // Deprecated: prefer using peek_guard(). This remains for transitional
        // calls outside refactored branches.
        let adv = if consume { scanner.consume() } else { scanner.skip() };
        if let Some(unit) = adv {
            if unit.ch == '\n' {
                self.line += 1;
                self.column = 1;
            } else {
                self.column += 1;
            }
            self.pos += 1;
        }
    }

    #[inline(always)]
    fn apply_advanced_unit(&mut self, unit: scanner::CharInfo) {
        if unit.ch == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        self.pos += 1;
    }

    #[inline(always)]
    fn new_token<'src>(&mut self, value: Token<'src>, partial: bool) -> Token<'src> {
        self.partial_lex = partial;
        value
    }

    #[inline(always)]
    fn produce_string<'src>(&mut self, partial: bool, scanner: &mut Scanner<'src>) -> Token<'src> {
        use Token::Eof;

        self.partial_lex = partial;

        if self.parse_state == ParseState::BeforePropertyName {
            if partial {
                return Eof;
            }
            return match scanner.emit() {
                scanner::Capture::Borrowed(v) => Token::PropertyName(v.into()),
                scanner::Capture::Owned(v) => Token::PropertyName(v),
                scanner::Capture::Raw(v) => Token::PropertyNameRaw(v),
            };
        }

        match scanner.emit() {
            scanner::Capture::Borrowed(v) => Token::StringBorrowed(v),
            scanner::Capture::Owned(v) => Token::StringOwned(v),
            scanner::Capture::Raw(v) => Token::StringRaw(v),
        }
    }

    #[inline]
    fn push_wtf8_for_u32<'src>(&mut self, scanner: &mut Scanner<'src>, code: u32) {
        let mut buf = [0u8; 4];
        let slice: &[u8] = if code < 0x80 {
            buf[0] = code as u8;
            &buf[..1]
        } else if code < 0x800 {
            buf[0] = 0xC0 | ((code >> 6) as u8);
            buf[1] = 0x80 | ((code & 0x3F) as u8);
            &buf[..2]
        } else if code < 0x10000 {
            buf[0] = 0xE0 | ((code >> 12) as u8);
            buf[1] = 0x80 | (((code >> 6) & 0x3F) as u8);
            buf[2] = 0x80 | ((code & 0x3F) as u8);
            &buf[..3]
        } else {
            buf[0] = 0xF0 | ((code >> 18) as u8);
            buf[1] = 0x80 | (((code >> 12) & 0x3F) as u8);
            buf[2] = 0x80 | (((code >> 6) & 0x3F) as u8);
            buf[3] = 0x80 | ((code & 0x3F) as u8);
            &buf[..4]
        };
        scanner.ensure_raw().extend_from_slice(slice);
    }

    #[expect(clippy::too_many_lines)]
    #[inline(always)]
    fn lex_state_step<'src>(
        &mut self,
        lex_state: LexState,
        scanner: &mut Scanner<'src>,
    ) -> Result<Option<Token<'src>>, ParserError<B>> {
        use LexState::*;
        match lex_state {
            Error => Ok(None),
            Default => {
                if let Some(g) = scanner.peek_guard() {
                    let c = g.ch();
                    if !self.allow_unicode_whitespace && matches!(c, ' ' | '\t' | '\n' | '\r') {
                        // Skip JSON's 4 whitespace code points by default
                        let unit = g.skip();
                        self.apply_advanced_unit(unit);
                        return Ok(None);
                    }
                    if self.allow_unicode_whitespace && (c.is_whitespace() || matches!(c, '\u{FEFF}')) {
                        // When enabled, accept all Unicode whitespace and BOM
                        let unit = g.skip();
                        self.apply_advanced_unit(unit);
                        return Ok(None);
                    }
                    // Delegate to parse-state entry without consuming
                    drop(g);
                    return self.lex_state_step(self.parse_state.into(), scanner);
                }
                if self.end_of_input {
                    return Ok(Some(self.new_token(Token::Eof, false)));
                }
                Ok(Some(self.new_token(Token::Eof, true)))
            }

            // -------------------------- VALUE entry --------------------------
            Value => {
                if let Some(g) = scanner.peek_guard() {
                    let c = g.ch();
                    if matches!(c, '{' | '[') {
                        let unit = g.skip();
                        self.apply_advanced_unit(unit);
                        return Ok(Some(self.new_token(Token::Punctuator(c as u8), false)));
                    }
                    if matches!(c, 'n' | 't' | 'f') {
                        let unit = g.consume();
                        self.apply_advanced_unit(unit);
                        self.lex_state = ValueLiteral;
                        self.expected_literal = ExpectedLiteralBuffer::new(c);
                        return Ok(None);
                    }
                    if c == '-' {
                        let unit = g.consume();
                        self.apply_advanced_unit(unit);
                        self.lex_state = Sign;
                        return Ok(None);
                    }
                    if c == '0' {
                        let unit = g.consume();
                        self.apply_advanced_unit(unit);
                        self.lex_state = Zero;
                        return Ok(None);
                    }
                    if c.is_ascii_digit() {
                        let unit = g.consume();
                        self.apply_advanced_unit(unit);
                        self.lex_state = DecimalInteger;
                        return Ok(None);
                    }
                    if c == '"' {
                        let unit = g.skip();
                        self.apply_advanced_unit(unit);
                        self.lex_state = LexState::String;
                        self.initialized_string = true;
                        return Ok(None);
                    }
                    return Err(self.invalid_char(Char(c)));
                }
                if self.end_of_input {
                    return Err(self.invalid_char(EndOfInput));
                }
                Ok(Some(self.new_token(Token::Eof, true)))
            }

            // -------------------------- LITERALS -----------------------------
            ValueLiteral => {
                if let Some(g) = scanner.peek_guard() {
                    let c = g.ch();
                    match self.expected_literal.step(c) {
                        literal_buffer::Step::NeedMore => {
                            let unit = g.consume();
                            self.apply_advanced_unit(unit);
                            Ok(None)
                        }
                        literal_buffer::Step::Done(tok) => {
                            let unit = g.consume();
                            self.apply_advanced_unit(unit);
                            let _ = scanner.emit();
                            Ok(Some(self.new_token(tok, false)))
                        }
                        literal_buffer::Step::Reject => Err(self.read_and_invalid_char(Char(c))),
                    }
                } else if self.end_of_input {
                    Err(self.read_and_invalid_char(EndOfInput))
                } else {
                    Ok(Some(self.new_token(Token::Eof, true)))
                }
            }

            // -------------------------- NUMBERS -----------------------------
            Sign => {
                if let Some(g) = scanner.peek_guard() {
                    let c = g.ch();
                    if c == '0' {
                        let unit = g.consume();
                        self.apply_advanced_unit(unit);
                        self.lex_state = Zero;
                        return Ok(None);
                    }
                    if c.is_ascii_digit() {
                        let unit = g.consume();
                        self.apply_advanced_unit(unit);
                        self.lex_state = DecimalInteger;
                        return Ok(None);
                    }
                    return Err(self.read_and_invalid_char(Char(c)));
                }
                Ok(Some(self.new_token(Token::Eof, true)))
            }

            Zero => {
                if let Some(g) = scanner.peek_guard() {
                    let c = g.ch();
                    if c == '.' {
                        let unit = g.consume();
                        self.apply_advanced_unit(unit);
                        self.lex_state = DecimalPoint;
                        return Ok(None);
                    }
                    if matches!(c, 'e' | 'E') {
                        let unit = g.consume();
                        self.apply_advanced_unit(unit);
                        self.lex_state = DecimalExponent;
                        return Ok(None);
                    }
                    let tok = match scanner.emit() {
                        scanner::Capture::Borrowed(v) => Token::NumberBorrowed(v),
                        scanner::Capture::Owned(v) => Token::Number(v),
                        scanner::Capture::Raw(_) => {
                            unreachable!("Cannot be raw, never fed non-ASCII bytes.");
                        }
                    };
                    return Ok(Some(self.new_token(tok, false)));
                }
                Ok(Some(self.new_token(Token::Eof, true)))
            }

            DecimalInteger => {
                if let Some(g) = scanner.peek_guard() {
                    let c = g.ch();
                    if c == '.' {
                        let unit = g.consume();
                        self.apply_advanced_unit(unit);
                        self.lex_state = DecimalPoint;
                        return Ok(None);
                    }
                    if matches!(c, 'e' | 'E') {
                        let unit = g.consume();
                        self.apply_advanced_unit(unit);
                        self.lex_state = DecimalExponent;
                        return Ok(None);
                    }
                    if c.is_ascii_digit() {
                        let unit = g.consume();
                        self.apply_advanced_unit(unit);
                        let consumed = scanner.consume_while_ascii(|d| d.is_ascii_digit());
                        self.column += consumed;
                        self.pos += consumed;
                        return Ok(None);
                    }
                    let tok = match scanner.emit() {
                        scanner::Capture::Borrowed(v) => Token::NumberBorrowed(v),
                        scanner::Capture::Owned(v) => Token::Number(v),
                        scanner::Capture::Raw(_) => {
                            unreachable!("Cannot be raw, never fed non-ASCII bytes.");
                        }
                    };
                    return Ok(Some(self.new_token(tok, false)));
                }
                Ok(Some(self.new_token(Token::Eof, true)))
            }

            DecimalPoint => {
                if let Some(g) = scanner.peek_guard() {
                    let c = g.ch();
                    if matches!(c, 'e' | 'E') {
                        let unit = g.consume();
                        self.apply_advanced_unit(unit);
                        self.lex_state = DecimalExponent;
                        return Ok(None);
                    }
                    if c.is_ascii_digit() {
                        let unit = g.consume();
                        self.apply_advanced_unit(unit);
                        self.lex_state = DecimalFraction;
                        let consumed = scanner.consume_while_ascii(|d| d.is_ascii_digit());
                        self.column += consumed;
                        self.pos += consumed;
                        return Ok(None);
                    }
                    return Err(self.read_and_invalid_char(Char(c)));
                }
                Ok(Some(self.new_token(Token::Eof, true)))
            }

            DecimalFraction => {
                if let Some(g) = scanner.peek_guard() {
                    let c = g.ch();
                    if matches!(c, 'e' | 'E') {
                        let unit = g.consume();
                        self.apply_advanced_unit(unit);
                        self.lex_state = DecimalExponent;
                        return Ok(None);
                    }
                    if c.is_ascii_digit() {
                        let unit = g.consume();
                        self.apply_advanced_unit(unit);
                        let consumed = scanner.consume_while_ascii(|d| d.is_ascii_digit());
                        self.column += consumed;
                        self.pos += consumed;
                        return Ok(None);
                    }
                    let tok = match scanner.emit() {
                        scanner::Capture::Borrowed(v) => Token::NumberBorrowed(v),
                        scanner::Capture::Owned(v) => Token::Number(v),
                        scanner::Capture::Raw(_) => {
                            unreachable!("Cannot be raw, never fed non-ASCII bytes.");
                        }
                    };
                    return Ok(Some(self.new_token(tok, false)));
                }
                Ok(Some(self.new_token(Token::Eof, true)))
            }

            DecimalExponent => {
                if let Some(g) = scanner.peek_guard() {
                    let c = g.ch();
                    if matches!(c, '+' | '-') {
                        let unit = g.consume();
                        self.apply_advanced_unit(unit);
                        self.lex_state = DecimalExponentSign;
                        return Ok(None);
                    }
                    if c.is_ascii_digit() {
                        let unit = g.consume();
                        self.apply_advanced_unit(unit);
                        self.lex_state = DecimalExponentInteger;
                        let consumed = scanner.consume_while_ascii(|d| d.is_ascii_digit());
                        self.column += consumed;
                        self.pos += consumed;
                        return Ok(None);
                    }
                    return Err(self.read_and_invalid_char(Char(c)));
                }
                Ok(Some(self.new_token(Token::Eof, true)))
            }

            DecimalExponentSign => {
                if let Some(g) = scanner.peek_guard() {
                    let c = g.ch();
                    if c.is_ascii_digit() {
                        let unit = g.consume();
                        self.apply_advanced_unit(unit);
                        self.lex_state = DecimalExponentInteger;
                        let consumed = scanner.consume_while_ascii(|d| d.is_ascii_digit());
                        self.column += consumed;
                        self.pos += consumed;
                        return Ok(None);
                    }
                    return Err(self.read_and_invalid_char(Char(c)));
                }
                Ok(Some(self.new_token(Token::Eof, true)))
            },

            DecimalExponentInteger => {
                if let Some(g) = scanner.peek_guard() {
                    let c = g.ch();
                    if c.is_ascii_digit() {
                        let unit = g.consume();
                        self.apply_advanced_unit(unit);
                        let consumed = scanner.consume_while_ascii(|d| d.is_ascii_digit());
                        self.column += consumed;
                        self.pos += consumed;
                        return Ok(None);
                    }
                    let tok = match scanner.emit() {
                        scanner::Capture::Borrowed(v) => Token::NumberBorrowed(v),
                        scanner::Capture::Owned(v) => Token::Number(v),
                        scanner::Capture::Raw(_) => {
                            unreachable!("Cannot be raw, never fed non-ASCII bytes.");
                        }
                    };
                    return Ok(Some(self.new_token(tok, false)));
                }
                Ok(Some(self.new_token(Token::Eof, true)))
            },

            // -------------------------- STRING -----------------------------
            LexState::String => match self.peek_char(scanner) {
                // escape sequence
                Char('\\') => {
                    // TODO: eventually we will want to emit a partial fragment here, but to
                    // maintain parity with the existing implementation _we do not_.
                    //
                    // We pass consume: false to the scanner to skip the escape start symbol.
                    if let Some(g) = scanner.peek_guard() {
                        let unit = g.skip();
                        self.apply_advanced_unit(unit);
                    }
                    self.lex_state = LexState::StringEscape;
                    Ok(None)
                }
                // closing quote -> complete string
                Char('"') => {
                    // Finalize pending high surrogate if any
                    if let Some(high) = self.pending_high_surrogate.take() {
                        match self.decode_mode {
                            DecodeMode::StrictUnicode => {
                                return Err(self.syntax_error(error::SyntaxError::InvalidUnicodeEscapeSequence(high as u32)));
                            }
                            DecodeMode::ReplaceInvalid => {
                                scanner.push_transformed_char('\u{FFFD}');
                            }
                            DecodeMode::SurrogatePreserving => {
                                self.push_wtf8_for_u32(scanner, high as u32);
                            }
                        }
                    }
                    // Important: emit before consuming the closing quote so the
                    // scanner's anchor remains borrow-eligible and the end
                    // index excludes the delimiter. Then advance past '"'.
                    let tok = self.produce_string(false, scanner);
                    if let Some(g) = scanner.peek_guard() {
                        let unit = g.skip();
                        self.apply_advanced_unit(unit);
                    }
                    Ok(Some(tok))
                }
                Char(c @ '\0'..='\x1F') => {
                    // JSON spec allows 0x20 .. 0x10FFFF unescaped.
                    Err(self.read_and_invalid_char(Char(c)))
                }
                Empty => {
                    if let Some(s) = scanner.try_borrow_slice() {
                        if !s.is_empty() {
                            return Ok(Some(self.produce_string(true, scanner)));
                        }
                    }
                    Ok(Some(self.new_token(Token::Eof, true)))
                },
                Char(c) => {
                    // If a previous high surrogate was pending but no low surrogate followed,
                    // finalize it now before consuming the normal character.
                    if let Some(high) = self.pending_high_surrogate.take() {
                        match self.decode_mode {
                            DecodeMode::StrictUnicode => {
                                return Err(self.syntax_error(error::SyntaxError::InvalidUnicodeEscapeSequence(high as u32)));
                            }
                            DecodeMode::ReplaceInvalid => {
                                scanner.push_transformed_char('\u{FFFD}');
                            }
                            DecodeMode::SurrogatePreserving => {
                                self.push_wtf8_for_u32(scanner, high as u32);
                            }
                        }
                    }
                    // Fast-path: keep scanner and source in lockstep. First let the
                    // scanner consume from the current source (ring or batch) until
                    // a boundary or special char, then mirror exactly that many
                    // chars into our local buffer from the source queue.

                    if let Some(g) = scanner.peek_guard() {
                        let unit = g.consume();
                        self.apply_advanced_unit(unit);
                    }

                    // let consumed = scanner
                    //     .consume_while_char(|ch| ch != '\\' && ch != '"' && ch >= '\u{20}');
                    // if consumed > 0 {
                    //     let copied = self.source.copy_n(&mut self.buffer, consumed);
                    //     self.column += copied;
                    //     self.pos += copied;
                    // }
                    Ok(None)
                }
                EndOfInput => Err(self.read_and_invalid_char(EndOfInput)),
            },

            StringEscape => match self.peek_char(scanner) {
                Empty => {
                    if let Some(s) = scanner.try_borrow_slice() {
                        if !s.is_empty() {
                            return Ok(Some(self.produce_string(true, scanner)));
                        }
                    }
                    Ok(Some(self.new_token(Token::Eof, true)))
                },
                Char(ch) if matches!(ch, '"' | '\\' | '/') => {
                    if let Some(g) = scanner.peek_guard() {
                        let unit = g.consume();
                        self.apply_advanced_unit(unit);
                    }
                    self.lex_state = LexState::String;
                    Ok(None)
                }
                Char('b') => {
                    if let Some(g) = scanner.peek_guard() {
                        let unit = g.skip();
                        self.apply_advanced_unit(unit);
                    }
                    let ch = '\u{0008}';
                    scanner.push_transformed_char(ch);
                    self.lex_state = LexState::String;
                    Ok(None)
                }
                Char('f') => {
                    if let Some(g) = scanner.peek_guard() {
                        let unit = g.skip();
                        self.apply_advanced_unit(unit);
                    }
                    let ch = '\u{000C}';
                    scanner.push_transformed_char(ch);
                    self.lex_state = LexState::String;
                    Ok(None)
                }
                Char('n') => {
                    if let Some(g) = scanner.peek_guard() {
                        let unit = g.skip();
                        self.apply_advanced_unit(unit);
                    }
                    let ch = '\n';
                    scanner.push_transformed_char(ch);
                    self.lex_state = LexState::String;
                    Ok(None)
                }
                Char('r') => {
                    if let Some(g) = scanner.peek_guard() {
                        let unit = g.skip();
                        self.apply_advanced_unit(unit);
                    }
                    let ch = '\r';
                    scanner.push_transformed_char(ch);
                    self.lex_state = LexState::String;
                    Ok(None)
                }
                Char('t') => {
                    if let Some(g) = scanner.peek_guard() {
                        let unit = g.skip();
                        self.apply_advanced_unit(unit);
                    }
                    let ch = '\t';
                    scanner.push_transformed_char(ch);
                    self.lex_state = LexState::String;
                    Ok(None)
                }
                Char('u') => {
                    // If we have a borrowable prefix (e.g., preceding plain text before the
                    // escape), emit it as a partial fragment before transitioning to
                    // unicode-escape handling.
                    if let Some(s) = scanner.try_borrow_slice() {
                        if !s.is_empty() {
                            return Ok(Some(self.produce_string(true, scanner)));
                        }
                    }
                    if let Some(g) = scanner.peek_guard() {
                        let unit = g.skip();
                        self.apply_advanced_unit(unit);
                    }
                    self.unicode_escape_buffer.reset();
                    self.lex_state = LexState::StringEscapeUnicode;
                    Ok(None)
                }
                Char('U') if self.allow_uppercase_u => {
                    if let Some(g) = scanner.peek_guard() {
                        let unit = g.skip();
                        self.apply_advanced_unit(unit);
                    }
                    self.unicode_escape_buffer.reset();
                    self.lex_state = LexState::StringEscapeUnicode;
                    Ok(None)
                }
                c => Err(self.read_and_invalid_char(c)),
            },

            StringEscapeUnicode => {
                match self.peek_char(scanner) {
                    Empty => {
                        if let Some(s) = scanner.try_borrow_slice() {
                            if !s.is_empty() {
                                return Ok(Some(self.produce_string(true, scanner)));
                            }
                        }
                        Ok(Some(self.new_token(Token::Eof, true)))
                    },
                    Char(c) if c.is_ascii_hexdigit() => {
                        if let Some(g) = scanner.peek_guard() {
                            let unit = g.skip();
                            self.apply_advanced_unit(unit);
                        }
                        match self.unicode_escape_buffer.feed(c) {
                            Ok(Some(ch)) => {
                                // If a previous high surrogate is pending but we received a non-low scalar,
                                // finalize the pending one before appending this char.
                                if let Some(high) = self.pending_high_surrogate.take() {
                                    match self.decode_mode {
                                        DecodeMode::StrictUnicode => {
                                            return Err(self.syntax_error(error::SyntaxError::InvalidUnicodeEscapeSequence(high as u32)));
                                        }
                                        DecodeMode::ReplaceInvalid => {
                                            scanner.push_transformed_char('\u{FFFD}');
                                        }
                                        DecodeMode::SurrogatePreserving => {
                                            self.push_wtf8_for_u32(scanner, high as u32);
                                        }
                                    }
                                }
                                scanner.push_transformed_char(ch);
                                self.lex_state = LexState::String;
                                Ok(None)
                            }
                            Ok(None) => Ok(None),
                            Err(err @ error::SyntaxError::InvalidUnicodeEscapeSequence(code)) => {
                                // Handle surrogate halves per decode_mode
                                let is_high = (0xD800..=0xDBFF).contains(&code);
                                let is_low = (0xDC00..=0xDFFF).contains(&code);
                                if !is_high && !is_low {
                                    return Err(self.syntax_error(err));
                                }
                                if is_high {
                                    match self.decode_mode {
                                        DecodeMode::StrictUnicode => {
                                            // Defer error; remember pending high surrogate and await a low.
                                            self.pending_high_surrogate = Some(code as u16);
                                            self.lex_state = LexState::String;
                                            Ok(None)
                                        }
                                        DecodeMode::ReplaceInvalid => {
                                            scanner.push_transformed_char('\u{FFFD}');
                                            self.lex_state = LexState::String;
                                            Ok(None)
                                        }
                                        DecodeMode::SurrogatePreserving => {
                                            // Hold high surrogate to combine if a low follows next.
                                            self.pending_high_surrogate = Some(code as u16);
                                            self.lex_state = LexState::String;
                                            Ok(None)
                                        }
                                    }
                                } else {
                                    // low surrogate
                                    if let Some(high) = self.pending_high_surrogate.take() {
                                        let hi = (high as u32) - 0xD800;
                                        let lo = code - 0xDC00;
                                        let cp = 0x1_0000 + ((hi << 10) | lo);
                                        match self.decode_mode {
                                            DecodeMode::StrictUnicode | DecodeMode::ReplaceInvalid => {
                                                if let Some(ch) = core::char::from_u32(cp) {
                                                    scanner.push_transformed_char(ch);
                                                } else {
                                                    // Shouldn't happen; cp is valid by construction
                                                    scanner.push_transformed_char('\u{FFFD}');
                                                }
                                            }
                                            DecodeMode::SurrogatePreserving => {
                                                self.push_wtf8_for_u32(scanner, cp);
                                            }
                                        }
                                        self.lex_state = LexState::String;
                                        Ok(None)
                                    } else {
                                        // Lone low surrogate
                                        match self.decode_mode {
                                            DecodeMode::StrictUnicode => Err(self.syntax_error(err)),
                                            DecodeMode::ReplaceInvalid => {
                                                scanner.push_transformed_char('\u{FFFD}');
                                                self.lex_state = LexState::String;
                                                Ok(None)
                                            }
                                            DecodeMode::SurrogatePreserving => {
                                                self.push_wtf8_for_u32(scanner, code);
                                                self.lex_state = LexState::String;
                                                Ok(None)
                                            }
                                        }
                                    }
                                }
                            }
                            Err(err) => Err(self.syntax_error(err)),
                        }
                    }
                    EndOfInput => {
                        // consume EOF sentinel and advance column to match TS behavior
                        // No guard available; mirror previous behavior: bump column
                        // to stay in sync with tests.
                        self.column += 1;
                        Err(self.invalid_eof())
                    }
                    c @ Char(_) => Err(self.read_and_invalid_char(c)),
                }
            }

            Start => {
                if let Some(g) = scanner.peek_guard() {
                    let c = g.ch();
                    if matches!(c, '{' | '[') {
                        let unit = g.skip();
                        self.apply_advanced_unit(unit);
                        return Ok(Some(self.new_token(Token::Punctuator(c as u8), false)));
                    }
                }
                self.lex_state = LexState::Value;
                Ok(None)
            }

            BeforePropertyName => {
                if let Some(g) = scanner.peek_guard() {
                    let c = g.ch();
                    if c == '}' {
                        let unit = g.skip();
                        self.apply_advanced_unit(unit);
                        return Ok(Some(self.new_token(Token::Punctuator(b'}'), false)));
                    }
                    if c == '"' {
                        let unit = g.skip();
                        self.apply_advanced_unit(unit);
                        scanner.emit();
                        self.lex_state = LexState::String;
                        return Ok(None);
                    }
                    return Err(self.read_and_invalid_char(Char(c)));
                }
                Err(self.read_and_invalid_char(Empty))
            }

            AfterPropertyName => {
                if let Some(g) = scanner.peek_guard() {
                    let c = g.ch();
                    if c == ':' {
                        let unit = g.skip();
                        self.apply_advanced_unit(unit);
                        return Ok(Some(self.new_token(Token::Punctuator(c as u8), false)));
                    }
                    return Err(self.read_and_invalid_char(Char(c)));
                }
                Err(self.read_and_invalid_char(Empty))
            }

            BeforePropertyValue => {
                self.lex_state = LexState::Value;
                Ok(None)
            }

            AfterPropertyValue => {
                if let Some(g) = scanner.peek_guard() {
                    let c = g.ch();
                    if matches!(c, ',' | '}') {
                        let unit = g.skip();
                        self.apply_advanced_unit(unit);
                        return Ok(Some(self.new_token(Token::Punctuator(c as u8), false)));
                    }
                    return Err(self.read_and_invalid_char(Char(c)));
                }
                Err(self.read_and_invalid_char(Empty))
            }

            BeforeArrayValue => {
                if let Some(g) = scanner.peek_guard() {
                    if g.ch() == ']' {
                        let unit = g.skip();
                        self.apply_advanced_unit(unit);
                        return Ok(Some(self.new_token(Token::Punctuator(b']'), false)));
                    }
                }
                self.lex_state = LexState::Value;
                Ok(None)
            }

            AfterArrayValue => {
                if let Some(g) = scanner.peek_guard() {
                    let c = g.ch();
                    if matches!(c, ',' | ']') {
                        let unit = g.skip();
                        self.apply_advanced_unit(unit);
                        return Ok(Some(self.new_token(Token::Punctuator(c as u8), false)));
                    }
                    return Err(self.read_and_invalid_char(Char(c)));
                }
                Err(self.read_and_invalid_char(Empty))
            }

            End => {
                let c = self.peek_char(scanner);
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
        token: Token<'src>,
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
                Token::PropertyNameRaw(value) => {
                    if !self.pending_key {
                        ctx.pop_kind(path);
                    }
                    ctx.push_key_from_raw_str(path, &value);
                    self.pending_key = false;
                    self.parse_state = AfterPropertyName;
                    Ok(None)
                }
                Token::PropertyName(value) => {
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
        token: Token<'src>,
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
            Token::NumberBorrowed(n) => {
                let value = f
                    .new_number(n)
                    .map_err(|e| self.event_context_error(e))?;
                Some(ParseEvent::Number {
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
            Token::StringBorrowed(fragment) => {
                let fragment = f
                    .new_str(fragment)
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
            Token::StringOwned(fragment) => {
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
            Token::StringRaw(fragment) => {
                let fragment = f
                    .new_str_raw_owned(fragment)
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
            Token::PropertyName(_) => {
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
    pub(crate) fn get_lexed_tokens(&self) -> &[Token<'static>] {
        &self.lexed_tokens
    }
}

impl StreamingParserImpl<RustContext> {
    pub fn new(options: ParserOptions) -> Self {
        let mut ctx = RustContext::default();
        Self::new_with_factory(&mut ctx, options)
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
        self.feed_with(RustContext::default(), text)
    }

    #[must_use]
    pub fn finish(self) -> ClosedStreamingParser<'static, RustContext> {
        self.finish_with(RustContext::default())
    }
}

#[cfg(test)]
mod tests;
