//! The JSON streaming parser implementation.
//!
//! This module provides the incremental streaming parser that processes input
//! in chunks and emits `ParseEvent`s. The core does not build composite values
//! or buffer full strings; adapters are responsible for those behaviors.

#![expect(clippy::struct_excessive_bools)]
#![expect(clippy::inline_always)]
#![allow(
    clippy::elidable_lifetime_names,
    clippy::type_complexity,
    clippy::wrong_self_convention
)]

mod buffer;
mod error;
mod escape_buffer;
mod event_builder;
mod literal_buffer;
mod options;
mod scanner;

use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::mem::{ManuallyDrop, MaybeUninit};

pub use error::{ErrorSource, ParserError, SyntaxError};
use escape_buffer::UnicodeEscapeBuffer;
pub use event_builder::EventBuilder;
use literal_buffer::ExpectedLiteralBuffer;
use options::DecodeMode;
pub use options::ParserOptions;

// buffer is no longer used directly by the parser core; Scanner owns input state.
pub use crate::event::ParseEvent;
#[cfg(test)]
#[allow(unused_imports)]
pub use crate::event::test_util;
use crate::{
    context::{EventCtx, PathKind},
    lending_iterator::LendingIterator,
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

use PeekedChar::{Char, Empty, EndOfInput};

/// ------------------------------------------------------------------------------------------------
/// State machines (1‑for‑1 with TS enums)
/// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParseState {
    Start,
    BeforePropertyName,
    AfterPropertyName,
    BeforePropertyValue,
    BeforeFirstArrayValue,
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
            ParseState::BeforeFirstArrayValue | ParseState::BeforeArrayValue => {
                LexState::BeforeArrayValue
            }
            ParseState::AfterPropertyValue => LexState::AfterPropertyValue,
            ParseState::AfterArrayValue => LexState::AfterArrayValue,
            ParseState::End => LexState::End,
            ParseState::Error => LexState::Error,
        }
    }
}

///
/// `JsonModem` can be fed partial or complete JSON input in chunks.
/// It implements a lending iterator that yields `ParseEvent`s representing
/// JSON tokens and structural events.
pub struct JsonModem<Ctx: EventCtx> {
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

    path: MaybeUninit<Ctx::PathState>,
    /// Indicates if a we've started parsing a string value and have not yet
    /// emitted a parse event. Determines the value of `is_initial` on
    /// [`ParseEvent::String`].
    initialized_string: bool,

    /// Options
    allow_unicode_whitespace: bool,

    /// Allow multiple JSON values in a single input (support transition from
    /// end state to a new value start state)
    multiple_values: bool,

    /// Unicode escape decoding behavior
    decode_mode: DecodeMode,

    /// Panic on syntax errors instead of returning them. Only affects execution
    /// in non-release builds.
    #[doc(hidden)]
    panic_on_error: bool,

    /// Tracks a pending high surrogate (0xD800..=0xDBFF) seen via \u escapes
    /// awaiting a following low surrogate to form a single code point.
    pending_high_surrogate: Option<u16>,
    /// Compatibility knob: accept uppercase 'U' for Unicode escapes
    /// (e.g., "\\UD83D\\UDE00").
    allow_uppercase_u: bool,
}

impl<Ctx: EventCtx> Drop for JsonModem<Ctx> {
    fn drop(&mut self) {
        // SAFETY: We are in control of the lifetime of `self.path`, and we ensure it is
        // fully initialized in `new`. The only place where it is uninitialized
        // is when an iterator holds the thawed path, and it must drop before
        // `JsonModem` is dropped.
        unsafe { MaybeUninit::assume_init_drop(&mut self.path) };
    }
}

pub struct JsonModemIterator<'p, 'src, Ctx: EventCtx> {
    parser: &'p mut JsonModem<Ctx>,
    path: ManuallyDrop<Ctx::Path>,
    pub(crate) factory: Ctx,
    scanner: Scanner<'src>,
}

