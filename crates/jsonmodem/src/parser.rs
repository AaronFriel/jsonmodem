//! The JSON streaming parser implementation.
//!
//! This module provides the `StreamingParser` for incremental JSON parsing,
//! capable of processing input in chunks and emitting `ParseEvent`s.
//!
//! # Examples
//!
//! Basic usage:
//!
//! ```rust
//! use jsonmodem::{ParseEvent, ParserOptions, StreamingParser};
//!
//! let mut parser = StreamingParser::new(ParserOptions::default());
//! parser.feed(r#"{"key": [null, true, 3.14]}"#);
//! for event in parser.finish() {
//!     let event = event.unwrap();
//!     println!("{:?}", event);
//! }
//! ```
#![allow(clippy::single_match_else)]
#![allow(clippy::enum_glob_use)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::inline_always)]

use alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use core::{f64, fmt};

use crate::{
    StringValueMode,
    buffer::Buffer,
    escape_buffer::UnicodeEscapeBuffer,
    event::{ParseEvent, PathComponent},
    event_stack::EventStack,
    literal_buffer::{self, ExpectedLiteralBuffer},
    options::{NonScalarValueMode, ParserOptions},
    value_zipper::{ValueBuilder, ZipperError},
};

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
        value: Option<String>,
        fragment: String,
    },
    Boolean(bool),
    Null,
    Number(f64),
    #[allow(clippy::doc_link_with_quotes)]
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

/// Stack entry – one per open container
#[derive(Clone, Debug)]
pub enum Frame {
    Array {
        next_index: usize, // slot for the next element
    },
    Object {
        pending_key: Option<String>, // key waiting for its value
    },
}

impl Frame {
    pub fn new_array_frame() -> Self {
        Frame::Array { next_index: 0 }
    }

    pub fn new_object_frame() -> Self {
        Frame::Object { pending_key: None }
    }

    pub fn to_path_component(&self) -> PathComponent {
        match self {
            Frame::Array { next_index } => PathComponent::Index(*next_index),
            Frame::Object { pending_key } => {
                PathComponent::Key(pending_key.clone().unwrap_or_default())
            }
        }
    }
}

#[derive(Debug)]
pub struct FrameStack {
    root: Option<Frame>,
    stack: Vec<(PathComponent, Frame)>,
}

impl Default for FrameStack {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for FrameStack {
    fn clone(&self) -> Self {
        let mut stack = Vec::with_capacity(self.stack.len());
        stack.extend(self.stack.iter().cloned());
        Self {
            root: self.root.clone(),
            stack,
        }
    }
}

impl FrameStack {
    pub fn new() -> Self {
        Self {
            root: None,
            stack: Vec::with_capacity(16),
        }
    }

    pub fn last(&self) -> Option<&Frame> {
        if let Some((_, frame)) = self.stack.last() {
            return Some(frame);
        }
        self.root.as_ref()
    }

    pub fn last_mut(&mut self) -> Option<&mut Frame> {
        if let Some((_, frame)) = self.stack.last_mut() {
            Some(frame)
        } else {
            self.root.as_mut()
        }
    }

    pub fn push(&mut self, frame: Frame) {
        match self.last() {
            Some(last_frame) => {
                let next_path_component = last_frame.to_path_component();
                self.stack.push((next_path_component, frame));
            }
            None => {
                self.root = Some(frame);
            }
        }
    }

    pub fn pop(&mut self) -> Option<Frame> {
        match self.stack.pop() {
            Some((_, f)) => Some(f),
            None => self.root.take(),
        }
    }

    pub fn to_path_components(&self) -> Vec<PathComponent> {
        self.stack.iter().map(|(pc, _)| pc.clone()).collect()
    }

    pub fn clear(&mut self) {
        self.root = None;
        self.stack.clear();
    }
}

#[derive(Debug)]
/// The streaming JSON parser.
///
/// `StreamingParser` can be fed partial or complete JSON input in chunks.
/// It implements `Iterator` to yield `ParseEvent`s representing JSON tokens
/// and structural events.
///
/// # Examples
///
/// ```rust
/// use jsonmodem::{ParseEvent, ParserOptions, StreamingParser};
///
/// let mut parser = StreamingParser::new(ParserOptions::default());
/// parser.feed(r#"[{"key": "value"}, true, null]"#);
/// let mut closed = parser.finish();
/// while let Some(result) = closed.next() {
///     let event = result.unwrap();
///     println!("{:?}", event);
/// }
/// ```
pub struct StreamingParser {
    // Raw source buffer (always grows then gets truncated after each “round”).
    source: Buffer,
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
    fragment_start: usize, // used to track string fragments start position within `buffer`
    unicode_escape_buffer: UnicodeEscapeBuffer, // for unicode escapes
    expected_literal: ExpectedLiteralBuffer,
    partial_lex: bool, // true ← we returned an *incomplete* token

    /// Last token we produced
    frames: FrameStack, // stack of open containers (arrays or objects)
    events: EventStack,

    multiple_values: bool,
    string_value_mode: StringValueMode,
    non_scalar_values: NonScalarValueMode,

    /// Panic on syntax errors instead of returning them
    #[cfg(test)]
    panic_on_error: bool,

    /// Sequence of tokens produced by the lexer.
    #[cfg(test)]
    lexed_tokens: Vec<Token>,
}

impl Default for StreamingParser {
    fn default() -> Self {
        Self::new(ParserOptions::default())
    }
}

impl Iterator for StreamingParser {
    type Item = Result<ParseEvent, ParserError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_event()
    }
}

/// A `StreamingParser` that has been closed to further input.
///
/// Returned by [`StreamingParser::finish`], this parser will process any
/// remaining input and then end. It implements `Iterator` to yield
/// `ParseEvent` results.
pub struct ClosedStreamingParser {
    parser: StreamingParser,
}

impl ClosedStreamingParser {
    #[cfg(test)]
    pub(crate) fn get_lexed_tokens(&self) -> &[Token] {
        self.parser.get_lexed_tokens()
    }

    pub(crate) fn unstable_get_current_value_ref(&self) -> Option<&crate::value::Value> {
        self.parser.unstable_get_current_value_ref()
    }
}

impl Iterator for ClosedStreamingParser {
    type Item = Result<ParseEvent, ParserError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.parser.next_event()
    }
}

impl StreamingParser {
    #[must_use]
    /// Creates a new `StreamingParser` with the given options.
    ///
    /// # Arguments
    ///
    /// * `options` - Parser configuration options.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use jsonmodem::{ParserOptions, StreamingParser};
    ///
    /// let parser = StreamingParser::new(ParserOptions {
    ///     allow_multiple_json_values: true,
    ///     ..Default::default()
    /// });
    /// ```
    pub fn new(options: ParserOptions) -> Self {
        Self {
            source: Buffer::new(),
            end_of_input: false,
            partial_lex: false,

            pos: 0,
            line: 1,
            column: 1,

            lex_state: LexState::Default,
            parse_state: ParseState::Start,

            buffer: String::new(),
            fragment_start: 0,
            unicode_escape_buffer: UnicodeEscapeBuffer::new(),
            expected_literal: ExpectedLiteralBuffer::none(),
            frames: FrameStack::new(),

            events: EventStack::new(
                vec![],
                if matches!(options.non_scalar_values, NonScalarValueMode::None) {
                    None
                } else {
                    Some(ValueBuilder::Empty)
                },
            ),

            multiple_values: options.allow_multiple_json_values,
            string_value_mode: options.string_value_mode,
            non_scalar_values: options.non_scalar_values,
            #[cfg(test)]
            panic_on_error: options.panic_on_error,
            #[cfg(test)]
            lexed_tokens: vec![],
        }
    }

    /// Feeds a chunk of JSON text into the parser.
    ///
    /// The parser buffers the input and parses it incrementally,
    /// yielding events when complete JSON tokens or structures are recognized.
    ///
    /// # Arguments
    ///
    /// * `text` - A string slice containing JSON data or partial data.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use jsonmodem::{StreamingParser, ParserOptions};
    /// let mut parser = StreamingParser::new(ParserOptions::default());
    /// parser.feed("{\"hello\":");
    /// ```
    pub fn feed(&mut self, text: &str) {
        self.source.push(text);
    }