impl<'src, Ctx: EventCtx> JsonModemIterator<'_, 'src, Ctx> {
    #[allow(clippy::wrong_self_convention)]
    pub fn to_iter(
        mut self,
    ) -> impl Iterator<Item = Result<ParseEvent<'src, Ctx::Path, Ctx>, ParserError<Ctx>>>
    where
        Ctx::Path: Clone,
    {
        core::iter::from_fn(
            move || -> Option<Result<ParseEvent<'src, Ctx::Path, Ctx>, ParserError<Ctx>>> {
                match self.next() {
                    None => None,
                    Some(Ok(evt)) => Some(Ok(evt.into())),
                    Some(Err(err)) => Some(Err(err)),
                }
            },
        )
    }
}

impl<Ctx: EventCtx> Drop for JsonModemIterator<'_, '_, Ctx> {
    #[inline]
    fn drop(&mut self) {
        // SAFETY: ManuallyDrop::take moves out without running Drop,
        // so the later field-drop won’t double-drop it.
        let thawed = unsafe { ManuallyDrop::take(&mut self.path) };
        self.parser.path = MaybeUninit::new(self.factory.freeze(thawed));

        // Persist scanner carryover (unread tail + token scratch + positions)
        self.parser.scanner_state = core::mem::take(&mut self.scanner).finish();
    }
}

impl<'src, Ctx: EventCtx> LendingIterator for JsonModemIterator<'_, 'src, Ctx> {
    type Item<'a>
        = Result<ParseEvent<'src, &'a Ctx::Path, Ctx>, ParserError<Ctx>>
    where
        Self: 'a;

    fn next(&mut self) -> Option<Self::Item<'_>> {
        self.parser
            .next_event_with(&mut self.factory, &mut self.path, &mut self.scanner)
    }
}

/// A `JsonModem` that has been closed to further input.
///
/// Returned by [`JsonModem::finish`], this parser will process any
/// remaining input and then end. It implements `Iterator` to yield
/// `ParseEvent` results.
pub struct JsonModemClosed<'src, Ctx: EventCtx> {
    parser: JsonModem<Ctx>,
    path: ManuallyDrop<Ctx::Path>,
    pub(crate) factory: Ctx,
    scanner: Scanner<'src>,
}

impl<Ctx: EventCtx> Drop for JsonModemClosed<'_, Ctx> {
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

impl<'src, Ctx: EventCtx> JsonModemClosed<'src, Ctx> {
    #[allow(clippy::wrong_self_convention)]
    pub fn to_iter(
        mut self,
    ) -> impl Iterator<Item = Result<ParseEvent<'src, Ctx::Path, Ctx>, ParserError<Ctx>>>
    where
        Ctx::Path: Clone,
    {
        core::iter::from_fn(
            move || -> Option<Result<ParseEvent<'src, Ctx::Path, Ctx>, ParserError<Ctx>>> {
                match self.next() {
                    None => None,
                    Some(Ok(evt)) => Some(Ok(evt.into())),
                    Some(Err(err)) => Some(Err(err)),
                }
            },
        )
    }
}

impl<'src, Ctx: EventCtx> LendingIterator for JsonModemClosed<'src, Ctx> {
    type Item<'a>
        = Result<ParseEvent<'src, &'a Ctx::Path, Ctx>, ParserError<Ctx>>
    where
        Self: 'a;

    fn next(&mut self) -> Option<Self::Item<'_>> {
        self.parser
            .next_event_with(&mut self.factory, &mut self.path, &mut self.scanner)
    }
}