    #[must_use]
    /// Marks the end of input and returns a closed parser to consume pending
    /// events.
    ///
    /// After calling `finish`, no further input can be fed. The returned
    /// `ClosedStreamingParser` implements `Iterator` yielding `ParseEvent`s
    /// and then ends.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use jsonmodem::{ParseEvent, ParserOptions, StreamingParser};
    /// let mut parser = StreamingParser::new(ParserOptions::default());
    /// parser.feed("true");
    /// let mut closed = parser.finish();
    /// assert_eq!(
    ///     closed.next().unwrap().unwrap(),
    ///     ParseEvent::Boolean {
    ///         path: vec![],
    ///         value: true
    ///     }
    /// );
    /// ```
    pub fn finish(mut self) -> ClosedStreamingParser {
        self.end_of_input = true;
        ClosedStreamingParser { parser: self }
    }

    /// Experimental helper that returns the *currently* fully-parsed JSON value
    /// (if any).
    ///
    /// ⚠️ **Unstable API** – exposed solely for benchmarking and may change or
    /// disappear without notice.
    #[doc(hidden)]
    #[doc(hidden)]
    #[must_use]
    pub fn unstable_get_current_value_ref(&self) -> Option<&crate::value::Value> {
        self.events.read_root()
    }

    #[cfg(test)]
    pub(crate) fn current_value(&self) -> Option<crate::value::Value> {
        self.unstable_get_current_value_ref().cloned()
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
    fn next_event(&mut self) -> Option<Result<ParseEvent, ParserError>> {
        match self.next_event_internal() {
            Some(Ok(event)) => Some(Ok(event)),
            None => None,
            Some(Err(err)) => {
                #[cfg(test)]
                assert!(
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

    fn next_event_internal(&mut self) -> Option<Result<ParseEvent, ParserError>> {
        if self.parse_state == ParseState::Error {
            // If we are in error state, we can’t produce any more events
            return None;
        }

        loop {
            #[cfg(any(test, feature = "fuzzing"))]
            assert!(
                self.events.len() <= 1,
                "Internal error: more than one event in the queue"
            );
            // Anything already queued up?
            if let Some(ev) = self.events.pop() {
                if matches!(self.non_scalar_values, NonScalarValueMode::Roots)
                    && !Self::is_root_event(&ev)
                {
                    continue;
                }
                return Some(Ok(ev));
            }

            // Streaming reset (mirrors TS `if (this.stream && this.parseState === 'end')`)
            if self.multiple_values && matches!(self.parse_state, ParseState::End) {
                // Reset *except* the source buffer
                self.lex_state = LexState::Default;
                self.parse_state = ParseState::Start;
                self.frames.clear();
                self.events = EventStack::new(
                    vec![],
                    if matches!(self.non_scalar_values, NonScalarValueMode::None) {
                        None
                    } else {
                        Some(ValueBuilder::Empty)
                    },
                );
            }

            // Drive the old lexer / dispatcher one token forward
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
            match self.dispatch_parse_state(token) {
                Ok(()) => {}
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

            // Stop when we reach EoF or partial token
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
    fn lex(&mut self) -> Result<Token, ParserError> {
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

    fn read_and_invalid_char(&mut self, c: PeekedChar) -> ParserError {
        // self.advance_char();
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

        match self.string_value_mode {
            _ if partial && self.buffer.len() == self.fragment_start => Eof,
            StringValueMode::None => {
                let fragment = core::mem::take(&mut self.buffer);
                String {
                    fragment,
                    value: None,
                }
            }
            StringValueMode::Values if partial => {
                let fragment = self.buffer[self.fragment_start..].to_string();
                self.fragment_start = self.buffer.len(); // reset for next fragment
                String {
                    fragment,
                    value: None,
                }
            }
            StringValueMode::Values => {
                let fragment = self.buffer[self.fragment_start..].to_string();
                let value = core::mem::take(&mut self.buffer);
                self.fragment_start = self.buffer.len(); // reset for next fragment
                String {
                    fragment,
                    value: Some(value),
                }
            }
            StringValueMode::Prefixes => {
                let fragment = self.buffer[self.fragment_start..].to_string();
                let value = if partial {
                    self.fragment_start = self.buffer.len(); // reset for next fragment
                    self.buffer.clone()
                } else {
                    self.fragment_start = 0; // reset for next fragment
                    core::mem::take(&mut self.buffer)
                };
                String {
                    fragment,
                    value: Some(value),
                }
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    #[inline(always)]
    fn lex_state_step(
        &mut self,
        lex_state: LexState,
        next_char: PeekedChar,
    ) -> Result<Option<Token>, ParserError> {
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
                    let num = self.buffer.parse::<f64>().unwrap();
                    self.buffer.clear();
                    Ok(Some(self.new_token(Token::Number(num), false)))
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
                    let num = self.buffer.parse::<f64>().unwrap();
                    self.buffer.clear();
                    Ok(Some(self.new_token(Token::Number(num), false)))
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
                    let num = self.buffer.parse::<f64>().unwrap();
                    self.buffer.clear();
                    Ok(Some(self.new_token(Token::Number(num), false)))
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
                    let num = self.buffer.parse::<f64>().unwrap();
                    self.buffer.clear();
                    Ok(Some(self.new_token(Token::Number(num), false)))
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
                            Err(err) => Err(self
                                .syntax_error(format!("Invalid unicode escape sequence: {err}"))),
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
    fn dispatch_parse_state(&mut self, token: Token) -> Result<(), ParserError> {
        use ParseState::*;

        match self.parse_state {
            // In single-value mode, EOF at start when end_of_input indicates unexpected end.
            Start => match token {
                Token::Eof if self.end_of_input && !self.multiple_values => {
                    return Err(self.invalid_eof());
                }
                Token::Eof => (),
                _ => self.push(token)?,
            },

            BeforePropertyName => match token {
                Token::Eof if self.end_of_input => return Err(self.invalid_eof()),
                Token::PropertyName { value } => {
                    match self.frames.last_mut() {
                        Some(Frame::Object { pending_key }) => {
                            *pending_key = Some(value);
                        }
                        _ => Err(self
                            .syntax_error("Expected object frame for property name".to_string()))?,
                    }
                    self.parse_state = AfterPropertyName;
                }
                Token::Punctuator(_) => self.pop()?,
                Token::String { .. } => {
                    return Err(
                        self.syntax_error("Unexpected string value in property name".to_string())
                    );
                }
                _ => (),
            },

            AfterPropertyName => match token {
                Token::Eof if self.end_of_input => return Err(self.invalid_eof()),
                Token::Eof => (),
                _ => self.parse_state = BeforePropertyValue,
            },

            BeforePropertyValue => match token {
                Token::Eof => (),
                _ => self.push(token)?,
            },

            BeforeArrayValue => match token {
                Token::Eof => (),
                Token::Punctuator(b']') => self.pop()?,
                _ => self.push(token)?,
            },

            AfterPropertyValue => match token {
                Token::Eof if self.end_of_input => return Err(self.invalid_eof()),
                Token::Punctuator(b',') => {
                    if let Some(Frame::Object { pending_key }) = self.frames.last_mut() {
                        *pending_key = None; // <-- reset for next property
                    }
                    self.parse_state = BeforePropertyName;
                }
                Token::Punctuator(b'}') => self.pop()?,
                _ => (),
            },

            AfterArrayValue => match token {
                Token::Eof if self.end_of_input => return Err(self.invalid_eof()),
                Token::Punctuator(b',') => {
                    match self.frames.last_mut() {
                        Some(Frame::Array { next_index }) => {
                            *next_index += 1; // increment index for next value
                        }
                        _ => Err(self.syntax_error(
                            "Expected array frame for after array value".to_string(),
                        ))?,
                    }

                    self.parse_state = BeforeArrayValue;
                }
                Token::Punctuator(b']') => self.pop()?,
                _ => (),
            },
            End | Error => {}
        }

        Ok(())
    }

    #[inline(always)]
    fn pop(&mut self) -> Result<(), ParserError> {
        let path = self.frames.to_path_components();
        match self.frames.pop() {
            Some(Frame::Array { .. }) => {
                self.events
                    .push(ParseEvent::ArrayEnd { path, value: None })
                    .map_err(|err| self.zipper_error(err))?;
            }
            Some(Frame::Object { .. }) => {
                self.events
                    .push(ParseEvent::ObjectEnd { path, value: None })
                    .map_err(|err| self.zipper_error(err))?;
            }
            _ => {}
        }

        // We actually need to peek at the new last frame and restore the parse state
        // now:
        if let Some(last_frame) = self.frames.last() {
            self.parse_state = match last_frame {
                Frame::Array { .. } => ParseState::AfterArrayValue,
                Frame::Object { .. } => ParseState::AfterPropertyValue,
            };
        } else {
            self.parse_state = ParseState::End;
        }

        Ok(())
    }

    #[inline(always)]
    fn push(&mut self, token: Token) -> Result<(), ParserError> {
        match token {
            Token::Punctuator(b'{') => {
                self.frames.push(Frame::new_object_frame());
                self.events
                    .push(ParseEvent::ObjectBegin {
                        path: self.frames.to_path_components(),
                    })
                    .map_err(|err| self.zipper_error(err))?;
                self.parse_state = ParseState::BeforePropertyName;
                return Ok(());
            }
            Token::Punctuator(b'[') => {
                self.frames.push(Frame::new_array_frame());
                self.events
                    .push(ParseEvent::ArrayStart {
                        path: self.frames.to_path_components(),
                    })
                    .map_err(|err| self.zipper_error(err))?;
                self.parse_state = ParseState::BeforeArrayValue;
                return Ok(());
            }
            _ => {
                // Handle primitive values below
            }
        }

        let mut path = self.frames.to_path_components();
        if let Some(frame) = self.frames.last() {
            path.push(frame.to_path_component());
        }

        match (token, self.partial_lex) {
            (Token::Null, _) => {
                self.events
                    .push(ParseEvent::Null { path })
                    .map_err(|err| self.zipper_error(err))?;
            }
            (Token::Boolean(b), _) => {
                self.events
                    .push(ParseEvent::Boolean { path, value: b })
                    .map_err(|err| self.zipper_error(err))?;
            }
            (Token::Number(n), _) => {
                self.events
                    .push(ParseEvent::Number { path, value: n })
                    .map_err(|err| self.zipper_error(err))?;
            }
            // Streaming string fragments (partial) build up until the full string is complete.
            (Token::String { fragment, value }, partial) => {
                self.events
                    .push(ParseEvent::String {
                        path,
                        fragment,
                        value,
                        is_final: !partial,
                    })
                    .map_err(|err| self.zipper_error(err))?;
            }
            (Token::PropertyName { .. }, _) => {
                return Err(
                    self.syntax_error("Unexpected property name outside of object".to_string())
                );
            }
            _ => (),
        }

        // 3. Adjust parse state exactly once, using `parent_kind`
        if !self.partial_lex {
            if let Some(Frame::Object { pending_key }) = self.frames.last_mut() {
                *pending_key = None;
            }

            self.parse_state = match self.frames.last() {
                None => ParseState::End,
                Some(Frame::Array { .. }) => ParseState::AfterArrayValue,
                Some(Frame::Object { .. }) => ParseState::AfterPropertyValue,
            };
        }

        Ok(())
    }

    // ------------------------------------------------------------------------------------------------
    // Errors
    // ------------------------------------------------------------------------------------------------
    fn invalid_char(&self, c: PeekedChar) -> ParserError {
        match c {
            EndOfInput | Empty => self.syntax_error("JSON5: invalid end of input".to_string()),
            Char(c) => self.syntax_error(format!(
                "JSON5: invalid character '{}' at {}:{}",
                Self::format_char(c),
                self.line,
                self.column
            )),
        }
    }

    fn invalid_eof(&self) -> ParserError {
        self.syntax_error("JSON5: invalid end of input".to_string())
    }

    fn syntax_error(&self, msg: String) -> ParserError {
        let err = ParserError {
            msg,
            line: self.line,
            column: self.column,
        };
        #[cfg(test)]
        assert!(!self.panic_on_error, "{err}");
        err
    }

    fn zipper_error(&self, err: ZipperError) -> ParserError {
        self.syntax_error(format!("Internal error: {err}"))
    }

    fn is_root_event(ev: &ParseEvent) -> bool {
        use ParseEvent::*;
        match ev {
            Null { path }
            | Boolean { path, .. }
            | Number { path, .. }
            | String { path, .. }
            | ArrayStart { path }
            | ArrayEnd { path, .. }
            | ObjectBegin { path }
            | ObjectEnd { path, .. } => path.is_empty(),
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_of_parser() {
        use core::mem::size_of;
        assert_eq!(size_of::<StreamingParser>(), 280);
    }

    #[test]
    fn size_of_closed_parser() {
        use core::mem::size_of;
        assert_eq!(size_of::<ClosedStreamingParser>(), 280);
    }
}