impl<Ctx: EventCtx> JsonModem<Ctx> {
    #[must_use]
    /// Creates a new `JsonModem` with the given event factory and options.
    pub fn new_with_factory(f: &mut Ctx, options: ParserOptions) -> JsonModem<Ctx> {
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

            multiple_values: options.allow_multiple_json_values,
            decode_mode: options.decode_mode,
            allow_uppercase_u: options.allow_uppercase_u,
            allow_unicode_whitespace: options.allow_unicode_whitespace,
            panic_on_error: options.panic_on_error,
            pending_high_surrogate: None,
        }
    }

    #[doc(hidden)]
    pub fn feed_with<'p, 'src>(
        &'p mut self,
        mut factory: Ctx,
        text: &'src str,
    ) -> JsonModemIterator<'p, 'src, Ctx> {
        let path = unsafe { factory.thaw(core::mem::take(self.path.assume_init_mut())) };
        let path = ManuallyDrop::new(path);
        let scanner = Scanner::from_state(core::mem::take(&mut self.scanner_state), text);
        JsonModemIterator {
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
    /// `JsonModemClosed` implements `Iterator` yielding `ParseEvent`s
    /// and then ends.
    pub fn finish_with<'src>(mut self, mut context: Ctx) -> JsonModemClosed<'src, Ctx> {
        self.close();
        let path = unsafe { context.thaw(core::mem::take(self.path.assume_init_mut())) };
        let path = ManuallyDrop::new(path);
        let scanner = Scanner::from_state(core::mem::take(&mut self.scanner_state), "");
        JsonModemClosed {
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
    #[inline]
    pub(crate) fn next_event_with<'a, 'src>(
        &'_ mut self,
        f: &mut Ctx,
        path: &'a mut Ctx::Path,
        scanner: &mut Scanner<'src>,
    ) -> Option<Result<ParseEvent<'src, &'a Ctx::Path, Ctx>, ParserError<Ctx>>> {
        match self.next_event_step(f, path, scanner) {
            None => None,
            Some(Ok(event)) => {
                let event = event;
                Some(Ok(event.with_path(path)))
            }
            Some(Err(err)) => {
                debug_assert!(
                    !self.panic_on_error,
                    "Syntax error at {}:{}: {err}",
                    self.line, self.column
                );
                self.parse_state = ParseState::Error;
                self.lex_state = LexState::Error;
                Some(Err(err))
            }
        }
    }

    #[inline]
    fn pop(&'_ mut self, f: &mut Ctx, path: &mut Ctx::Path) {
        let _ = f.pop_kind(path);
        self.parse_state = match f.last_kind(path) {
            Some(PathKind::Index) => ParseState::AfterArrayValue,
            Some(PathKind::Key) => ParseState::AfterPropertyValue,
            None => ParseState::End,
        };
    }

    #[inline]
    fn next_event_step<'src>(
        &mut self,
        f: &mut Ctx,
        path: &mut Ctx::Path,
        scanner: &mut Scanner<'src>,
    ) -> Option<Result<ParseEvent<'src, (), Ctx>, ParserError<Ctx>>> {
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
                    debug_assert!(
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
                    debug_assert!(
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

    #[inline]
    fn lex<'src>(&mut self, scanner: &mut Scanner<'src>) -> Result<Token<'src>, ParserError<Ctx>> {
        if !self.partial_lex {
            self.lex_state = LexState::Default;
        }

        loop {
            if let Some(tok) = self.lex_state_step(self.lex_state, scanner)? {
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

    fn read_and_invalid_char(&mut self, c: PeekedChar) -> ParserError<Ctx> {
        self.invalid_char(c)
    }

    #[allow(dead_code)]
    #[inline(always)]
    fn advance_char(&mut self, scanner: &mut Scanner<'_>, consume: bool) {
        // Deprecated: prefer using peek_guard(). This remains for transitional
        // calls outside refactored branches.
        let adv = if consume {
            scanner.consume()
        } else {
            scanner.skip()
        };
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

    #[inline(always)]
    fn produce_borrowed_fragment<'src>(
        &mut self,
        partial: bool,
        fragment: &'src str,
    ) -> Token<'src> {
        use Token::Eof;

        self.partial_lex = partial;

        if self.parse_state == ParseState::BeforePropertyName {
            if partial {
                return Eof;
            }
            return Token::PropertyName(fragment.into());
        }

        Token::StringBorrowed(fragment)
    }

    #[expect(clippy::too_many_lines)]
    #[inline]
    fn lex_state_step<'src>(
        &mut self,
        lex_state: LexState,
        scanner: &mut Scanner<'src>,
    ) -> Result<Option<Token<'src>>, ParserError<Ctx>> {
        use LexState::{
            AfterArrayValue, AfterPropertyName, AfterPropertyValue, BeforeArrayValue,
            BeforePropertyName, BeforePropertyValue, DecimalExponent, DecimalExponentInteger,
            DecimalExponentSign, DecimalFraction, DecimalInteger, DecimalPoint, Default, End,
            Error, Sign, Start, StringEscape, StringEscapeUnicode, Value, ValueLiteral, Zero,
        };

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
                    if self.allow_unicode_whitespace
                        && (c.is_whitespace() || matches!(c, '\u{FEFF}'))
                    {
                        // When enabled, accept all Unicode whitespace and BOM
                        let unit = g.skip();
                        self.apply_advanced_unit(unit);
                        return Ok(None);
                    }
                    // Delegate to parse-state entry without consuming
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
                        let consumed_fast = scanner.consume_digits_ascii_fast();
                        if consumed_fast > 0 {
                            self.column += consumed_fast;
                            self.pos += consumed_fast;
                        } else {
                            let consumed = scanner.consume_while_ascii(|d| d.is_ascii_digit());
                            self.column += consumed;
                            self.pos += consumed;
                        }
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
                        let consumed_fast = scanner.consume_digits_ascii_fast();
                        if consumed_fast > 0 {
                            self.column += consumed_fast;
                            self.pos += consumed_fast;
                        } else {
                            let consumed = scanner.consume_while_ascii(|d| d.is_ascii_digit());
                            self.column += consumed;
                            self.pos += consumed;
                        }
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
                        let consumed_fast = scanner.consume_digits_ascii_fast();
                        if consumed_fast > 0 {
                            self.column += consumed_fast;
                            self.pos += consumed_fast;
                        } else {
                            let consumed = scanner.consume_while_ascii(|d| d.is_ascii_digit());
                            self.column += consumed;
                            self.pos += consumed;
                        }
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
            }

            DecimalExponentInteger => {
                if let Some(g) = scanner.peek_guard() {
                    let c = g.ch();
                    if c.is_ascii_digit() {
                        let unit = g.consume();
                        self.apply_advanced_unit(unit);
                        let consumed_fast = scanner.consume_digits_ascii_fast();
                        if consumed_fast > 0 {
                            self.column += consumed_fast;
                            self.pos += consumed_fast;
                        } else {
                            let consumed = scanner.consume_while_ascii(|d| d.is_ascii_digit());
                            self.column += consumed;
                            self.pos += consumed;
                        }
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

            // -------------------------- STRING -----------------------------
            LexState::String => {
                if self.pending_high_surrogate.is_none() {
                    let skipped = scanner.consume_string_ascii_fast();
                    if skipped > 0 {
                        self.column += skipped;
                        self.pos += skipped;
                        return Ok(None);
                    }
                }
                match self.peek_char(scanner) {
                    // escape sequence
                    Char('\\') => {
                        if matches!(self.parse_state, ParseState::BeforePropertyName) {
                            scanner.ensure_prefix_copied();
                        } else if let Some(fragment) = scanner.try_emit_borrowed_fragment() {
                            return Ok(Some(self.produce_borrowed_fragment(true, fragment)));
                        }

                        // Skip the backslash and enter escape state.
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
                                    return Err(self.syntax_error(
                                        error::SyntaxError::InvalidUnicodeEscapeSequence(
                                            u32::from(high),
                                        ),
                                    ));
                                }
                                DecodeMode::ReplaceInvalid => {
                                    scanner.push_char('\u{FFFD}');
                                }
                                DecodeMode::SurrogatePreserving => {
                                    scanner.push_codepoint(u32::from(high));
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
                        if !matches!(self.parse_state, ParseState::BeforePropertyName) {
                            if let Some(fragment) = scanner.try_emit_borrowed_fragment() {
                                return Ok(Some(self.produce_borrowed_fragment(true, fragment)));
                            }
                        }
                        Ok(Some(self.new_token(Token::Eof, true)))
                    }
                    Char(_c) => {
                        // If a previous high surrogate was pending but no low surrogate followed,
                        // finalize it now before consuming the normal character.
                        if let Some(high) = self.pending_high_surrogate.take() {
                            match self.decode_mode {
                                DecodeMode::StrictUnicode => {
                                    return Err(self.syntax_error(
                                        error::SyntaxError::InvalidUnicodeEscapeSequence(
                                            u32::from(high),
                                        ),
                                    ));
                                }
                                DecodeMode::ReplaceInvalid => {
                                    scanner.push_char('\u{FFFD}');
                                }
                                DecodeMode::SurrogatePreserving => {
                                    scanner.push_codepoint(u32::from(high));
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

                        Ok(None)
                    }
                    EndOfInput => Err(self.read_and_invalid_char(EndOfInput)),
                }
            }

            StringEscape => match self.peek_char(scanner) {
                Empty => {
                    if !matches!(self.parse_state, ParseState::BeforePropertyName) {
                        if let Some(fragment) = scanner.try_emit_borrowed_fragment() {
                            return Ok(Some(self.produce_borrowed_fragment(true, fragment)));
                        }
                    }
                    Ok(Some(self.new_token(Token::Eof, true)))
                }
                Char('"' | '\\' | '/') => {
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
                    scanner.push_char(ch);
                    self.lex_state = LexState::String;
                    Ok(None)
                }
                Char('f') => {
                    if let Some(g) = scanner.peek_guard() {
                        let unit = g.skip();
                        self.apply_advanced_unit(unit);
                    }
                    let ch = '\u{000C}';
                    scanner.push_char(ch);
                    self.lex_state = LexState::String;
                    Ok(None)
                }
                Char('n') => {
                    if let Some(g) = scanner.peek_guard() {
                        let unit = g.skip();
                        self.apply_advanced_unit(unit);
                    }
                    let ch = '\n';
                    scanner.push_char(ch);
                    self.lex_state = LexState::String;
                    Ok(None)
                }
                Char('r') => {
                    if let Some(g) = scanner.peek_guard() {
                        let unit = g.skip();
                        self.apply_advanced_unit(unit);
                    }
                    let ch = '\r';
                    scanner.push_char(ch);
                    self.lex_state = LexState::String;
                    Ok(None)
                }
                Char('t') => {
                    if let Some(g) = scanner.peek_guard() {
                        let unit = g.skip();
                        self.apply_advanced_unit(unit);
                    }
                    let ch = '\t';
                    scanner.push_char(ch);
                    self.lex_state = LexState::String;
                    Ok(None)
                }
                Char('u') => {
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
                        if !matches!(self.parse_state, ParseState::BeforePropertyName) {
                            if let Some(fragment) = scanner.try_emit_borrowed_fragment() {
                                return Ok(Some(self.produce_borrowed_fragment(true, fragment)));
                            }
                        }
                        Ok(Some(self.new_token(Token::Eof, true)))
                    }
                    Char(c) if c.is_ascii_hexdigit() => {
                        if let Some(g) = scanner.peek_guard() {
                            let unit = g.skip();
                            self.apply_advanced_unit(unit);
                        }
                        match self.unicode_escape_buffer.feed(c) {
                            Ok(Some(ch)) => {
                                // If a previous high surrogate is pending but we received a non-low
                                // scalar, finalize the pending one
                                // before appending this char.
                                if let Some(high) = self.pending_high_surrogate.take() {
                                    match self.decode_mode {
                                        DecodeMode::StrictUnicode => {
                                            return Err(self.syntax_error(
                                                error::SyntaxError::InvalidUnicodeEscapeSequence(
                                                    u32::from(high),
                                                ),
                                            ));
                                        }
                                        DecodeMode::ReplaceInvalid => {
                                            scanner.push_char('\u{FFFD}');
                                        }
                                        DecodeMode::SurrogatePreserving => {
                                            scanner.push_codepoint(u32::from(high));
                                        }
                                    }
                                }
                                scanner.push_char(ch);
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
                                        #[allow(clippy::cast_possible_truncation)]
                                        DecodeMode::StrictUnicode => {
                                            // Defer error; remember pending high surrogate and
                                            // await a low.
                                            self.pending_high_surrogate = Some(code as u16);
                                            self.lex_state = LexState::String;
                                            Ok(None)
                                        }
                                        DecodeMode::ReplaceInvalid => {
                                            scanner.push_char('\u{FFFD}');
                                            self.lex_state = LexState::String;
                                            Ok(None)
                                        }
                                        #[allow(clippy::cast_possible_truncation)]
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
                                        let hi = u32::from(high) - 0xD800;
                                        let lo = code - 0xDC00;
                                        let cp = 0x1_0000 + ((hi << 10) | lo);
                                        match self.decode_mode {
                                            DecodeMode::StrictUnicode
                                            | DecodeMode::ReplaceInvalid => {
                                                if let Some(ch) = core::char::from_u32(cp) {
                                                    scanner.push_char(ch);
                                                } else {
                                                    // Shouldn't happen; cp is valid by construction
                                                    scanner.push_char('\u{FFFD}');
                                                }
                                            }
                                            DecodeMode::SurrogatePreserving => {
                                                scanner.push_codepoint(cp);
                                            }
                                        }
                                        self.lex_state = LexState::String;
                                        Ok(None)
                                    } else {
                                        // Lone low surrogate
                                        match self.decode_mode {
                                            DecodeMode::StrictUnicode => {
                                                Err(self.syntax_error(err))
                                            }
                                            DecodeMode::ReplaceInvalid
                                            | DecodeMode::SurrogatePreserving => {
                                                // In UTF-8 backends (including Raw backend tests),
                                                // SurrogatePreserving
                                                // degrades to replacement.
                                                scanner.push_char('\u{FFFD}');
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
    #[allow(clippy::too_many_lines)]
    fn dispatch_parse_state<'src>(
        &mut self,
        token: Token<'src>,
        ctx: &mut Ctx,
        path: &mut Ctx::Path,
    ) -> Result<Option<ParseEvent<'src, (), Ctx>>, ParserError<Ctx>> {
        use ParseState::{
            AfterArrayValue, AfterPropertyName, AfterPropertyValue, BeforeArrayValue,
            BeforeFirstArrayValue, BeforePropertyName, BeforePropertyValue, End, Error, Start,
        };

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
                    ctx.push_key_from_raw_str(path, &value);
                    self.parse_state = AfterPropertyName;
                    Ok(None)
                }
                Token::PropertyName(value) => {
                    ctx.push_key_from_str(path, &value);
                    self.parse_state = AfterPropertyName;
                    Ok(None)
                }
                Token::Punctuator(b'}') => {
                    // Closing an object before any property: do not pop the
                    // path (no key was pushed yet). Update parse state based on
                    // the parent context.
                    self.parse_state = match ctx.last_kind(path) {
                        Some(PathKind::Index) => ParseState::AfterArrayValue,
                        Some(PathKind::Key) => ParseState::AfterPropertyValue,
                        None => ParseState::End,
                    };
                    Ok(Some(ParseEvent::ObjectEnd { path: () }))
                }
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

            BeforeFirstArrayValue => match token {
                Token::Eof => Ok(None),
                Token::Punctuator(b']') => {
                    self.parse_state = match ctx.last_kind(path) {
                        Some(PathKind::Index) => ParseState::AfterArrayValue,
                        Some(PathKind::Key) => ParseState::AfterPropertyValue,
                        None => ParseState::End,
                    };
                    Ok(Some(ParseEvent::ArrayEnd { path: () }))
                }
                _ => {
                    ctx.push_index_zero(path);

                    self.parse_state = ParseState::BeforeArrayValue;
                    self.push(token, ctx, path)
                }
            },

            BeforeArrayValue => match token {
                Token::Eof => Ok(None),
                Token::Punctuator(b']') => {
                    self.pop(ctx, path);
                    Ok(Some(ParseEvent::ArrayEnd { path: () }))
                }
                _ => self.push(token, ctx, path),
            },

            AfterPropertyValue => match token {
                Token::Eof if self.end_of_input => Err(self.invalid_eof()),
                Token::Punctuator(b',') => {
                    self.pop(ctx, path);
                    self.parse_state = BeforePropertyName;
                    Ok(None)
                }
                Token::Punctuator(b'}') => {
                    self.pop(ctx, path);
                    Ok(Some(ParseEvent::ObjectEnd { path: () }))
                }
                _ => Ok(None),
            },

            AfterArrayValue => match token {
                Token::Eof if self.end_of_input => Err(self.invalid_eof()),
                Token::Punctuator(b',') => {
                    ctx.bump_last_index(path)
                        .map_err(|e| self.syntax_error(SyntaxError::PathError(e)))?;
                    self.parse_state = ParseState::BeforeArrayValue;
                    Ok(None)
                }
                Token::Punctuator(b']') => {
                    self.pop(ctx, path);
                    Ok(Some(ParseEvent::ArrayEnd { path: () }))
                }
                _ => Ok(None),
            },
            End | Error => Ok(None),
        }
    }

    #[inline]
    fn push<'src, 'a>(
        &'_ mut self,
        token: Token<'src>,
        f: &'_ mut Ctx,
        path: &'a Ctx::Path,
    ) -> Result<Option<ParseEvent<'src, (), Ctx>>, ParserError<Ctx>> {
        let evt: Option<ParseEvent<'src, (), Ctx>> = match token {
            Token::Punctuator(b'{') => {
                self.parse_state = ParseState::BeforePropertyName;
                return Ok(Some(ParseEvent::ObjectBegin { path: () }));
            }
            Token::Punctuator(b'[') => {
                self.parse_state = ParseState::BeforeFirstArrayValue;
                return Ok(Some(ParseEvent::ArrayBegin { path: () }));
            }

            Token::Null => Some(ParseEvent::Null { path: () }),
            Token::Boolean(b) => {
                let value = f.new_bool(b).map_err(|e| self.event_context_error(e))?;
                Some(ParseEvent::Boolean { path: (), value })
            }
            Token::NumberBorrowed(n) => {
                let value = f.new_number(n).map_err(|e| self.event_context_error(e))?;
                Some(ParseEvent::Number { path: (), value })
            }
            Token::Number(n) => {
                let value = f
                    .new_number_owned(n)
                    .map_err(|e| self.event_context_error(e))?;
                Some(ParseEvent::Number { path: (), value })
            }
            Token::StringBorrowed(fragment) => {
                let fragment = f
                    .new_str(fragment)
                    .map_err(|e| self.event_context_error(e))?;
                let is_initial = self.initialized_string;
                let is_final = !self.partial_lex;
                self.initialized_string = false;
                Some(ParseEvent::String {
                    path: (),
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
                    path: (),
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
                    path: (),
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
    fn invalid_char(&self, c: PeekedChar) -> ParserError<Ctx> {
        match c {
            EndOfInput | Empty => self.syntax_error(SyntaxError::UnexpectedEndOfInput),
            Char(c) => self.syntax_error(SyntaxError::InvalidCharacter(c)),
        }
    }

    fn invalid_eof(&self) -> ParserError<Ctx> {
        self.syntax_error(SyntaxError::UnexpectedEndOfInput)
    }

    fn event_context_error(&self, err: Ctx::Error) -> ParserError<Ctx> {
        self.parser_error(ErrorSource::EventContextError(err))
    }

    fn syntax_error(&self, err: SyntaxError) -> ParserError<Ctx> {
        self.parser_error(ErrorSource::SyntaxError(err))
    }

    fn parser_error(&self, err: ErrorSource<Ctx>) -> ParserError<Ctx> {
        let err = ParserError {
            source: err,
            line: self.line,
            column: self.column,
        };
        debug_assert!(!self.panic_on_error, "{err}");
        err
    }

    #[allow(dead_code)]
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
}

impl<Ctx> JsonModem<Ctx>
where
    Ctx: EventCtx + Default,
{
    /// Creates a new [`JsonModem`] using `Ctx::default()` as the backend.
    #[must_use]
    pub fn new(options: ParserOptions) -> Self {
        let mut ctx = Ctx::default();
        Self::new_with_factory(&mut ctx, options)
    }

    /// Creates a [`JsonModem`] backed by an existing context instance.
    #[must_use]
    pub fn new_with_context(ctx: &mut Ctx, options: ParserOptions) -> Self {
        Self::new_with_factory(ctx, options)
    }

    /// Feeds a chunk of JSON text and returns a lending iterator over events.
    pub fn feed<'p, 'src>(&'p mut self, text: &'src str) -> JsonModemIterator<'p, 'src, Ctx> {
        self.feed_with(Ctx::default(), text)
    }

    /// Finishes parsing and returns an iterator draining any remaining events.
    #[must_use]
    pub fn finish(self) -> JsonModemClosed<'static, Ctx> {
        self.finish_with(Ctx::default())
    }
}

#[cfg(test)]
mod tests;
