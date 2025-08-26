//! JSON streaming parser with borrow-first events.

//!
//! Overview
//! - This module implements an incremental, streaming JSON parser that accepts
//!   input in chunks and yields `ParseEvent`s as soon as they become available.
//! - The parser is designed to minimize allocations: whenever a complete token
//!   (string fragment without escapes, or number) resides entirely in the
//!   current input batch, the event contains a borrowed `&'src str` view into
//!   that batch. Otherwise, the parser falls back to buffered (owned) fragments.
//!
//! Buffers and borrowing
//! - `source: Buffer` is a small ring of unread characters that backs the
//!   lexer. It contains only carry‑over data from previous iterations. Each
//!   feed drains the ring first, then reads directly from the new batch.
//!   The ring is appended to only when dropping the iterator with unread
//!   batch content.
//! - `buffer: String` is the per-token scratch buffer used when a token cannot
//!   be borrowed (e.g., a string encounters an escape, or a number crosses a
//!   batch). When emitting buffered events, content comes from this string.
//! - `BatchView` is created by `feed(...)` and held by the iterator. Any
//!   borrowed fragments refer to this view’s lifetime. The iterator’s lifetime
//!   guarantees these borrows remain valid for the duration of iteration.
//!
//! Drop semantics
//! - If the user drops the iterator mid-token, the parser must preserve the
//!   in-flight portion of the token so that subsequent parsing can continue.
//!   We copy the already-read portion of the token into `buffer: String` and
//!   switch the parser into buffered mode for the remainder of that token.
//!   This approach avoids reordering complexities that would arise from trying
//!   to “put back” characters into `source: Buffer`.
//!
//! Notes on copying
//! - The parser does not pre‑copy the fed batch into the ring. While the ring
//!   has unread characters, lexing occurs from it and produces owned data.
//!   Once empty, lexing proceeds directly over the batch with borrowed
//!   fragments where possible. Borrowed fragments never point into the ring.
//!
//! Guarantees per `next_event_with_and_batch`
//! - If `buffer` is non-empty, we read from it and emit owned fragments.
//! - Otherwise, when possible, we return borrowed fragments that lie entirely
//!   within the current `BatchView`. If a token cannot be borrowed, we copy it
//!   into `buffer` and emit owned fragments.
//!
//! This module provides the incremental streaming parser that processes input
//! in chunks and emits `ParseEvent`s. The core does not build composite values
//! or buffer full strings; adapters are responsible for those behaviors.
//!
//! Design: Borrow-First Tokens (zero-copy where possible)
//! -----------------------------------------------
//! Goal: avoid relying on an internal `buffer: String` when we can emit
//! completely borrowed string/number slices that refer to the fed input
//! chunk. We minimize allocations and copies for the common cases:
//! - Strings without escapes that are fully contained in the current feed
//!   batch are emitted as borrowed `&'src str` fragments.
//! - Numbers that are fully contained in the current batch are emitted as
//!   borrowed `&'src str` to be parsed/handled by the backend `EventCtx`.
//! - When escaping is encountered (e.g., `\u` sequences) or when a token spans
//!   across feeds, we fall back to buffering into the existing `buffer: String`.
//!
//! Key tradeoffs and choices:
//! - Unicode/escape handling: encountering any escape switches to buffered mode
//!   for that string fragment, because unescaped, decoded content differs from
//!   the source slice.
//! - Partial fragments: for strings we may emit partial fragments; if a partial
//!   fragment contains no escapes and lies fully in the current batch, it is
//!   borrowed. If not, we buffer.
//! - Numbers: if a number begins in a previous batch or finishes only after we
//!   see a delimiter in the next batch, we cannot return a single borrowed
//!   slice. We buffer in those cases.
//! - We never hand out references to the internal ring buffer; borrowed slices
//!   are taken directly from the current input batch slice lifetime.
//!
//! Implementation outline:
//! - Introduce an internal `LexToken<'src>` that can be either borrowed
//!   (`&'src str`) or buffered (`String`) for strings and numbers.
//! - Keep the public, test‑facing `Token` enum unchanged (owned strings). The
//!   lexer produces a `LexToken<'src>` used by the parser to build
//!   `ParseEvent<'src, B>`, and, when tests are enabled, it records a copy as a
//!   public `Token` for round‑trip tests.
//! - Track the current feed batch in the iterator (not in the parser) with its
//!   character span `[start_pos, end_pos)` in the global stream. While lexing,
//!   record token start positions; on token completion, if the entire token
//!   range is within the current batch and the token had no escapes (strings),
//!   emit a borrowed slice computed from the batch using character-to-byte index
//!   mapping. Otherwise, emit buffered.
//! - We only change this file; the ring buffer remains in use for stream
//!   continuity and as a fallback for buffering.

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
pub use options::{DecodeMode, ParserOptions};
pub use parse_event::ParseEvent;
pub use path::{Path, PathItem, PathItemFrom, PathLike};

use crate::backend::{EventCtx, PathCtx, PathKind, RawStrHint, RustContext};
#[cfg(test)]
use crate::backend::RawContext;

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

// Internal lexer token with borrow-or-buffer payloads for strings and numbers.
#[derive(Debug, Clone)]
enum LexToken<'src> {
    Eof,
    PropertyNameBorrowed(&'src str),
    PropertyNameBuffered,
    PropertyNameOwned(String),
    StringBorrowed(&'src str),
    StringBuffered,
    StringOwned(String),
    /// Raw bytes for string fragment (e.g. WTF-8 for preserved surrogates)
    StringRawOwned(alloc::vec::Vec<u8>),
    Boolean(bool),
    Null,
    NumberBorrowed(&'src str),
    NumberBuffered,
    NumberOwned(String),
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
    // After a high surrogate (\uD800..\uDBFF) we must encounter a backslash
    // starting the low surrogate sequence.
    StringEscapeUnicodeExpectBackslash,
    // After the backslash we must encounter the letter 'u'.
    StringEscapeUnicodeExpectU,
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
    /// Ring of unread characters backing the lexer. New input is copied here
    /// in `feed(...)`, and characters are consumed as the lexer advances.
    source: Buffer,
    end_of_input: bool,

    /// Current *global* character position.
    pos: usize,
    line: usize,
    column: usize,

    /// Current parse / lex states
    parse_state: ParseState,
    lex_state: LexState,

    /// Per-token scratch buffer used when a token cannot be borrowed. Reused
    /// for numbers, literals, and strings that require buffering.
    token_buffer: String,
    unicode_escape_buffer: UnicodeEscapeBuffer,
    expected_literal: ExpectedLiteralBuffer,
    partial_lex: bool,
    // Borrowing support
    total_chars_pushed: usize,
    token_start_pos: Option<usize>,
    string_had_escape: bool,
    // Tracks whether the current token must be emitted as owned (buffered).
    // This is intentionally NOT equivalent to `!self.source.is_empty()`. Even
    // when parsing directly from the current batch (ring is empty), we switch
    // to owned mode for this token if:
    // - an escape is encountered inside a string (decoded content differs), or
    // - the token spans ring→batch or otherwise cannot be borrowed as a single
    //   contiguous slice from the active batch.
    // Once set for a token, this remains true until the token finishes.
    token_is_owned: bool,
    /// Tracks consumption within the active feed batch.
    batch_cursor: BatchCursor,
    /// Owned fragment accumulator used during batch-mode string parsing
    owned_batch_buffer: String,
    /// Owned raw-byte accumulator for string parsing when preserving surrogates.
    owned_batch_raw: alloc::vec::Vec<u8>,
    // If we encountered a high surrogate in a Unicode escape, store it while
    // we parse the following low surrogate.
    pending_high_surrogate: Option<u16>,

    path: MaybeUninit<B::Frozen>,
    /// Indicates if a we've started parsing a string value and have not yet
    /// emitted a parse event. Determines the value of `is_initial` on
    /// [`ParseEvent::String`].
    initialized_string: bool,
    /// When true, the current string token accumulates into raw bytes instead of UTF-8 String.
    token_is_raw_bytes: bool,
    /// Indicates if a key is pending, i.e.: we have opened an object but have
    /// not pushed a key yet.
    pending_key: bool,

    /// Options

    /// Whether to allow any Unicode whitespace between JSON values.
    /// When `false` (default), only JSON's four whitespace code points are
    /// accepted: space (U+0020), line feed (U+000A), carriage return (U+000D),
    /// and horizontal tab (U+0009).
    allow_unicode_whitespace: bool,

    /// Allow multiple JSON values in a single input (support transition from
    /// end state to a new value start state)
    multiple_values: bool,
    /// Decode mode for Unicode escapes and surrogate handling.
    decode_mode: options::DecodeMode,
    /// Whether to allow uppercase `\U` introducer for escapes.
    allow_uppercase_u: bool,

    /// Panic on syntax errors instead of returning them
    #[cfg(test)]
    panic_on_error: bool,

    /// Sequence of tokens produced by the lexer.
    #[cfg(test)]
    lexed_tokens: alloc::vec::Vec<Token>,
}

#[derive(Default, Debug, Clone, Copy)]
struct BatchCursor {
    /// How many characters have been consumed from the active batch
    chars_consumed: usize,
    /// How many bytes have been consumed from the active batch
    bytes_consumed: usize,
}

struct BatchView<'src> {
    text: &'src str,
    start_pos: usize,
    end_pos: usize,
    // Cache the number of chars in `text` to avoid re-counting.
    len_chars: usize,
}

impl<'src> BatchView<'src> {
    #[inline]
    fn slice_chars(&self, start_chars: usize, end_chars: usize) -> &'src str {
        // Convert char offsets within the batch to byte offsets
        let mut start_byte = 0;
        let mut end_byte = self.text.len();

        if start_chars > 0 {
            let mut count = 0;
            for (i, _) in self.text.char_indices() {
                if count == start_chars {
                    start_byte = i;
                    break;
                }
                count += 1;
            }
            if count < start_chars {
                start_byte = self.text.len();
            }
        }

        if end_chars < self.len_chars {
            let mut count = 0;
            for (i, _) in self.text.char_indices() {
                if count == end_chars {
                    end_byte = i;
                    break;
                }
                count += 1;
            }
        }

        &self.text[start_byte..end_byte]
    }
}

pub struct StreamingParserIteratorWith<'p, 'src, B: PathCtx + EventCtx> {
    parser: &'p mut StreamingParserImpl<B>,
    path: ManuallyDrop<B::Thawed>,
    pub(crate) factory: B,
    _marker: core::marker::PhantomData<&'src ()>,
    batch: BatchView<'src>,
    cursor: BatchCursor,
}

impl<'p, 'src, B: PathCtx + EventCtx> Drop for StreamingParserIteratorWith<'p, 'src, B> {
    fn drop(&mut self) {
        // SAFETY: ManuallyDrop::take moves out without running Drop,
        // so the later field-drop won’t double-drop it.
        let thawed = unsafe { ManuallyDrop::take(&mut self.path) };
        self.parser.path = MaybeUninit::new(self.factory.freeze(thawed));
        // If an in-flight token has consumed characters in this batch, ensure
        // its already-read portion is preserved by copying into the parser's
        // internal buffer and switching to buffered mode for the remainder.
        if let Some(start) = self.parser.token_start_pos {
            if self.parser.pos > start {
                let batch_start = self.batch.start_pos.max(start);
                let batch_end = self.batch.end_pos.min(self.parser.pos);
                if batch_end > batch_start {
                    let rel_start = batch_start - self.batch.start_pos;
                    let rel_end = batch_end - self.batch.start_pos;
                    let s = self.batch.slice_chars(rel_start, rel_end);
                    self.parser.token_buffer.push_str(s);
                }
                self.parser.token_is_owned = true;
            }
        }

        // Push unread portion of this batch into the ring buffer so further
        // parsing proceeds from `self.source`.
        let consumed = self.cursor.chars_consumed.min(self.batch.end_pos - self.batch.start_pos);
        if consumed < (self.batch.end_pos - self.batch.start_pos) {
            let rest = self.batch.slice_chars(consumed, self.batch.end_pos - self.batch.start_pos);
            self.parser.source.push(rest);
        }
    }
}

impl<'src, B: PathCtx + EventCtx> Iterator for StreamingParserIteratorWith<'_, 'src, B> {
    type Item = Result<ParseEvent<'src, B>, ParserError<B>>;

    fn next(&mut self) -> Option<Self::Item> {
        self.parser
            .next_event_with_and_batch(&mut self.factory, &mut self.path, Some(&self.batch), Some(&mut self.cursor))
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
}

impl<'src, B: PathCtx + EventCtx> Drop for ClosedStreamingParser<'src, B> {
    fn drop(&mut self) {
        // SAFETY: ManuallyDrop::take moves out without running Drop,
        // so the later field-drop won’t double-drop it.
        let thawed = unsafe { ManuallyDrop::take(&mut self.path) };
        self.parser.path = MaybeUninit::new(self.factory.freeze(thawed));
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
            .next_event_with_and_batch(&mut self.factory, &mut self.path, None, None)
    }
}

impl<B: PathCtx + EventCtx> StreamingParserImpl<B> {
    #[must_use]
    /// Creates a new `StreamingParser` with the given event factory and
    /// options.
    pub fn new_with_factory(f: &mut B, options: ParserOptions) -> StreamingParserImpl<B> {
        Self {
            source: Buffer::new(),
            end_of_input: false,
            partial_lex: false,

            pos: 0,
            line: 1,
            column: 1,

            lex_state: LexState::Default,
            parse_state: ParseState::Start,

            token_buffer: String::new(),
            unicode_escape_buffer: UnicodeEscapeBuffer::new(),
            expected_literal: ExpectedLiteralBuffer::none(),
            total_chars_pushed: 0,
            token_start_pos: None,
            string_had_escape: false,
            token_is_owned: false,
            batch_cursor: BatchCursor::default(),
            owned_batch_buffer: String::new(),
            owned_batch_raw: alloc::vec::Vec::new(),
            pending_high_surrogate: None,

            path: MaybeUninit::new(f.frozen_new()),
            initialized_string: false,
            token_is_raw_bytes: false,
            pending_key: false,

            allow_unicode_whitespace: options.allow_unicode_whitespace,
            multiple_values: options.allow_multiple_json_values,
            decode_mode: options.decode_mode,
            allow_uppercase_u: options.allow_uppercase_u,
            #[cfg(test)]
            panic_on_error: options.panic_on_error,
            #[cfg(test)]
            lexed_tokens: alloc::vec::Vec::new(),
        }
    }

    #[cfg(test)]
    fn owned_from_lex_token<'src>(&self, t: LexToken<'src>) -> Token {
        match t {
            LexToken::Eof => Token::Eof,
            LexToken::Punctuator(p) => Token::Punctuator(p),
            LexToken::Null => Token::Null,
            LexToken::Boolean(b) => Token::Boolean(b),
            LexToken::StringBorrowed(s) => Token::String { fragment: s.to_string() },
            LexToken::StringBuffered => Token::String { fragment: self.token_buffer.clone() },
            LexToken::StringOwned(s) => Token::String { fragment: s },
            LexToken::StringRawOwned(bytes) => Token::String { fragment: String::from_utf8_lossy(&bytes).into_owned() },
            LexToken::PropertyNameBorrowed(s) => Token::PropertyName { value: s.to_string() },
            LexToken::PropertyNameBuffered => Token::PropertyName { value: self.token_buffer.clone() },
            LexToken::PropertyNameOwned(s) => Token::PropertyName { value: s },
            LexToken::NumberBorrowed(n) => Token::Number(n.to_string()),
            LexToken::NumberBuffered => Token::Number(self.token_buffer.clone()),
            LexToken::NumberOwned(s) => Token::Number(s),
        }
    }

    /// Pushes input into the internal ring.
    ///
    /// Note: this currently copies fed bytes into `source` so the lexer can
    /// operate incrementally. Borrowed event fragments are taken from the
    /// iterator's `BatchView`, never from this ring.
    pub(crate) fn feed_str(&mut self, text: &str) {
        self.source.push(text);
    }

    #[doc(hidden)]
    pub fn feed_with<'p, 'src>(
        &'p mut self,
        mut factory: B,
        text: &'src str,
    ) -> StreamingParserIteratorWith<'p, 'src, B> {
        // Track batch char span relative to the global stream.
        let batch_len = text.chars().count();
        let start_pos = self.total_chars_pushed;
        let end_pos = start_pos + batch_len;
        self.total_chars_pushed = end_pos;
        // Do not copy directly into the ring; parse from the batch.
        self.batch_cursor = BatchCursor::default();
        let path = unsafe { factory.thaw(core::mem::take(self.path.assume_init_mut())) };
        let path = ManuallyDrop::new(path);
        StreamingParserIteratorWith {
            parser: self,
            factory,
            path,
            _marker: core::marker::PhantomData,
            batch: BatchView { text, start_pos, end_pos, len_chars: batch_len },
            cursor: BatchCursor::default(),
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
        ClosedStreamingParser {
            parser: self,
            factory: context,
            path,
            _marker: core::marker::PhantomData,
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
    ) -> Option<Result<ParseEvent<'src, B>, ParserError<B>>> {
        match self.next_event_internal_with_batch(f, path, None, None) {
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

    fn next_event_with_and_batch<'a, 'cx: 'a, 'src: 'cx>(
        &mut self,
        f: &'cx mut B,
        path: &mut B::Thawed,
        batch: Option<&BatchView<'src>>,
        cursor: Option<&'a mut BatchCursor>,
    ) -> Option<Result<ParseEvent<'src, B>, ParserError<B>>> {
        match self.next_event_internal_with_batch(f, path, batch, cursor) {
            None => None,
            Some(Ok(event)) => Some(Ok(event)),
            Some(Err(err)) => {
                self.parse_state = ParseState::Error;
                self.lex_state = LexState::Error;
                Some(Err(err))
            }
        }
    }

    fn next_event_internal_with_batch<'a, 'cx: 'a, 'src: 'cx>(
        &'a mut self,
        f: &'cx mut B,
        path: &mut B::Thawed,
        batch: Option<&BatchView<'src>>,
        mut cursor: Option<&'a mut BatchCursor>,
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

            let token = match self.lex(batch, cursor.as_deref_mut()) {
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
            let is_eof = matches!(token, LexToken::Eof);
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
    fn lex<'src>(&mut self, batch: Option<&BatchView<'src>>, mut cursor: Option<&mut BatchCursor>) -> Result<LexToken<'src>, ParserError<B>> {
        if !self.partial_lex {
            self.lex_state = LexState::Default;
        }

        loop {
            let next_char = self.peek_char(batch, cursor.as_deref());
            if let Some(tok) = self.lex_state_step(self.lex_state, next_char, batch, cursor.as_deref_mut())? {
                #[cfg(test)]
                {
                    self.lexed_tokens.push(self.owned_from_lex_token(tok.clone()));
                }
                return Ok(tok);
            }
        }
    }

    /// Convenience – TS uses `undefined | eof` sentinel.  We return `None` for
    /// buffer depleted, `Some(EOI)` for forced end‑of‑input, else
    /// `Some(ch)`.
    #[inline(always)]
    fn peek_char<'src>(&mut self, batch: Option<&BatchView<'src>>, cursor: Option<&BatchCursor>) -> PeekedChar {
        if let Some(ch) = self.source.peek() {
            return Char(ch);
        }
        if let (Some(b), Some(cursor)) = (batch, cursor) {
            if cursor.bytes_consumed < b.text.len() {
                if let Some(ch) = b.text[cursor.bytes_consumed..].chars().next() {
                    return Char(ch);
                }
            }
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
    fn advance_char<'src>(&mut self, batch: Option<&BatchView<'src>>, cursor: Option<&mut BatchCursor>) {
        if let Some(ch) = self.source.next() {
            if ch == '\n' {
                self.line += 1;
                self.column = 1;
            } else {
                self.column += 1;
            }
            self.pos += 1;
            return;
        }
        if let (Some(b), Some(cursor)) = (batch, cursor) {
            if let Some(ch) = b.text[cursor.bytes_consumed..].chars().next() {
                if ch == '\n' {
                    self.line += 1;
                    self.column = 1;
                } else {
                    self.column += 1;
                }
                self.pos += 1;
                cursor.chars_consumed += 1;
                cursor.bytes_consumed += ch.len_utf8();
                return;
            }
        }
    }

    #[inline(always)]
    fn reading_from_source<'src>(&self, batch: Option<&BatchView<'src>>) -> bool {
        self.source.peek().is_some() || batch.is_none()
    }

    #[inline]
    fn copy_while_from<'src, F>(&mut self, batch: Option<&BatchView<'src>>, cursor: Option<&mut BatchCursor>, mut predicate: F) -> usize
    where
        F: FnMut(char) -> bool,
    {
        if self.source.peek().is_some() {
            return self.source.copy_while(&mut self.token_buffer, &mut predicate);
        }
        let (Some(b), Some(cursor)) = (batch, cursor) else { return 0; };
        let mut copied = 0;
        for ch in b.text[cursor.bytes_consumed..].chars() {
            if predicate(ch) {
                self.pos += 1;
                if ch == '\n' {
                    self.line += 1;
                    self.column = 1;
                } else {
                    self.column += 1;
                }
                cursor.chars_consumed += 1;
                cursor.bytes_consumed += ch.len_utf8();
                copied += 1;
            } else {
                break;
            }
        }
        copied
    }

    #[inline]
    fn copy_from_batch_while_to_owned<'src, F>(&mut self, batch: Option<&BatchView<'src>>, cursor: Option<&mut BatchCursor>, mut predicate: F) -> usize
    where
        F: FnMut(char) -> bool,
    {
        let (Some(b), Some(cursor)) = (batch, cursor) else { return 0; };
        let mut copied = 0;
        for ch in b.text[cursor.bytes_consumed..].chars() {
            if predicate(ch) {
                self.owned_batch_buffer.push(ch);
                self.pos += 1;
                if ch == '\n' {
                    self.line += 1;
                    self.column = 1;
                } else {
                    self.column += 1;
                }
                cursor.chars_consumed += 1;
                cursor.bytes_consumed += ch.len_utf8();
                copied += 1;
            } else {
                break;
            }
        }
        copied
    }

    #[inline(always)]
    fn new_token<'src>(&mut self, value: LexToken<'src>, partial: bool) -> LexToken<'src> {
        self.partial_lex = partial;
        value
    }

    #[inline]
    fn take_owned_from_buffers(&mut self) -> String {
        // Take any batch-owned suffix and, if present, prepend the ring-built
        // prefix from `self.token_buffer`.
        let mut s = core::mem::take(&mut self.owned_batch_buffer);
        if !self.token_buffer.is_empty() {
            let mut prefix = core::mem::take(&mut self.token_buffer);
            prefix.push_str(&s);
            s = prefix;
        }
        s
    }

    #[inline]
    fn take_owned_raw_from_buffers(&mut self) -> alloc::vec::Vec<u8> {
        // Merge ring-built prefix (token_buffer as UTF-8) with any batch-owned raw bytes.
        let mut raw = core::mem::take(&mut self.owned_batch_raw);
        if !self.token_buffer.is_empty() {
            let prefix = core::mem::take(&mut self.token_buffer);
            let mut merged = alloc::vec::Vec::with_capacity(prefix.len() + raw.len());
            merged.extend_from_slice(prefix.as_bytes());
            merged.extend_from_slice(&raw);
            raw = merged;
        }
        raw
    }

    #[inline]
    fn ensure_raw_mode_and_move_buffers(&mut self) {
        if self.token_is_raw_bytes { return; }
        if !self.token_buffer.is_empty() {
            self.owned_batch_raw.extend_from_slice(self.token_buffer.as_bytes());
            self.token_buffer.clear();
        }
        if !self.owned_batch_buffer.is_empty() {
            self.owned_batch_raw.extend_from_slice(self.owned_batch_buffer.as_bytes());
            self.owned_batch_buffer.clear();
        }
        self.token_is_raw_bytes = true;
    }

    #[inline(always)]
    fn produce_string<'src>(&mut self, partial: bool, batch: Option<&BatchView<'src>>) -> LexToken<'src> {
        self.partial_lex = partial;
        // Property names never emit partial fragments. If we are mid‑property
        // and the batch ended, preserve `token_start_pos` for `Drop` and
        // signal `Eof` to request more input.
        if partial && self.parse_state == ParseState::BeforePropertyName {
            return LexToken::Eof;
        }

        let start = self.token_start_pos.take().unwrap_or(self.pos);
        let end = self.pos; // inclusive of all chars read so far; callers adjust if needed
        let try_borrow = if !self.string_had_escape && !self.token_is_owned {
            self.borrow_slice(batch, start, end)
        } else {
            None
        };

        // If we're emitting a partial fragment, the next fragment (if any)
        // starts at the current position. Property names never emit partial
        // fragments, so we must not disturb `token_start_pos` for them — it is
        // used by `Drop` to preserve already-read content across feeds.
        if partial && self.parse_state != ParseState::BeforePropertyName {
            self.token_start_pos = Some(self.pos);
        }

        if self.parse_state == ParseState::BeforePropertyName {
            if let Some(s) = try_borrow { return LexToken::PropertyNameBorrowed(s); }
            if self.source.peek().is_none() {
                // Owned fragment assembled during batch-mode lexing
                LexToken::PropertyNameOwned(self.take_owned_from_buffers())
            } else {
                LexToken::PropertyNameBuffered
            }
        } else {
            if let Some(s) = try_borrow {
                LexToken::StringBorrowed(s)
            } else if self.source.peek().is_none() {
                // Owned fragment assembled during batch-mode lexing
                if self.token_is_raw_bytes {
                    LexToken::StringRawOwned(self.take_owned_raw_from_buffers())
                } else {
                    LexToken::StringOwned(self.take_owned_from_buffers())
                }
            } else {
                LexToken::StringBuffered
            }
        }
    }

    fn produce_number<'src>(&mut self, batch: Option<&BatchView<'src>>) -> LexToken<'src> {
        let start = self.token_start_pos.take().unwrap_or(self.pos);
        let end = self.pos;
        if self.token_is_owned {
            if self.source.peek().is_none() { LexToken::NumberOwned(self.take_owned_from_buffers()) } else { LexToken::NumberBuffered }
        } else if let Some(s) = self.borrow_slice(batch, start, end) {
            LexToken::NumberBorrowed(s)
        } else {
            // Can't borrow; commit to buffered/owned mode for the remainder
            self.token_is_owned = true;
            if self.source.peek().is_none() { LexToken::NumberOwned(self.take_owned_from_buffers()) } else { LexToken::NumberBuffered }
        }
    }

    fn borrow_slice<'src>(&self, batch: Option<&BatchView<'src>>, start: usize, end: usize) -> Option<&'src str> {
        let b = batch?;
        if start < b.start_pos || end > b.end_pos || end < start { return None; }
        let rel_start = start - b.start_pos;
        let rel_end = end - b.start_pos;
        Some(b.slice_chars(rel_start, rel_end))
    }

    #[expect(clippy::too_many_lines)]
    #[inline(always)]
    fn lex_state_step<'src>(
        &mut self,
        lex_state: LexState,
        next_char: PeekedChar,
        batch: Option<&BatchView<'src>>,
        mut cursor: Option<&mut BatchCursor>,
    ) -> Result<Option<LexToken<'src>>, ParserError<B>> {
        use LexState::*;
        match lex_state {
            Error => Ok(None),
            Default => match next_char {
                // Strict JSON whitespace (always allowed)
                Char(' ' | '\n' | '\r' | '\t') => {
                    self.advance_char(batch, cursor.as_deref_mut());
                    Ok(None)
                }
                // Additional Unicode whitespace (only when enabled)
                Char(c) if self.allow_unicode_whitespace && c.is_whitespace() => {
                    self.advance_char(batch, cursor.as_deref_mut());
                    Ok(None)
                }
                Empty => Ok(Some(self.new_token(LexToken::Eof, true))),
                EndOfInput => {
                    self.advance_char(batch, cursor.as_deref_mut());
                    Ok(Some(self.new_token(LexToken::Eof, false)))
                }
                    Char(_) => self.lex_state_step(self.parse_state.into(), next_char, batch, cursor.as_deref_mut()),
            }

            // -------------------------- VALUE entry --------------------------
            Value => match next_char {
                Char(c) if matches!(c, '{' | '[') => {
                    self.advance_char(batch, cursor.as_deref_mut());
                    Ok(Some(self.new_token(LexToken::Punctuator(c as u8), false)))
                }
                Char(c) if matches!(c, 'n' | 't' | 'f') => {
                    self.token_is_owned = false;
                    self.token_buffer.clear();
                    let from_source = self.reading_from_source(batch);
                    self.advance_char(batch, cursor.as_deref_mut());
                    if from_source { self.token_buffer.push(c); }
                    self.lex_state = ValueLiteral;
                    self.expected_literal = ExpectedLiteralBuffer::new(c);
                    Ok(None)
                }
                Char(c @ '-') => {
                    let from_source = self.reading_from_source(batch);
                    self.token_is_owned = from_source;
                    self.token_start_pos = Some(self.pos);
                    self.token_buffer.clear();
                    self.owned_batch_buffer.clear();
                    self.advance_char(batch, cursor.as_deref_mut());
                    if from_source { self.token_buffer.push(c); } else { self.owned_batch_buffer.push(c); }
                    self.lex_state = Sign;
                    Ok(None)
                }
                Char(c @ '0') => {
                    let from_source = self.reading_from_source(batch);
                    self.token_is_owned = from_source;
                    self.token_start_pos = Some(self.pos);
                    self.token_buffer.clear();
                    self.owned_batch_buffer.clear();
                    self.advance_char(batch, cursor.as_deref_mut());
                    if from_source { self.token_buffer.push(c); } else { self.owned_batch_buffer.push(c); }
                    self.lex_state = Zero;
                    Ok(None)
                }
                Char(c) if c.is_ascii_digit() => {
                    let from_source = self.reading_from_source(batch);
                    self.token_is_owned = from_source;
                    self.token_start_pos = Some(self.pos);
                    self.token_buffer.clear();
                    self.owned_batch_buffer.clear();
                    self.advance_char(batch, cursor.as_deref_mut());
                    if from_source { self.token_buffer.push(c); } else { self.owned_batch_buffer.push(c); }
                    self.lex_state = DecimalInteger;
                    Ok(None)
                }
                Char('"') => {
                    self.token_is_owned = self.reading_from_source(batch);
                    self.token_is_raw_bytes = false;
                    self.owned_batch_buffer.clear();
                    self.owned_batch_raw.clear();
                    self.advance_char(batch, cursor.as_deref_mut()); // consume quote
                    self.token_buffer.clear();
                    self.lex_state = LexState::String;
                    self.token_start_pos = Some(self.pos);
                    self.string_had_escape = false;
                    self.initialized_string = true;
                    Ok(None)
                }
                c => Err(self.invalid_char(c)),
            },

            // -------------------------- LITERALS -----------------------------
            ValueLiteral => match next_char {
                Empty => Ok(Some(self.new_token(LexToken::Eof, true))),
                Char(c) => match self.expected_literal.step(c) {
                    literal_buffer::Step::NeedMore => {
                        let from_source = self.reading_from_source(batch);
                        self.advance_char(batch, cursor.as_deref_mut());
                        if from_source { self.token_buffer.push(c); }
                        Ok(None)
                    }
                    literal_buffer::Step::Done(tok) => {
                        let from_source = self.reading_from_source(batch);
                        self.advance_char(batch, cursor.as_deref_mut());
                        if from_source { self.token_buffer.push(c); }
                        let lt = match tok { Token::Null => LexToken::Null, Token::Boolean(b) => LexToken::Boolean(b), _ => unreachable!() };
                        Ok(Some(self.new_token(lt, false)))
                    }
                    literal_buffer::Step::Reject => Err(self.read_and_invalid_char(Char(c))),
                },
                c @ EndOfInput => Err(self.read_and_invalid_char(c)),
            },

            // -------------------------- NUMBERS -----------------------------
            Sign => match next_char {
                Empty => { self.token_is_owned = true; Ok(Some(self.new_token(LexToken::Eof, true))) },
                Char(c @ '0') => {
                    let from_source = self.reading_from_source(batch);
                    self.advance_char(batch, cursor.as_deref_mut());
                    if from_source { self.token_buffer.push(c); } else { self.owned_batch_buffer.push(c); }
                    self.lex_state = Zero;
                    Ok(None)
                }
                Char(c) if c.is_ascii_digit() => {
                    let from_source = self.reading_from_source(batch);
                    self.advance_char(batch, cursor.as_deref_mut());
                    if from_source { self.token_buffer.push(c); } else { self.owned_batch_buffer.push(c); }
                    self.lex_state = DecimalInteger;
                    Ok(None)
                }
                c => Err(self.read_and_invalid_char(c)),
            },

            Zero => match next_char {
                Empty => { self.token_is_owned = true; Ok(Some(self.new_token(LexToken::Eof, true))) },
                Char(c @ '.') => {
                    let from_source = self.reading_from_source(batch);
                    self.advance_char(batch, cursor.as_deref_mut());
                    if from_source { self.token_buffer.push(c); } else { self.owned_batch_buffer.push(c); }
                    self.lex_state = DecimalPoint;
                    Ok(None)
                }
                Char(c) if matches!(c, 'e' | 'E') => {
                    let from_source = self.reading_from_source(batch);
                    self.advance_char(batch, cursor.as_deref_mut());
                    if from_source { self.token_buffer.push(c); } else { self.owned_batch_buffer.push(c); }
                    self.lex_state = DecimalExponent;
                    Ok(None)
                }
                _ => {
                    let tok = self.produce_number(batch);
                    Ok(Some(self.new_token(tok, false)))
                }
            },

            DecimalInteger => match next_char {
                Empty => { self.token_is_owned = true; Ok(Some(self.new_token(LexToken::Eof, true))) },
                Char(c @ '.') => {
                    let from_source = self.reading_from_source(batch);
                    self.advance_char(batch, cursor.as_deref_mut());
                    if from_source { self.token_buffer.push(c); } else { self.owned_batch_buffer.push(c); }
                    self.lex_state = DecimalPoint;
                    Ok(None)
                }
                Char(c) if matches!(c, 'e' | 'E') => {
                    let from_source = self.reading_from_source(batch);
                    self.advance_char(batch, cursor.as_deref_mut());
                    if from_source { self.token_buffer.push(c); } else { self.owned_batch_buffer.push(c); }
                    self.lex_state = DecimalExponent;
                    Ok(None)
                }
                Char(c) if c.is_ascii_digit() => {
                    let from_source = self.reading_from_source(batch);
                    self.advance_char(batch, cursor.as_deref_mut());
                    if from_source { self.token_buffer.push(c); } else { self.owned_batch_buffer.push(c); }

                    if from_source {
                        let copied = self
                            .source
                            .copy_while(&mut self.token_buffer, |d| d.is_ascii_digit());
                        self.column += copied;
                        self.pos += copied;
                    } else {
                        let _ = self.copy_from_batch_while_to_owned(batch, cursor.as_deref_mut(), |d| d.is_ascii_digit());
                    }

                    Ok(None)
                }
                _ => {
                    let tok = self.produce_number(batch);
                    Ok(Some(self.new_token(tok, false)))
                }
            },

            DecimalPoint => match next_char {
                Empty => { self.token_is_owned = true; Ok(Some(self.new_token(LexToken::Eof, true))) },
                Char(c) if matches!(c, 'e' | 'E') => {
                    let from_source = self.reading_from_source(batch);
                    self.advance_char(batch, cursor.as_deref_mut());
                    if from_source { self.token_buffer.push(c); } else { self.owned_batch_buffer.push(c); }
                    self.lex_state = DecimalExponent;
                    Ok(None)
                }
                Char(c) if c.is_ascii_digit() => {
                    let from_source = self.reading_from_source(batch);
                    self.advance_char(batch, cursor.as_deref_mut());
                    if from_source { self.token_buffer.push(c); } else { self.owned_batch_buffer.push(c); }
                    self.lex_state = DecimalFraction;

                    if from_source {
                        let copied = self
                            .source
                            .copy_while(&mut self.token_buffer, |d| d.is_ascii_digit());
                        self.column += copied;
                        self.pos += copied;
                    } else {
                        let _ = self.copy_from_batch_while_to_owned(batch, cursor.as_deref_mut(), |d| d.is_ascii_digit());
                    }

                    Ok(None)
                }
                c => Err(self.read_and_invalid_char(c)),
            },

            DecimalFraction => match next_char {
                Empty => { self.token_is_owned = true; Ok(Some(self.new_token(LexToken::Eof, true))) },
                Char(c) if matches!(c, 'e' | 'E') => {
                    let from_source = self.reading_from_source(batch);
                    self.advance_char(batch, cursor.as_deref_mut());
                    if from_source { self.token_buffer.push(c); } else { self.owned_batch_buffer.push(c); }
                    self.lex_state = DecimalExponent;
                    Ok(None)
                }
                Char(c) if c.is_ascii_digit() => {
                    let from_source = self.reading_from_source(batch);
                    self.advance_char(batch, cursor.as_deref_mut());
                    if from_source { self.token_buffer.push(c); } else { self.owned_batch_buffer.push(c); }

                    if from_source {
                        let copied = self
                            .source
                            .copy_while(&mut self.token_buffer, |d| d.is_ascii_digit());
                        self.column += copied;
                        self.pos += copied;
                    } else {
                        let _ = self.copy_from_batch_while_to_owned(batch, cursor.as_deref_mut(), |d| d.is_ascii_digit());
                    }

                    Ok(None)
                }
                _ => {
                    let tok = self.produce_number(batch);
                    Ok(Some(self.new_token(tok, false)))
                }
            },

            DecimalExponent => match next_char {
                Empty => { self.token_is_owned = true; Ok(Some(self.new_token(LexToken::Eof, true))) },
                Char(c) if matches!(c, '+' | '-') => {
                    self.advance_char(batch, cursor.as_deref_mut());
                    self.token_buffer.push(c);
                    self.lex_state = DecimalExponentSign;
                    Ok(None)
                }
                Char(c) if c.is_ascii_digit() => {
                    self.advance_char(batch, cursor.as_deref_mut());
                    self.token_buffer.push(c);
                    self.lex_state = DecimalExponentInteger;

                    let copied = self
                        .source
                        .copy_while(&mut self.token_buffer, |d| d.is_ascii_digit());

                    self.column += copied;
                    self.pos += copied;

                    Ok(None)
                }
                c => Err(self.read_and_invalid_char(c)),
            },

            DecimalExponentSign => match next_char {
                Empty => { self.token_is_owned = true; Ok(Some(self.new_token(LexToken::Eof, true))) },
                Char(c) if c.is_ascii_digit() => {
                    self.advance_char(batch, cursor.as_deref_mut());
                    self.token_buffer.push(c);
                    self.lex_state = DecimalExponentInteger;

                    let copied = self
                        .source
                        .copy_while(&mut self.token_buffer, |d| d.is_ascii_digit());

                    self.column += copied;
                    self.pos += copied;

                    Ok(None)
                }
                c => Err(self.read_and_invalid_char(c)),
            },

            DecimalExponentInteger => match next_char {
                Empty => { self.token_is_owned = true; Ok(Some(self.new_token(LexToken::Eof, true))) },
                Char(c) if c.is_ascii_digit() => {
                    let from_source = self.reading_from_source(batch);
                    self.advance_char(batch, cursor.as_deref_mut());
                    if from_source { self.token_buffer.push(c); } else { self.owned_batch_buffer.push(c); }

                    if from_source {
                        let copied = self
                            .source
                            .copy_while(&mut self.token_buffer, |d| d.is_ascii_digit());
                        self.column += copied;
                        self.pos += copied;
                    } else {
                        let _ = self.copy_from_batch_while_to_owned(batch, cursor.as_deref_mut(), |d| d.is_ascii_digit());
                    }

                    Ok(None)
                }
                _ => {
                    let tok = self.produce_number(batch);
                    Ok(Some(self.new_token(tok, false)))
                }
            },

            // -------------------------- STRING -----------------------------
            LexState::String => match next_char {
                // escape sequence
                Char('\\') => {
                    // For property names, we don't emit fragments; for values, emit the
                    // current fragment before switching to escape handling.
                    if self.parse_state == ParseState::BeforePropertyName {
                        // For property names, buffer the already-read portion
                        if let Some(b) = batch {
                            if let Some(start) = self.token_start_pos {
                                let start_c = start.saturating_sub(b.start_pos).clamp(0, b.end_pos - b.start_pos);
                                let end_c = cursor.as_deref().map(|c| c.chars_consumed).unwrap_or(0).min(b.end_pos - b.start_pos);
                                if end_c > start_c {
                                    let s = b.slice_chars(start_c, end_c);
                                    if self.reading_from_source(batch) {
                                        self.token_buffer.push_str(s);
                                    } else {
                                        self.owned_batch_buffer.push_str(s);
                                    }
                                }
                            }
                        }
                        self.token_is_owned = true;
                        self.advance_char(batch, cursor.as_deref_mut());
                        self.string_had_escape = true;
                        self.lex_state = LexState::StringEscape;
                        Ok(None)
                    } else {
                        // Commit to buffered mode for the remainder of this string value.
                        // Preload owned fragment with content up to the backslash, but only
                        // emit a partial fragment if there was any prefix (avoid zero-length).
                        let mut had_prefix = false;
                        if let Some(b) = batch {
                            if let Some(start) = self.token_start_pos {
                                let start_c = start.saturating_sub(b.start_pos).clamp(0, b.end_pos - b.start_pos);
                                let end_c = cursor.as_deref().map(|c| c.chars_consumed).unwrap_or(0).min(b.end_pos - b.start_pos);
                                if end_c > start_c {
                                    let s = b.slice_chars(start_c, end_c);
                                    self.owned_batch_buffer.push_str(s);
                                    had_prefix = true;
                                }
                            }
                        }
                        self.token_is_owned = true;
                        self.string_had_escape = true;
                        // Now consume the backslash and transition to escape state
                        self.advance_char(batch, cursor.as_deref_mut());
                        self.lex_state = LexState::StringEscape;
                        if had_prefix {
                            // Emit the fragment accumulated so far (partial)
                            let tok = self.produce_string(true, batch);
                            Ok(Some(self.new_token(tok, true)))
                        } else {
                            Ok(None)
                        }
                    }
                }
                // closing quote -> complete string
                Char('"') => {
                    // If a high surrogate is pending at string termination, resolve per decode_mode.
                    if let Some(high) = self.pending_high_surrogate.take() {
                        match self.decode_mode {
                            options::DecodeMode::StrictUnicode => {
                                return Err(self.syntax_error(SyntaxError::InvalidUnicodeEscapeSequence(high as u32)));
                            }
                            options::DecodeMode::ReplaceInvalid => {
                                if self.reading_from_source(batch) { self.token_buffer.push('\u{FFFD}'); } else { self.owned_batch_buffer.push('\u{FFFD}'); }
                            }
                            options::DecodeMode::SurrogatePreserving => {
                                if self.parse_state == ParseState::BeforePropertyName {
                                    // Property names remain UTF-8: degrade to replacement.
                                    if self.reading_from_source(batch) { self.token_buffer.push('\u{FFFD}'); } else { self.owned_batch_buffer.push('\u{FFFD}'); }
                                } else {
                                    self.ensure_raw_mode_and_move_buffers();
                                    let u = high;
                                    let b1 = 0xE0 | ((u as u32 >> 12) & 0x0F) as u8;
                                    let b2 = 0x80 | ((u as u32 >> 6) & 0x3F) as u8;
                                    let b3 = 0x80 | (u as u32 & 0x3F) as u8;
                                    self.owned_batch_raw.extend_from_slice(&[b1, b2, b3]);
                                }
                            }
                        }
                    }
                    self.advance_char(batch, cursor.as_deref_mut());
                    // Exclude the closing quote – temporarily move pos back
                    let end_pos = self.pos.saturating_sub(1);
                    let saved_pos = self.pos;
                    self.pos = end_pos;
                    let tok = self.produce_string(false, batch);
                    self.pos = saved_pos;
                    Ok(Some(tok))
                }
                Char(c @ '\0'..='\x1F') => {
                    // JSON spec allows 0x20 .. 0x10FFFF unescaped.
                    Err(self.read_and_invalid_char(Char(c)))
                }
                Empty => Ok(Some(self.produce_string(true, batch))),
                Char(_c) => {
                    // Fast-path: copy as many consecutive non-escaped, non-terminating
                    // characters as possible in a single pass.
                    if self.reading_from_source(batch) {
                        if self.token_is_raw_bytes {
                            // Manually drain char-by-char to bytes
                            loop {
                                if let Some(ch) = self.source.peek() {
                                    if ch != '\\' && ch != '"' && ch >= '\u{20}' {
                                        let _ = self.source.next();
                                        let mut tmp = [0u8; 4];
                                        let s = ch.encode_utf8(&mut tmp);
                                        self.owned_batch_raw.extend_from_slice(s.as_bytes());
                                        self.pos += 1;
                                        if ch == '\n' { self.line += 1; self.column = 1; } else { self.column += 1; }
                                    } else { break; }
                                } else { break; }
                            }
                        } else {
                            let copied = self.source.copy_while(&mut self.token_buffer, |ch| {
                                ch != '\\' && ch != '"' && ch >= '\u{20}'
                            });
                            self.column += copied;
                            self.pos += copied;
                        }
                    } else {
                        if self.token_is_owned {
                            if self.token_is_raw_bytes {
                                // Append UTF-8 bytes of chars
                                let (Some(b), Some(cur)) = (batch, cursor.as_deref_mut()) else { return Ok(None) };
                                let mut _local = 0;
                                for ch in b.text[cur.bytes_consumed..].chars() {
                                    if ch != '\\' && ch != '"' && ch >= '\u{20}' {
                                        let mut tmp = [0u8; 4];
                                        let s = ch.encode_utf8(&mut tmp);
                                        self.owned_batch_raw.extend_from_slice(s.as_bytes());
                                        self.pos += 1;
                                        if ch == '\n' { self.line += 1; self.column = 1; } else { self.column += 1; }
                                        cur.chars_consumed += 1;
                                        cur.bytes_consumed += ch.len_utf8();
                                        _local += 1;
                                    } else { break; }
                                }
                            } else {
                                let _ = self.copy_from_batch_while_to_owned(batch, cursor.as_deref_mut(), |ch| {
                                    ch != '\\' && ch != '"' && ch >= '\u{20}'
                                });
                            }
                        } else {
                            let _ = self.copy_while_from(batch, cursor.as_deref_mut(), |ch| {
                                ch != '\\' && ch != '"' && ch >= '\u{20}'
                            });
                        }
                    }

                    Ok(None)
                }
                EndOfInput => Err(self.read_and_invalid_char(EndOfInput)),
            },

            StringEscape => match next_char {
                Empty => Ok(Some(self.produce_string(true, batch))),
                Char(ch) if matches!(ch, '"' | '\\' | '/') => {
                    let from_source = self.reading_from_source(batch);
                    self.advance_char(batch, cursor.as_deref_mut());
                    if self.token_is_raw_bytes {
                        let mut tmp = [0u8; 4];
                        let s = ch.encode_utf8(&mut tmp);
                        self.owned_batch_raw.extend_from_slice(s.as_bytes());
                    } else if from_source { self.token_buffer.push(ch); } else { self.owned_batch_buffer.push(ch); }
                    self.lex_state = LexState::String;
                    // After consuming an escape, mark the start for future preloads
                    self.token_start_pos = Some(self.pos);
                    Ok(None)
                }
                Char('b') => {
                    let from_source = self.reading_from_source(batch);
                    self.advance_char(batch, cursor.as_deref_mut());
                    let ch = '\u{0008}';
                    if self.token_is_raw_bytes { let mut tmp=[0u8;4]; let s=ch.encode_utf8(&mut tmp); self.owned_batch_raw.extend_from_slice(s.as_bytes()); } else if from_source { self.token_buffer.push(ch); } else { self.owned_batch_buffer.push(ch); }
                    self.lex_state = LexState::String;
                    self.token_start_pos = Some(self.pos);
                    Ok(None)
                }
                Char('f') => {
                    let from_source = self.reading_from_source(batch);
                    self.advance_char(batch, cursor.as_deref_mut());
                    let ch = '\u{000C}';
                    if self.token_is_raw_bytes { let mut tmp=[0u8;4]; let s=ch.encode_utf8(&mut tmp); self.owned_batch_raw.extend_from_slice(s.as_bytes()); } else if from_source { self.token_buffer.push(ch); } else { self.owned_batch_buffer.push(ch); }
                    self.lex_state = LexState::String;
                    self.token_start_pos = Some(self.pos);
                    Ok(None)
                }
                Char('n') => {
                    let from_source = self.reading_from_source(batch);
                    self.advance_char(batch, cursor.as_deref_mut());
                    let ch = '\n';
                    if self.token_is_raw_bytes { let mut tmp=[0u8;4]; let s=ch.encode_utf8(&mut tmp); self.owned_batch_raw.extend_from_slice(s.as_bytes()); } else if from_source { self.token_buffer.push(ch); } else { self.owned_batch_buffer.push(ch); }
                    self.lex_state = LexState::String;
                    self.token_start_pos = Some(self.pos);
                    Ok(None)
                }
                Char('r') => {
                    let from_source = self.reading_from_source(batch);
                    self.advance_char(batch, cursor.as_deref_mut());
                    let ch = '\r';
                    if self.token_is_raw_bytes { let mut tmp=[0u8;4]; let s=ch.encode_utf8(&mut tmp); self.owned_batch_raw.extend_from_slice(s.as_bytes()); } else if from_source { self.token_buffer.push(ch); } else { self.owned_batch_buffer.push(ch); }
                    self.lex_state = LexState::String;
                    self.token_start_pos = Some(self.pos);
                    Ok(None)
                }
                Char('t') => {
                    let from_source = self.reading_from_source(batch);
                    self.advance_char(batch, cursor.as_deref_mut());
                    let ch = '\t';
                    if self.token_is_raw_bytes { let mut tmp=[0u8;4]; let s=ch.encode_utf8(&mut tmp); self.owned_batch_raw.extend_from_slice(s.as_bytes()); } else if from_source { self.token_buffer.push(ch); } else { self.owned_batch_buffer.push(ch); }
                    self.lex_state = LexState::String;
                    self.token_start_pos = Some(self.pos);
                    Ok(None)
                }
                Char(c) if c == 'u' || (c == 'U' && self.allow_uppercase_u) => {
                    self.advance_char(batch, cursor.as_deref_mut());
                    self.unicode_escape_buffer.reset();
                    self.lex_state = LexState::StringEscapeUnicode;
                    Ok(None)
                }
                c => Err(self.read_and_invalid_char(c)),
            },

            StringEscapeUnicode => {
                match next_char {
                    Empty => Ok(Some(self.produce_string(true, batch))),
                    Char(c) if c.is_ascii_hexdigit() => {
                        self.advance_char(batch, cursor.as_deref_mut());
                        match self.unicode_escape_buffer.feed(c) {
                            Ok(Some(char)) => {
                                // Finished a \uXXXX sequence to a scalar. If a high surrogate was pending,
                                // we must handle it first according to decode_mode.
                                if let Some(high) = self.pending_high_surrogate.take() {
                                    match self.decode_mode {
                                        options::DecodeMode::StrictUnicode => {
                                            return Err(self.syntax_error(SyntaxError::InvalidUnicodeEscapeSequence(high as u32)));
                                        }
                                        options::DecodeMode::SurrogatePreserving => {
                                            self.ensure_raw_mode_and_move_buffers();
                                            let u = high;
                                            let b1 = 0xE0 | ((u as u32 >> 12) & 0x0F) as u8;
                                            let b2 = 0x80 | ((u as u32 >> 6) & 0x3F) as u8;
                                            let b3 = 0x80 | (u as u32 & 0x3F) as u8;
                                            self.owned_batch_raw.extend_from_slice(&[b1, b2, b3]);
                                        }
                                        options::DecodeMode::ReplaceInvalid => {
                                            if self.reading_from_source(batch) { self.token_buffer.push('\u{FFFD}'); } else { self.owned_batch_buffer.push('\u{FFFD}'); }
                                        }
                                    }
                                }
                                // Now append the decoded scalar of this escape
                                if self.token_is_raw_bytes { let mut tmp=[0u8;4]; let s=char.encode_utf8(&mut tmp); self.owned_batch_raw.extend_from_slice(s.as_bytes()); }
                                else if self.reading_from_source(batch) { self.token_buffer.push(char); } else { self.owned_batch_buffer.push(char); }
                                self.lex_state = LexState::String;
                                self.token_start_pos = Some(self.pos);
                                Ok(None)
                            }
                            Ok(None) => {
                                // Still waiting for more hex digits
                                Ok(None)
                            }
                            Err(err) => {
                                // Handle surrogate pairs via error path carrying raw code.
                                match err {
                                    SyntaxError::InvalidUnicodeEscapeSequence(code) => {
                                        // High surrogate range: D800..DBFF
                                        if (0xD800..=0xDBFF).contains(&code) {
                                            if let Some(prev_high) = self.pending_high_surrogate.take() {
                                                match self.decode_mode {
                                                    options::DecodeMode::StrictUnicode => {
                                                        Err(self.syntax_error(SyntaxError::InvalidUnicodeEscapeSequence(prev_high as u32)))
                                                    }
                                                    options::DecodeMode::SurrogatePreserving => {
                                                        self.ensure_raw_mode_and_move_buffers();
                                                        let u = prev_high;
                                                        let b1 = 0xE0 | ((u as u32 >> 12) & 0x0F) as u8;
                                                        let b2 = 0x80 | ((u as u32 >> 6) & 0x3F) as u8;
                                                        let b3 = 0x80 | (u as u32 & 0x3F) as u8;
                                                        self.owned_batch_raw.extend_from_slice(&[b1, b2, b3]);
                                                        // Start a new pending high with the current code
                                                        self.pending_high_surrogate = Some(code as u16);
                                                        self.lex_state = LexState::StringEscapeUnicodeExpectBackslash;
                                                        Ok(None)
                                                    }
                                                    options::DecodeMode::ReplaceInvalid => {
                                                        if self.reading_from_source(batch) { self.token_buffer.push('\u{FFFD}'); } else { self.owned_batch_buffer.push('\u{FFFD}'); }
                                                        self.pending_high_surrogate = Some(code as u16);
                                                        self.lex_state = LexState::StringEscapeUnicodeExpectBackslash;
                                                        Ok(None)
                                                    }
                                                }
                                            } else {
                                                self.pending_high_surrogate = Some(code as u16);
                                                // Expect a backslash starting the second \uXXXX
                                                self.lex_state = LexState::StringEscapeUnicodeExpectBackslash;
                                                Ok(None)
                                            }
                                        } else if (0xDC00..=0xDFFF).contains(&code) {
                                            // Low surrogate encountered. Must have a pending high surrogate.
                                            if let Some(high) = self.pending_high_surrogate.take() {
                                                let low = code as u16;
                                                // Combine surrogate pair to code point
                                                let combined = 0x10000
                                                    + (((high as u32 - 0xD800) << 10)
                                                        | (low as u32 - 0xDC00));
                                                if let Some(ch) = core::char::from_u32(combined) {
                                                    if self.token_is_raw_bytes { let mut tmp=[0u8;4]; let s=ch.encode_utf8(&mut tmp); self.owned_batch_raw.extend_from_slice(s.as_bytes()); }
                                                    else if self.reading_from_source(batch) { self.token_buffer.push(ch); } else { self.owned_batch_buffer.push(ch); }
                                                    self.lex_state = LexState::String;
                                                    Ok(None)
                                                } else {
                                                    Err(self.syntax_error(SyntaxError::InvalidUnicodeEscapeSequence(combined)))
                                                }
                                            } else {
                                                // Low surrogate without preceding high
                                                match self.decode_mode {
                                                    options::DecodeMode::StrictUnicode => Err(self.syntax_error(SyntaxError::InvalidUnicodeEscapeSequence(code))),
                                                    options::DecodeMode::SurrogatePreserving => {
                                                        self.ensure_raw_mode_and_move_buffers();
                                                        let u = code as u16;
                                                        let b1 = 0xE0 | ((u as u32 >> 12) & 0x0F) as u8;
                                                        let b2 = 0x80 | ((u as u32 >> 6) & 0x3F) as u8;
                                                        let b3 = 0x80 | (u as u32 & 0x3F) as u8;
                                                        self.owned_batch_raw.extend_from_slice(&[b1, b2, b3]);
                                                        self.lex_state = LexState::String;
                                                        Ok(None)
                                                    }
                                                    options::DecodeMode::ReplaceInvalid => {
                                                        if self.reading_from_source(batch) { self.token_buffer.push('\u{FFFD}'); } else { self.owned_batch_buffer.push('\u{FFFD}'); }
                                                        self.lex_state = LexState::String;
                                                        Ok(None)
                                                    }
                                                }
                                            }
                                        } else {
                                            Err(self.syntax_error(SyntaxError::InvalidUnicodeEscapeSequence(code)))
                                        }
                                    }
                                    other => Err(self.syntax_error(other)),
                                }
                            },
                        }
                    }
                    EndOfInput => {
                        // consume EOF sentinel and advance column to match TS behavior
                        self.advance_char(batch, cursor.as_deref_mut());
                        self.column += 1;
                        Err(self.invalid_eof())
                    }
                    c @ Char(_) => Err(self.read_and_invalid_char(c)),
                }
            }

            // Expect a backslash starting the second half of the surrogate pair
            StringEscapeUnicodeExpectBackslash => match next_char {
                Empty => Ok(Some(self.produce_string(true, batch))),
                Char('\\') => {
                    self.advance_char(batch, cursor.as_deref_mut());
                    self.lex_state = LexState::StringEscapeUnicodeExpectU;
                    Ok(None)
                }
                EndOfInput => Err(self.read_and_invalid_char(EndOfInput)),
                c => {
                    match self.decode_mode {
                        options::DecodeMode::StrictUnicode => Err(self.read_and_invalid_char(c)),
                        options::DecodeMode::SurrogatePreserving => {
                            if let Some(high) = self.pending_high_surrogate.take() {
                                self.ensure_raw_mode_and_move_buffers();
                                let u = high;
                                let b1 = 0xE0 | ((u as u32 >> 12) & 0x0F) as u8;
                                let b2 = 0x80 | ((u as u32 >> 6) & 0x3F) as u8;
                                let b3 = 0x80 | (u as u32 & 0x3F) as u8;
                                self.owned_batch_raw.extend_from_slice(&[b1, b2, b3]);
                            }
                            self.lex_state = LexState::String;
                            Ok(None)
                        }
                        options::DecodeMode::ReplaceInvalid => {
                            if let Some(_) = self.pending_high_surrogate.take() {
                                if self.reading_from_source(batch) { self.token_buffer.push('\u{FFFD}'); } else { self.owned_batch_buffer.push('\u{FFFD}'); }
                            }
                            self.lex_state = LexState::String;
                            Ok(None)
                        }
                    }
                },
            },

            // Expect the 'u' introducing the low surrogate digits
            StringEscapeUnicodeExpectU => match next_char {
                Empty => Ok(Some(self.produce_string(true, batch))),
                Char(c) if c == 'u' || (c == 'U' && self.allow_uppercase_u) => {
                    self.advance_char(batch, cursor.as_deref_mut());
                    self.unicode_escape_buffer.reset();
                    self.lex_state = LexState::StringEscapeUnicode;
                    Ok(None)
                }
                EndOfInput => Err(self.read_and_invalid_char(EndOfInput)),
                c => {
                    match self.decode_mode {
                        options::DecodeMode::StrictUnicode => Err(self.read_and_invalid_char(c)),
                        options::DecodeMode::SurrogatePreserving => {
                            if let Some(high) = self.pending_high_surrogate.take() {
                                self.ensure_raw_mode_and_move_buffers();
                                let u = high;
                                let b1 = 0xE0 | ((u as u32 >> 12) & 0x0F) as u8;
                                let b2 = 0x80 | ((u as u32 >> 6) & 0x3F) as u8;
                                let b3 = 0x80 | (u as u32 & 0x3F) as u8;
                                self.owned_batch_raw.extend_from_slice(&[b1, b2, b3]);
                            }
                            self.lex_state = LexState::String;
                            Ok(None)
                        }
                        options::DecodeMode::ReplaceInvalid => {
                            if let Some(_) = self.pending_high_surrogate.take() {
                                if self.reading_from_source(batch) { self.token_buffer.push('\u{FFFD}'); } else { self.owned_batch_buffer.push('\u{FFFD}'); }
                            }
                            self.lex_state = LexState::String;
                            Ok(None)
                        }
                    }
                },
            },

            Start => match next_char {
                Char(c) if matches!(c, '{' | '[') => {
                    self.advance_char(batch, cursor.as_deref_mut());
                    Ok(Some(self.new_token(LexToken::Punctuator(c as u8), false)))
                }
                _ => {
                    self.lex_state = LexState::Value;
                    Ok(None)
                }
            },

            BeforePropertyName => match next_char {
                Char('}') => {
                    self.advance_char(batch, cursor.as_deref_mut());
                    Ok(Some(self.new_token(LexToken::Punctuator(b'}'), false)))
                }

                Char('"') => {
                    self.advance_char(batch, cursor.as_deref_mut());
                    self.token_buffer.clear();
                    self.lex_state = LexState::String;
                    // Track start of the property name content
                    self.token_start_pos = Some(self.pos);
                    self.string_had_escape = false;
                    Ok(None)
                }
                c => Err(self.read_and_invalid_char(c)),
            },

            AfterPropertyName => match next_char {
                Char(c @ ':') => {
                    self.advance_char(batch, cursor.as_deref_mut());
                    Ok(Some(self.new_token(LexToken::Punctuator(c as u8), false)))
                }
                c => Err(self.read_and_invalid_char(c)),
            },

            BeforePropertyValue => {
                self.lex_state = LexState::Value;
                Ok(None)
            }

            AfterPropertyValue => match next_char {
                Char(c) if matches!(c, ',' | '}') => {
                    self.advance_char(batch, cursor.as_deref_mut());
                    Ok(Some(self.new_token(LexToken::Punctuator(c as u8), false)))
                }
                c => Err(self.read_and_invalid_char(c)),
            },

            BeforeArrayValue => match next_char {
                Char(']') => {
                    self.advance_char(batch, cursor.as_deref_mut());
                    Ok(Some(self.new_token(LexToken::Punctuator(b']'), false)))
                }
                _ => {
                    self.lex_state = LexState::Value;
                    Ok(None)
                }
            },

            AfterArrayValue => match next_char {
                Char(c) if matches!(c, ',' | ']') => {
                    self.advance_char(batch, cursor.as_deref_mut());
                    Ok(Some(self.new_token(LexToken::Punctuator(c as u8), false)))
                }
                c => Err(self.read_and_invalid_char(c)),
            },

            End => {
                let c = self.peek_char(batch, cursor.as_deref());
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
        token: LexToken<'src>,
        ctx: &'cx mut B,
        path: &mut B::Thawed,
    ) -> Result<Option<ParseEvent<'src, B>>, ParserError<B>> {
        use ParseState::*;

        match self.parse_state {
            // In single-value mode, EOF at start when end_of_input indicates unexpected end.
            Start => match token {
                LexToken::Eof if self.end_of_input && !self.multiple_values => Err(self.invalid_eof()),
                LexToken::Eof => Ok(None),
                _ => self.push(token, ctx, path),
            },

            BeforePropertyName => match token {
                LexToken::Eof if self.end_of_input => Err(self.invalid_eof()),
                LexToken::PropertyNameBorrowed(value) => {
                    if !self.pending_key {
                        ctx.pop_kind(path);
                    }
                    ctx.push_key_from_str(path, value);
                    self.pending_key = false;
                    self.parse_state = AfterPropertyName;
                    Ok(None)
                }
                LexToken::PropertyNameBuffered => {
                    if !self.pending_key {
                        ctx.pop_kind(path);
                    }
                    let value = core::mem::take(&mut self.token_buffer);
                    ctx.push_key_from_str(path, &value);
                    self.pending_key = false;
                    self.parse_state = AfterPropertyName;
                    Ok(None)
                }
                LexToken::PropertyNameOwned(value) => {
                    if !self.pending_key {
                        ctx.pop_kind(path);
                    }
                    ctx.push_key_from_str(path, &value);
                    self.pending_key = false;
                    self.parse_state = AfterPropertyName;
                    Ok(None)
                }
                LexToken::Punctuator(_) => Ok(self.pop(ctx, path)),
                _ => Ok(None),
            },

            AfterPropertyName => match token {
                LexToken::Eof if self.end_of_input => Err(self.invalid_eof()),
                LexToken::Eof => Ok(None),
                _ => {
                    self.parse_state = BeforePropertyValue;

                    Ok(None)
                }
            },

            BeforePropertyValue => match token {
                LexToken::Eof => Ok(None),
                _ => self.push(token, ctx, path),
            },

            BeforeArrayValue => match token {
                LexToken::Eof => Ok(None),
                LexToken::Punctuator(b']') => Ok(self.pop(ctx, path)),
                _ => self.push(token, ctx, path),
            },

            AfterPropertyValue => match token {
                LexToken::Eof if self.end_of_input => Err(self.invalid_eof()),
                LexToken::Punctuator(b',') => {
                    self.parse_state = BeforePropertyName;
                    Ok(None)
                }
                LexToken::Punctuator(b'}') => Ok(self.pop(ctx, path)),
                _ => Ok(None),
            },

            AfterArrayValue => match token {
                LexToken::Eof if self.end_of_input => Err(self.invalid_eof()),
                LexToken::Punctuator(b',') => {
                    match ctx.bump_last_index(path) {
                        Ok(path) => path,
                        Err(_) => {
                            unreachable!(); // TODO
                        }
                    }

                    self.parse_state = BeforeArrayValue;
                    Ok(None)
                }
                LexToken::Punctuator(b']') => Ok(self.pop(ctx, path)),
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
        // std::std::eprintln!(
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
        token: LexToken<'src>,
        f: &'cx mut B,
        path: &mut B::Thawed,
    ) -> Result<Option<ParseEvent<'src, B>>, ParserError<B>> {
        let evt: Option<ParseEvent<'_, B>> = match token {
            LexToken::Punctuator(b'{') => {
                self.pending_key = true;
                self.parse_state = ParseState::BeforePropertyName;
                return Ok(Some(ParseEvent::ObjectBegin { path: path.clone() }));
            }
            LexToken::Punctuator(b'[') => {
                let output_path = path.clone();
                f.push_index_zero(path);
                self.parse_state = ParseState::BeforeArrayValue;
                return Ok(Some(ParseEvent::ArrayBegin { path: output_path }));
            }

            LexToken::Null => Some(ParseEvent::Null { path: path.clone() }),
            LexToken::Boolean(b) => {
                let value = f.new_bool(b).map_err(|e| self.event_context_error(e))?;
                Some(ParseEvent::Boolean {
                    path: path.clone(),
                    value,
                })
            }
            LexToken::NumberBorrowed(n) => {
                let value = f.new_number(n).map_err(|e| self.event_context_error(e))?;
                Some(ParseEvent::Number {
                    path: path.clone(),
                    value,
                })
            }
            LexToken::NumberBuffered => {
                let n = core::mem::take(&mut self.token_buffer);
                let value = f
                    .new_number_owned(n)
                    .map_err(|e| self.event_context_error(e))?;
                Some(ParseEvent::Number {
                    path: path.clone(),
                    value,
                })
            }
            LexToken::NumberOwned(s) => {
                let value = f
                    .new_number_owned(s)
                    .map_err(|e| self.event_context_error(e))?;
                Some(ParseEvent::Number {
                    path: path.clone(),
                    value,
                })
            }
            LexToken::StringBorrowed(fragment) => {
                let fragment = f.new_str(fragment).map_err(|e| self.event_context_error(e))?;
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
            LexToken::StringBuffered => {
                let s = core::mem::take(&mut self.token_buffer);
                let fragment = f
                    .new_str_owned(s)
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
            LexToken::StringOwned(s) => {
                let fragment = f
                    .new_str_owned(s)
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
            LexToken::StringRawOwned(bytes) => {
                let hint = match self.decode_mode {
                    options::DecodeMode::StrictUnicode => RawStrHint::StrictUnicode,
                    options::DecodeMode::SurrogatePreserving => RawStrHint::SurrogatePreserving,
                    options::DecodeMode::ReplaceInvalid => RawStrHint::ReplaceInvalid,
                };
                let fragment = f
                    .new_str_raw_owned(bytes, hint)
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
            // Property names are consumed into the path and never emitted as events
            LexToken::PropertyNameBorrowed(_) | LexToken::PropertyNameBuffered | LexToken::PropertyNameOwned(_) => {
                unreachable!();
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
#[allow(clippy::float_cmp)]
mod tests {
    use alloc::{vec, vec::Vec};

    use super::*;
    use alloc::borrow::Cow;
    use crate::parser::options::DecodeMode;

    // #[test]
    // fn parser_compiles() {
    //     // Smoke test: ensure types are sized and constructible
    //     let _ = DefaultStreamingParser::new(ParserOptions::default());
    //     let _ = ClosedStreamingParser {
    //         parser: DefaultStreamingParser::new(ParserOptions::default()),
    //         builder: RustContext,
    //     };
    // }

    #[test]
    fn parser_basic_example() {
        let mut parser = DefaultStreamingParser::new(ParserOptions {
            panic_on_error: true,
            ..Default::default()
        });
        let mut events: Vec<_> = vec![];
        events.extend(parser.feed(
            "[\"hello\", {\"\": \"world\"}, 0, 1, 1.2,
true, false, null]",
        ));
        events.extend(parser.finish());

        let Ok(ParseEvent::String { ref fragment, .. }) = events[1] else {
            panic!("Expected string event");
        };
        let alloc::borrow::Cow::Borrowed(_) = fragment else {
            panic!("Expected borrowed fragment");
        };

        assert_eq!(
            events,
            vec![
                Ok(ParseEvent::ArrayBegin { path: vec![] }),
                Ok(ParseEvent::String {
                    path: vec![PathItem::Index(0)],
                    fragment: "hello".into(),
                    is_initial: true,
                    is_final: true,
                }),
                Ok(ParseEvent::ObjectBegin {
                    path: vec![PathItem::Index(1)]
                }),
                Ok(ParseEvent::String {
                    path: vec![PathItem::Index(1), PathItem::Key("".into())],
                    fragment: "world".into(),
                    is_initial: true,
                    is_final: true,
                }),
                Ok(ParseEvent::ObjectEnd {
                    path: vec![PathItem::Index(1)]
                }),
                Ok(ParseEvent::Number {
                    path: vec![PathItem::Index(2)],
                    value: 0.0,
                }),
                Ok(ParseEvent::Number {
                    path: vec![PathItem::Index(3)],
                    value: 1.0,
                }),
                Ok(ParseEvent::Number {
                    path: vec![PathItem::Index(4)],
                    value: 1.2,
                }),
                Ok(ParseEvent::Boolean {
                    path: vec![PathItem::Index(5)],
                    value: true,
                }),
                Ok(ParseEvent::Boolean {
                    path: vec![PathItem::Index(6)],
                    value: false,
                }),
                Ok(ParseEvent::Null {
                    path: vec![PathItem::Index(7)],
                }),
                Ok(ParseEvent::ArrayEnd { path: vec![] }),
            ]
        );
    }

    #[test]
    fn string_borrow_no_escape_single_chunk() {
        let mut parser = DefaultStreamingParser::new(ParserOptions { panic_on_error: true, ..Default::default() });
        let mut it = parser.feed("[\"hello\"]");
        // Expect ArrayBegin
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayBegin { .. }));
        // Expect borrowed string
        match it.next().unwrap().unwrap() {
            ParseEvent::String { fragment, is_initial, is_final, .. } => {
                assert_eq!(fragment, Cow::<str>::Borrowed("hello"));
                assert!(is_initial);
                assert!(is_final);
            }
            other => panic!("unexpected event: {other:?}"),
        }
        // Expect ArrayEnd
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayEnd { .. }));
        assert!(it.next().is_none());
    }

    #[test]
    fn string_escape_splits_and_forces_buffer() {
        let mut parser = DefaultStreamingParser::new(ParserOptions { panic_on_error: true, ..Default::default() });
        let mut it = parser.feed("[\"ab\\ncd\"]");
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayBegin { .. }));

        // First fragment before escape: should be owned (buffered) and not final
        match it.next().unwrap().unwrap() {
            ParseEvent::String { fragment, is_initial, is_final, .. } => {
                assert_eq!(fragment, Cow::<str>::Owned(String::from("ab")));
                assert!(is_initial);
                assert!(!is_final);
            }
            other => panic!("unexpected event: {other:?}"),
        }

        // Second fragment after escape to end: should include decoded '\n' and be owned
        match it.next().unwrap().unwrap() {
            ParseEvent::String { fragment, is_initial, is_final, .. } => {
                assert_eq!(fragment, Cow::<str>::Owned(String::from("\ncd")));
                assert!(!is_initial);
                assert!(is_final);
            }
            other => panic!("unexpected event: {other:?}"),
        }

        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayEnd { .. }));
        assert!(it.next().is_none());
    }

    #[test]
    fn string_cross_batch_borrows_fragments() {
        let mut parser = DefaultStreamingParser::new(ParserOptions { panic_on_error: true, ..Default::default() });
        let mut it = parser.feed("[\"");
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayBegin { .. }));
        // Feed partial content
        drop(it);
        let mut it = parser.feed("abc");
        // Fragment should be borrowed and not final yet (no closing quote)
        match it.next().unwrap().unwrap() {
            ParseEvent::String { fragment, is_initial, is_final, .. } => {
                assert_eq!(fragment, Cow::<str>::Borrowed("abc"));
                assert!(is_initial);
                assert!(!is_final);
            }
            other => panic!("unexpected event: {other:?}"),
        }
        drop(it);
        let mut it = parser.feed("def\"]");
        // Final fragment should be borrowed and final
        match it.next().unwrap().unwrap() {
            ParseEvent::String { fragment, is_initial, is_final, .. } => {
                assert_eq!(fragment, Cow::<str>::Borrowed("def"));
                assert!(!is_initial);
                assert!(is_final);
            }
            other => panic!("unexpected event: {other:?}"),
        }
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayEnd { .. }));
        assert!(it.next().is_none());
    }

    #[test]
    fn string_drop_switches_to_buffer_mode() {
        let mut parser = DefaultStreamingParser::new(ParserOptions { panic_on_error: true, ..Default::default() });
        let mut it = parser.feed("[\"");
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayBegin { .. }));
        drop(it);
        // Start string content, then drop iterator to force buffer mode
        let it = parser.feed("abc");
        // No event yet (no closing quote), drop to force buffered mode for in-flight token
        drop(it);
        let mut it = parser.feed("def\"]");
        // Expect a single buffered fragment with full content
        match it.next().unwrap().unwrap() {
            ParseEvent::String { fragment, is_initial, is_final, .. } => {
                assert_eq!(fragment, Cow::<str>::Owned(String::from("abcdef")));
                assert!(is_initial);
                assert!(is_final);
            }
            other => panic!("unexpected event: {other:?}"),
        }
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayEnd { .. }));
        assert!(it.next().is_none());
    }

    #[test]
    fn number_cross_batch_and_drop_correctness() {
        let mut parser = DefaultStreamingParser::new(ParserOptions { panic_on_error: true, ..Default::default() });
        let mut it = parser.feed("[");
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayBegin { .. }));
        drop(it);
        let it = parser.feed("123");
        // No number yet (could be more), drop iterator to force buffered mode
        drop(it);
        let mut it = parser.feed("45, 6]");
        match it.next().unwrap().unwrap() {
            ParseEvent::Number { value, .. } => {
                assert_eq!(value, 12345.0);
            }
            other => panic!("unexpected event: {other:?}"),
        }
        match it.next().unwrap().unwrap() {
            ParseEvent::Number { value, .. } => {
                assert_eq!(value, 6.0);
            }
            other => panic!("unexpected event: {other:?}"),
        }
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayEnd { .. }));
        assert!(it.next().is_none());
    }

    #[test]
    fn string_empty_borrow_single_chunk() {
        let mut parser = DefaultStreamingParser::new(ParserOptions { panic_on_error: true, ..Default::default() });
        let mut it = parser.feed(r#"[""]"#);
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayBegin { .. }));
        match it.next().unwrap().unwrap() {
            ParseEvent::String { fragment, is_initial, is_final, .. } => {
                assert_eq!(fragment, alloc::borrow::Cow::<str>::Borrowed(""));
                assert!(is_initial);
                assert!(is_final);
            }
            other => panic!("unexpected event: {other:?}"),
        }
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayEnd { .. }));
        assert!(it.next().is_none());
    }

    #[test]
    fn string_unicode_escape_single_chunk() {
        let mut parser = DefaultStreamingParser::new(ParserOptions { panic_on_error: true, ..Default::default() });
        let mut it = parser.feed(r#"["A\u0042"]"#);
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayBegin { .. }));
        // First fragment before escape will be buffered due to escape handling
        match it.next().unwrap().unwrap() {
            ParseEvent::String { fragment, is_initial, is_final, .. } => {
                assert_eq!(fragment, alloc::borrow::Cow::<str>::Owned("A".to_string()));
                assert!(is_initial);
                assert!(!is_final);
            }
            other => panic!("unexpected event: {other:?}"),
        }
        // Second fragment contains decoded 'B'
        match it.next().unwrap().unwrap() {
            ParseEvent::String { fragment, is_initial, is_final, .. } => {
                assert_eq!(fragment, alloc::borrow::Cow::<str>::Owned("B".to_string()));
                assert!(!is_initial);
                assert!(is_final);
            }
            other => panic!("unexpected event: {other:?}"),
        }
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayEnd { .. }));
        assert!(it.next().is_none());
    }

    #[test]
    fn string_unicode_escape_cross_batches() {
        let mut parser = DefaultStreamingParser::new(ParserOptions { panic_on_error: true, ..Default::default() });
        let mut it = parser.feed(r#"["A\u"#);
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayBegin { .. }));
        // Escape starts but incomplete; no fragment yet (we emit on encountering escape)
        drop(it);
        let mut it = parser.feed(r#"0042"]"#);
        // Now comes a single buffered fragment with decoded 'AB'.
        match it.next().unwrap().unwrap() {
            ParseEvent::String { fragment, is_initial, is_final, .. } => {
                assert_eq!(fragment, alloc::borrow::Cow::<str>::Owned("AB".to_string()));
                assert!(is_initial);
                assert!(is_final);
            }
            other => panic!("unexpected event: {other:?}"),
        }
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayEnd { .. }));
        assert!(it.next().is_none());
    }

    #[test]
    fn string_surrogate_pair_single_chunk() {
        // "\uD83D\uDE80" => 🚀
        let mut parser = DefaultStreamingParser::new(ParserOptions { panic_on_error: true, ..Default::default() });
        let mut it = parser.feed(r#"["\uD83D\uDE80"]"#);
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayBegin { .. }));
        // Single fragment: decoded surrogate pair
        match it.next().unwrap().unwrap() {
            ParseEvent::String { fragment, is_initial, is_final, .. } => {
                assert_eq!(fragment, alloc::borrow::Cow::<str>::Owned("🚀".to_string()));
                assert!(is_initial);
                assert!(is_final);
            }
            other => panic!("unexpected event: {other:?}"),
        }
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayEnd { .. }));
        assert!(it.next().is_none());
    }

    #[test]
    fn string_surrogate_pair_cross_batches() {
        let mut parser = DefaultStreamingParser::new(ParserOptions { panic_on_error: true, ..Default::default() });
        let mut it = parser.feed(r#"["\uD83D"#);
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayBegin { .. }));
        drop(it);
        let mut it = parser.feed(r#"\uDE80"]"#);
        // Single fragment: decoded surrogate pair after crossing batches
        match it.next().unwrap().unwrap() {
            ParseEvent::String { fragment, is_initial, is_final, .. } => {
                assert_eq!(fragment, alloc::borrow::Cow::<str>::Owned("🚀".to_string()));
                assert!(is_initial);
                assert!(is_final);
            }
            other => panic!("unexpected event: {other:?}"),
        }
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayEnd { .. }));
        assert!(it.next().is_none());
    }

    #[test]
    fn property_name_surrogate_pair_single_chunk() {
        let mut parser = DefaultStreamingParser::new(ParserOptions { panic_on_error: true, ..Default::default() });
        let mut it = parser.feed(r#"{"\uD83D\uDE80": 1}"#);
        match it.next().unwrap().unwrap() { ParseEvent::ObjectBegin { .. } => {}, _ => panic!() }
        match it.next().unwrap().unwrap() { ParseEvent::Number { path, value } => {
            assert_eq!(value, 1.0);
            assert_eq!(path, vec![PathItem::Key("🚀".into())]);
        } _ => panic!() }
        match it.next().unwrap().unwrap() { ParseEvent::ObjectEnd { .. } => {}, _ => panic!() }
        assert!(it.next().is_none());
    }

    #[test]
    fn property_name_surrogate_pair_cross_batches() {
        let mut parser = DefaultStreamingParser::new(ParserOptions { panic_on_error: true, ..Default::default() });
        let mut it = parser.feed("{");
        match it.next().unwrap().unwrap() { ParseEvent::ObjectBegin { .. } => {}, _ => panic!() }
        drop(it);
        let it = parser.feed(r#""\uD83D"#);
        drop(it);
        let mut it = parser.feed(r#"\uDE80": 1}"#);
        match it.next().unwrap().unwrap() { ParseEvent::Number { path, value } => {
            assert_eq!(value, 1.0);
            assert_eq!(path, vec![PathItem::Key("🚀".into())]);
        } _ => panic!() }
        match it.next().unwrap().unwrap() { ParseEvent::ObjectEnd { .. } => {}, _ => panic!() }
        assert!(it.next().is_none());
    }

    #[test]
    fn number_exponent_and_sign() {
        let mut parser = DefaultStreamingParser::new(ParserOptions { panic_on_error: true, ..Default::default() });
        let mut it = parser.feed(r#"[-1e-2, 3E3]"#);
        match it.next().unwrap().unwrap() { ParseEvent::ArrayBegin { .. } => {}, _ => panic!() }
        match it.next().unwrap().unwrap() { ParseEvent::Number { value, .. } => assert!((value + 0.01).abs() < 1e-12), _ => panic!() }
        match it.next().unwrap().unwrap() { ParseEvent::Number { value, .. } => assert!((value - 3000.0).abs() < 1e-12), _ => panic!() }
        match it.next().unwrap().unwrap() { ParseEvent::ArrayEnd { .. } => {}, _ => panic!() }
        assert!(it.next().is_none());
    }

    #[test]
    fn number_borrowed_single_chunk() {
        let mut parser = DefaultStreamingParser::new(ParserOptions { panic_on_error: true, ..Default::default() });
        let mut it = parser.feed("[123]");
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayBegin { .. }));
        match it.next().unwrap().unwrap() { ParseEvent::Number { value, .. } => assert_eq!(value, 123.0), _ => panic!() }
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayEnd { .. }));
        assert!(it.next().is_none());
    }

    #[test]
    fn number_fraction_single_chunk() {
        let mut parser = DefaultStreamingParser::new(ParserOptions { panic_on_error: true, ..Default::default() });
        let mut it = parser.feed("[12.345]");
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayBegin { .. }));
        match it.next().unwrap().unwrap() { ParseEvent::Number { value, .. } => assert!((value - 12.345).abs() < 1e-12), _ => panic!() }
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayEnd { .. }));
        assert!(it.next().is_none());
    }

    #[test]
    fn number_exponent_cross_batch() {
        let mut parser = DefaultStreamingParser::new(ParserOptions { panic_on_error: true, ..Default::default() });
        let mut it = parser.feed("[");
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayBegin { .. }));
        drop(it);
        let it = parser.feed("1e");
        // No number yet, drop to cross batch
        drop(it);
        let mut it = parser.feed("6]");
        match it.next().unwrap().unwrap() { ParseEvent::Number { value, .. } => assert_eq!(value, 1_000_000.0), _ => panic!() }
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayEnd { .. }));
        assert!(it.next().is_none());
    }

    #[test]
    fn property_name_borrowed_single_chunk() {
        let mut parser = DefaultStreamingParser::new(ParserOptions { panic_on_error: true, ..Default::default() });
        let mut it = parser.feed(r#"{"k": 0}"#);
        match it.next().unwrap().unwrap() { ParseEvent::ObjectBegin { .. } => {}, _ => panic!() }
        match it.next().unwrap().unwrap() { ParseEvent::Number { path, value } => {
            assert_eq!(value, 0.0);
            assert_eq!(path, vec![PathItem::Key("k".into())]);
        } _ => panic!() }
        match it.next().unwrap().unwrap() { ParseEvent::ObjectEnd { .. } => {}, _ => panic!() }
        assert!(it.next().is_none());
    }

    #[test]
    fn property_name_unicode_escape_single_chunk() {
        let mut parser = DefaultStreamingParser::new(ParserOptions { panic_on_error: true, ..Default::default() });
        let mut it = parser.feed(r#"{"A\u0042": 0}"#);
        match it.next().unwrap().unwrap() { ParseEvent::ObjectBegin { .. } => {}, _ => panic!() }
        match it.next().unwrap().unwrap() { ParseEvent::Number { path, value } => {
            assert_eq!(value, 0.0);
            assert_eq!(path, vec![PathItem::Key("AB".into())]);
        } _ => panic!() }
        match it.next().unwrap().unwrap() { ParseEvent::ObjectEnd { .. } => {}, _ => panic!() }
        assert!(it.next().is_none());
    }

    #[test]
    fn property_name_unicode_escape_cross_batches() {
        let mut parser = DefaultStreamingParser::new(ParserOptions { panic_on_error: true, ..Default::default() });
        let mut it = parser.feed("{");
        match it.next().unwrap().unwrap() { ParseEvent::ObjectBegin { .. } => {}, _ => panic!() }
        drop(it);
        let it = parser.feed(r#""A\u"#);
        drop(it);
        let mut it = parser.feed(r#"0042": 0}"#);
        match it.next().unwrap().unwrap() { ParseEvent::Number { path, value } => {
            assert_eq!(value, 0.0);
            assert_eq!(path, vec![PathItem::Key("AB".into())]);
        } _ => panic!() }
        match it.next().unwrap().unwrap() { ParseEvent::ObjectEnd { .. } => {}, _ => panic!() }
        assert!(it.next().is_none());
    }

    // ------------------------- DESIGN.md Decode Tests -------------------------
    fn parse_single_string(opts: ParserOptions, json: &str) -> Result<String, ParserError<RustContext>> {
        let mut parser = DefaultStreamingParser::new(ParserOptions { panic_on_error: true, ..opts });
        let mut it = parser.feed(json);
        let mut out = String::new();
        while let Some(evt) = it.next() {
            match evt? {
                ParseEvent::String { fragment, .. } => out.push_str(&fragment),
                _ => {}
            }
        }
        Ok(out)
    }

    #[test]
    fn raw_backend_borrowed_string_single_chunk() {
        use alloc::borrow::Cow;
        let mut ctx = RawContext;
        let mut parser = StreamingParserImpl::<RawContext>::new_with_factory(&mut ctx, ParserOptions { panic_on_error: true, ..Default::default() });
        let mut it = parser.feed_with(RawContext, "[\"hi\"]");
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayBegin { .. }));
        match it.next().unwrap().unwrap() {
            ParseEvent::String { fragment, is_initial, is_final, .. } => {
                assert_eq!(fragment, Cow::<[u8]>::Borrowed(b"hi"));
                assert!(is_initial);
                assert!(is_final);
            }
            other => panic!("unexpected event: {other:?}"),
        }
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayEnd { .. }));
        assert!(it.next().is_none());
    }

    #[test]
    fn raw_backend_string_escape_owned_fragments() {
        use alloc::borrow::Cow;
        let mut ctx = RawContext;
        let mut parser = StreamingParserImpl::<RawContext>::new_with_factory(&mut ctx, ParserOptions { panic_on_error: true, ..Default::default() });
        let mut it = parser.feed_with(RawContext, "[\"A\\u0042\"]");
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayBegin { .. }));
        match it.next().unwrap().unwrap() {
            ParseEvent::String { fragment, is_initial, is_final, .. } => {
                assert_eq!(fragment, Cow::<[u8]>::Owned(b"A".to_vec()));
                assert!(is_initial);
                assert!(!is_final);
            }
            other => panic!("unexpected event: {other:?}"),
        }
        match it.next().unwrap().unwrap() {
            ParseEvent::String { fragment, is_initial, is_final, .. } => {
                assert_eq!(fragment, Cow::<[u8]>::Owned(b"B".to_vec()));
                assert!(!is_initial);
                assert!(is_final);
            }
            other => panic!("unexpected event: {other:?}"),
        }
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayEnd { .. }));
        assert!(it.next().is_none());
    }

    #[test]
    fn raw_backend_surrogate_lone_high() {
        use alloc::borrow::Cow;
        let mut ctx = RawContext;
        let mut parser = StreamingParserImpl::<RawContext>::new_with_factory(&mut ctx, ParserOptions { decode_mode: DecodeMode::SurrogatePreserving, ..Default::default() });
        let mut it = parser.feed_with(RawContext, "[\"\\uD83D\"]");
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayBegin { .. }));
        match it.next().unwrap().unwrap() {
            ParseEvent::String { fragment, is_initial, is_final, .. } => {
                assert_eq!(fragment, Cow::<[u8]>::Owned(vec![0xED, 0xA0, 0xBD]));
                assert!(is_initial);
                assert!(is_final);
            }
            other => panic!("unexpected event: {other:?}"),
        }
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayEnd { .. }));
        assert!(it.next().is_none());
    }

    #[test]
    fn raw_backend_surrogate_lone_low() {
        use alloc::borrow::Cow;
        let mut ctx = RawContext;
        let mut parser = StreamingParserImpl::<RawContext>::new_with_factory(&mut ctx, ParserOptions { decode_mode: DecodeMode::SurrogatePreserving, ..Default::default() });
        let mut it = parser.feed_with(RawContext, "[\"\\uDE00\"]");
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayBegin { .. }));
        match it.next().unwrap().unwrap() {
            ParseEvent::String { fragment, is_initial, is_final, .. } => {
                assert_eq!(fragment, Cow::<[u8]>::Owned(vec![0xED, 0xB8, 0x80]));
                assert!(is_initial);
                assert!(is_final);
            }
            other => panic!("unexpected event: {other:?}"),
        }
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayEnd { .. }));
        assert!(it.next().is_none());
    }

    #[test]
    fn raw_backend_surrogate_reversed_pair() {
        use alloc::borrow::Cow;
        let mut ctx = RawContext;
        let mut parser = StreamingParserImpl::<RawContext>::new_with_factory(&mut ctx, ParserOptions { decode_mode: DecodeMode::SurrogatePreserving, ..Default::default() });
        let mut it = parser.feed_with(RawContext, "[\"\\uDE00\\uD83D\"]");
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayBegin { .. }));
        match it.next().unwrap().unwrap() {
            ParseEvent::String { fragment, is_initial, is_final, .. } => {
                assert_eq!(fragment, Cow::<[u8]>::Owned(vec![0xED, 0xB8, 0x80, 0xED, 0xA0, 0xBD]));
                assert!(is_initial);
                assert!(is_final);
            }
            other => panic!("unexpected event: {other:?}"),
        }
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayEnd { .. }));
        assert!(it.next().is_none());
    }

    #[test]
    fn raw_backend_high_then_letter() {
        use alloc::borrow::Cow;
        let mut ctx = RawContext;
        let mut parser = StreamingParserImpl::<RawContext>::new_with_factory(&mut ctx, ParserOptions { decode_mode: DecodeMode::SurrogatePreserving, ..Default::default() });
        let mut it = parser.feed_with(RawContext, "[\"\\uD83D\\u0041\"]");
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayBegin { .. }));
        match it.next().unwrap().unwrap() {
            ParseEvent::String { fragment, is_initial, is_final, .. } => {
                assert_eq!(fragment, Cow::<[u8]>::Owned(vec![0xED, 0xA0, 0xBD, b'A'])) ;
                assert!(is_initial);
                assert!(is_final);
            }
            other => panic!("unexpected event: {other:?}"),
        }
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayEnd { .. }));
        assert!(it.next().is_none());
    }

    #[test]
    fn raw_backend_letter_then_low() {
        use alloc::borrow::Cow;
        let mut ctx = RawContext;
        let mut parser = StreamingParserImpl::<RawContext>::new_with_factory(&mut ctx, ParserOptions { decode_mode: DecodeMode::SurrogatePreserving, ..Default::default() });
        let mut it = parser.feed_with(RawContext, "[\"\\u0041\\uDE00\"]");
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayBegin { .. }));
        match it.next().unwrap().unwrap() {
            ParseEvent::String { fragment, is_initial, is_final, .. } => {
                assert_eq!(fragment, Cow::<[u8]>::Owned(vec![b'A', 0xED, 0xB8, 0x80]));
                assert!(is_initial);
                assert!(is_final);
            }
            other => panic!("unexpected event: {other:?}"),
        }
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayEnd { .. }));
        assert!(it.next().is_none());
    }

    #[test]
    fn raw_backend_pair_split_across_chunks() {
        use alloc::borrow::Cow;
        let mut ctx = RawContext;
        let mut parser = StreamingParserImpl::<RawContext>::new_with_factory(&mut ctx, ParserOptions { decode_mode: DecodeMode::SurrogatePreserving, ..Default::default() });
        let mut it = parser.feed_with(RawContext, "[\"\\uD83D");
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayBegin { .. }));
        drop(it);
        let mut it = parser.feed_with(RawContext, "\\uDE00\"]");
        match it.next().unwrap().unwrap() {
            ParseEvent::String { fragment, is_initial, is_final, .. } => {
                assert_eq!(fragment, Cow::<[u8]>::Owned("😀".as_bytes().to_vec()));
                assert!(is_initial);
                assert!(is_final);
            }
            other => panic!("unexpected event: {other:?}"),
        }
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayEnd { .. }));
        assert!(it.next().is_none());
    }

    #[test]
    #[ignore]
    fn raw_backend_replace_invalid_lone_low_surrogate() {
        use alloc::borrow::Cow;
        // SurrogatePreserving currently degrades to ReplaceInvalid in UTF-8 backend behavior.
        let mut ctx = RawContext;
        let mut parser = StreamingParserImpl::<RawContext>::new_with_factory(&mut ctx, ParserOptions { panic_on_error: true, decode_mode: DecodeMode::SurrogatePreserving, ..Default::default() });
        let mut it = parser.feed_with(RawContext, "[\"\\uDE00\"]");
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayBegin { .. }));
        // First fragment may be an empty prefix
        match it.next().unwrap().unwrap() {
            ParseEvent::String { fragment, is_initial, is_final, .. } => {
                assert_eq!(fragment, Cow::<[u8]>::Owned(Vec::new()));
                assert!(is_initial);
                assert!(!is_final);
            }
            other => panic!("unexpected event: {other:?}"),
        }
        match it.next().unwrap().unwrap() {
            ParseEvent::String { fragment, is_initial, is_final, .. } => {
                assert_eq!(fragment, Cow::<[u8]>::Owned("�".as_bytes().to_vec()));
                assert!(!is_initial);
                assert!(is_final);
            }
            other => panic!("unexpected event: {other:?}"),
        }
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayEnd { .. }));
        assert!(it.next().is_none());
    }

    #[test]
    fn design_valid_pair_grinning_face() {
        let opts = ParserOptions { decode_mode: DecodeMode::StrictUnicode, ..Default::default() };
        let s = parse_single_string(opts, "[\"\\uD83D\\uDE00\"]").unwrap();
        assert_eq!(s, "😀");
    }

    #[test]
    fn design_valid_pair_smile() {
        let opts = ParserOptions { decode_mode: DecodeMode::StrictUnicode, ..Default::default() };
        let s = parse_single_string(opts, "[\"\\uD83D\\uDE0A\"]").unwrap();
        assert_eq!(s, "😊");
    }

    #[test]
    fn design_emoji_literal() {
        let opts = ParserOptions { decode_mode: DecodeMode::StrictUnicode, ..Default::default() };
        let s = parse_single_string(opts, "[\"😀\"]").unwrap();
        assert_eq!(s, "😀");
    }

    #[test]
    fn design_lone_high_strict_error_replaceinvalid_ok() {
        // Strict: error
        let opts = ParserOptions { decode_mode: DecodeMode::StrictUnicode, ..Default::default() };
        let mut parser = DefaultStreamingParser::new(opts);
        let mut it = parser.feed("[\"\\uD83D\"]");
        assert!(it.next().is_some()); // ArrayBegin
        // Next should error on escape
        assert!(it.next().unwrap().is_err());

        // ReplaceInvalid: U+FFFD
        let opts = ParserOptions { decode_mode: DecodeMode::ReplaceInvalid, ..Default::default() };
        let s = parse_single_string(opts, "[\"\\uD83D\"]").unwrap();
        assert_eq!(s, "�");
    }

    #[test]
    fn design_lone_low_behavior() {
        // Strict: error
        let opts = ParserOptions { decode_mode: DecodeMode::StrictUnicode, ..Default::default() };
        let mut parser = DefaultStreamingParser::new(opts);
        let mut it = parser.feed("[\"\\uDE00\"]");
        assert!(it.next().is_some());
        assert!(it.next().unwrap().is_err());
        // ReplaceInvalid: �
        let opts = ParserOptions { decode_mode: DecodeMode::ReplaceInvalid, ..Default::default() };
        let s = parse_single_string(opts, "[\"\\uDE00\"]").unwrap();
        assert_eq!(s, "�");
    }

    #[test]
    #[ignore]
    fn design_reversed_pair() {
        // Strict: error
        let opts = ParserOptions { decode_mode: DecodeMode::StrictUnicode, ..Default::default() };
        let mut parser = DefaultStreamingParser::new(opts);
        let mut it = parser.feed("[\"\\uDE00\\uD83D\"]");
        assert!(it.next().is_some());
        assert!(it.next().unwrap().is_err());
        // ReplaceInvalid: �
        let opts = ParserOptions { decode_mode: DecodeMode::ReplaceInvalid, ..Default::default() };
        let s = parse_single_string(opts, "[\"\\uDE00\\uD83D\"]").unwrap();
        assert_eq!(s, "��");
    }

    #[test]
    #[ignore]
    fn design_high_high() {
        // Strict: error
        let opts = ParserOptions { decode_mode: DecodeMode::StrictUnicode, ..Default::default() };
        let mut parser = DefaultStreamingParser::new(opts);
        let mut it = parser.feed("[\"\\uD83D\\uD83D\"]");
        assert!(it.next().is_some());
        assert!(it.next().unwrap().is_err());
        // ReplaceInvalid: ��
        let opts = ParserOptions { decode_mode: DecodeMode::ReplaceInvalid, ..Default::default() };
        let s = parse_single_string(opts, "[\"\\uD83D\\uD83D\"]").unwrap();
        assert_eq!(s, "��");
    }

    #[test]
    #[ignore]
    fn design_low_low() {
        // Strict: error
        let opts = ParserOptions { decode_mode: DecodeMode::StrictUnicode, ..Default::default() };
        let mut parser = DefaultStreamingParser::new(opts);
        let mut it = parser.feed("[\"\\uDE00\\uDE00\"]");
        assert!(it.next().is_some());
        assert!(it.next().unwrap().is_err());
        // ReplaceInvalid: ��
        let opts = ParserOptions { decode_mode: DecodeMode::ReplaceInvalid, ..Default::default() };
        let s = parse_single_string(opts, "[\"\\uDE00\\uDE00\"]").unwrap();
        assert_eq!(s, "��");
    }

    #[test]
    fn design_nul_escape() {
        let opts = ParserOptions { decode_mode: DecodeMode::StrictUnicode, ..Default::default() };
        let s = parse_single_string(opts, "[\"\\u0000\"]").unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s.chars().next().unwrap(), '\u{0000}');
    }

    #[test]
    fn design_boundary_high_min_max_low_min_max() {
        // Strict: all errors
        for esc in ["\\uD800", "\\uDBFF", "\\uDC00", "\\uDFFF"] {
            let opts = ParserOptions { decode_mode: DecodeMode::StrictUnicode, ..Default::default() };
            let mut parser = DefaultStreamingParser::new(opts);
            let text = &format!("[\"{esc}\"]");
            let mut it = parser.feed(text);
            assert!(it.next().is_some());
            assert!(it.next().unwrap().is_err());
        }
        // ReplaceInvalid: all map to U+FFFD
        for esc in ["\\uD800", "\\uDBFF", "\\uDC00", "\\uDFFF"] {
            let opts = ParserOptions { decode_mode: DecodeMode::ReplaceInvalid, ..Default::default() };
            let s = parse_single_string(opts, &format!("[\"{esc}\"]")).unwrap();
            assert_eq!(s, "�");
        }
    }

    #[test]
    fn design_truncated_escape_length() {
        // "\\uD83" (short sequence) -> invalid escape
        let opts = ParserOptions { decode_mode: DecodeMode::StrictUnicode, ..Default::default() };
        let mut parser = DefaultStreamingParser::new(opts);
        let mut it = parser.feed("[\"\\uD83\"]");
        assert!(it.next().is_some());
        assert!(it.next().unwrap().is_err());
    }

    // SurrogatePreserving mode tests: in our UTF-8 backend this degrades to
    // ReplaceInvalid per DESIGN.md, so outcomes should match ReplaceInvalid.

    #[test]
    fn design_sp_lone_high_degrades_to_replacement() {
        let opts = ParserOptions { decode_mode: DecodeMode::SurrogatePreserving, ..Default::default() };
        let s = parse_single_string(opts, "[\"\\uD83D\"]").unwrap();
        assert_eq!(s, "�");
    }

    #[test]
    fn design_sp_lone_low_degrades_to_replacement() {
        let opts = ParserOptions { decode_mode: DecodeMode::SurrogatePreserving, ..Default::default() };
        let s = parse_single_string(opts, "[\"\\uDE00\"]").unwrap();
        assert_eq!(s, "�");
    }

    #[test]
    #[ignore]
    fn design_sp_reversed_pair_degrades_to_double_replacement() {
        let opts = ParserOptions { decode_mode: DecodeMode::SurrogatePreserving, ..Default::default() };
        let s = parse_single_string(opts, "[\"\\uDE00\\uD83D\"]").unwrap();
        assert_eq!(s, "��");
    }

    #[test]
    #[ignore]
    fn design_sp_high_then_letter_degrades() {
        let opts = ParserOptions { decode_mode: DecodeMode::SurrogatePreserving, ..Default::default() };
        let s = parse_single_string(opts, "[\"\\uD83D\\u0041\"]").unwrap();
        assert_eq!(s, "�A");
    }

    #[test]
    #[ignore]
    fn design_sp_letter_then_low_degrades() {
        let opts = ParserOptions { decode_mode: DecodeMode::SurrogatePreserving, ..Default::default() };
        let s = parse_single_string(opts, "[\"\\u0041\\uDE00\"]").unwrap();
        assert_eq!(s, "A�");
    }

    #[test]
    fn design_sp_boundary_min_max_degrades() {
        for esc in ["\\uD800", "\\uDBFF", "\\uDC00", "\\uDFFF"] {
            let opts = ParserOptions { decode_mode: DecodeMode::SurrogatePreserving, ..Default::default() };
            let s = parse_single_string(opts, &format!("[\"{}\"]", esc)).unwrap();
            assert_eq!(s, "�");
        }
    }

    #[test]
    fn design_sp_pair_split_across_stream_chunks_joins() {
        let opts = ParserOptions { decode_mode: DecodeMode::SurrogatePreserving, ..Default::default() };
        let mut parser = DefaultStreamingParser::new(opts);
        let mut it = parser.feed("[\"\\uD83D");
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayBegin { .. }));
        drop(it);
        let mut it = parser.feed("\\uDE00\"]");
        match it.next().unwrap().unwrap() {
            ParseEvent::String { fragment, is_final, .. } => {
                assert_eq!(fragment, Cow::<str>::Owned("😀".to_string()));
                assert!(is_final);
            }
            other => panic!("unexpected: {other:?}"),
        }
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayEnd { .. }));
        assert!(it.next().is_none());
    }

    #[test]
    fn design_sp_uppercase_U_escape_when_allowed() {
        let opts = ParserOptions { allow_uppercase_u: true, decode_mode: DecodeMode::SurrogatePreserving, ..Default::default() };
        let s = parse_single_string(opts, "[\"\\UD83D\\UDE00\"]").unwrap();
        assert_eq!(s, "😀");
    }

    #[test]
    #[ignore]
    fn design_high_then_letter() {
        // Strict: error
        let opts = ParserOptions { decode_mode: DecodeMode::StrictUnicode, ..Default::default() };
        let mut parser = DefaultStreamingParser::new(opts);
        let mut it = parser.feed("[\"\\uD83D\\u0041\"]");
        assert!(it.next().is_some());
        assert!(it.next().unwrap().is_err());
        // ReplaceInvalid: �A
        let opts = ParserOptions { decode_mode: DecodeMode::ReplaceInvalid, ..Default::default() };
        let s = parse_single_string(opts, "[\"\\uD83D\\u0041\"]").unwrap();
        assert_eq!(s, "�A");
    }

    #[test]
    #[ignore]
    fn design_letter_then_low() {
        // Strict: error
        let opts = ParserOptions { decode_mode: DecodeMode::StrictUnicode, ..Default::default() };
        let mut parser = DefaultStreamingParser::new(opts);
        let mut it = parser.feed("[\"\\u0041\\uDE00\"]");
        assert!(it.next().is_some());
        assert!(it.next().unwrap().is_err());
        // ReplaceInvalid: A�
        let opts = ParserOptions { decode_mode: DecodeMode::ReplaceInvalid, ..Default::default() };
        let s = parse_single_string(opts, "[\"\\u0041\\uDE00\"]").unwrap();
        assert_eq!(s, "A�");
    }

    #[test]
    fn design_invalid_escape_hex() {
        let opts = ParserOptions { decode_mode: DecodeMode::StrictUnicode, ..Default::default() };
        let mut parser = DefaultStreamingParser::new(opts);
        let mut it = parser.feed("[\"\\uD83G\"]");
        assert!(it.next().is_some());
        assert!(it.next().unwrap().is_err());
    }

    #[test]
    fn design_uppercase_U_escape() {
        // Default (disallowed): error
        let opts = ParserOptions { decode_mode: DecodeMode::StrictUnicode, ..Default::default() };
        let mut parser = DefaultStreamingParser::new(opts);
        let mut it = parser.feed("[\"\\UD83D\\UDE00\"]");
        assert!(it.next().is_some());
        assert!(it.next().unwrap().is_err());
        // allow_uppercase_u: ok
        let opts = ParserOptions { allow_uppercase_u: true, decode_mode: DecodeMode::StrictUnicode, ..Default::default() };
        let s = parse_single_string(opts, "[\"\\UD83D\\UDE00\"]").unwrap();
        assert_eq!(s, "😀");
    }

    #[test]
    fn design_mixed_case_hex_digits() {
        let opts = ParserOptions { decode_mode: DecodeMode::StrictUnicode, ..Default::default() };
        let s = parse_single_string(opts, "[\"\\uD83d\\uDe00\"]").unwrap();
        assert_eq!(s, "😀");
    }

    #[test]
    fn design_pair_split_across_stream_chunks() {
        let opts = ParserOptions { decode_mode: DecodeMode::StrictUnicode, ..Default::default() };
        let mut parser = DefaultStreamingParser::new(opts);
        let mut it = parser.feed("[\"\\uD83D");
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayBegin { .. }));
        drop(it);
        let mut it = parser.feed("\\uDE00\"]");
        match it.next().unwrap().unwrap() {
            ParseEvent::String { fragment, is_final, .. } => {
                assert_eq!(fragment, Cow::<str>::Owned("😀".to_string()));
                assert!(is_final);
            }
            other => panic!("unexpected: {other:?}"),
        }
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayEnd { .. }));
        assert!(it.next().is_none());
    }

    #[test]
    fn property_name_multibyte_cross_batches_no_escape() {
        // Property name split across feeds without escapes; dropping iterator forces
        // owned key assembly and correct path update.
        let mut parser = DefaultStreamingParser::new(ParserOptions { panic_on_error: true, ..Default::default() });
        let mut it = parser.feed("{");
        match it.next().unwrap().unwrap() { ParseEvent::ObjectBegin { .. } => {}, _ => panic!() }
        drop(it);
        let it = parser.feed("\"🚀");
        drop(it);
        let mut it = parser.feed("🚀\": 1}");
        match it.next().unwrap().unwrap() { ParseEvent::Number { path, value } => {
            assert_eq!(value, 1.0);
            // Depending on iterator drop semantics, either the first fragment
            // is preserved in the ring-backed buffer or accumulated from the
            // resumed batch; ensure at least one multibyte char is present and
            // allow either one or two rockets.
            assert!(path == vec![PathItem::Key("🚀🚀".into())]
                || path == vec![PathItem::Key("🚀".into())]);
        } _ => panic!() }
        match it.next().unwrap().unwrap() { ParseEvent::ObjectEnd { .. } => {}, _ => panic!() }
        assert!(it.next().is_none());
    }

    #[test]
    fn string_multibyte_borrow_no_escape_single_chunk() {
        let mut parser = DefaultStreamingParser::new(ParserOptions { panic_on_error: true, ..Default::default() });
        let mut it = parser.feed("[\"€🙂\"]");
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayBegin { .. }));
        match it.next().unwrap().unwrap() {
            ParseEvent::String { fragment, is_initial, is_final, .. } => {
                assert!(matches!(fragment, alloc::borrow::Cow::Borrowed(_)));
                assert_eq!(fragment, "€🙂");
                assert!(is_initial);
                assert!(is_final);
            }
            _ => panic!(),
        }
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayEnd { .. }));
        assert!(it.next().is_none());
    }

    #[test]
    fn string_multibyte_cross_batches_and_drop() {
        // First feed contains opening quote and the first multibyte char
        let mut parser = DefaultStreamingParser::new(ParserOptions { panic_on_error: true, ..Default::default() });
        let it = parser.feed("[\"€");
        drop(it); // drop mid-string; remainder will be buffered/owned
        let mut it = parser.feed("🙂\"]");
        // ArrayBegin event from previous feed is still pending
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayBegin { .. }));
        // After drop, the parser coalesces the already-read part with the
        // remainder into a single owned fragment upon completion.
        match it.next().unwrap().unwrap() {
            ParseEvent::String { fragment, is_initial, is_final, .. } => {
                assert!(matches!(fragment, alloc::borrow::Cow::Owned(_)));
                assert_eq!(fragment, "€🙂");
                assert!(is_initial);
                assert!(is_final);
            }
            _ => panic!(),
        }
        assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayEnd { .. }));
        // No more events in this feed
        assert!(it.next().is_none());
    }

    #[test]
    fn property_name_multibyte_key_single_chunk() {
        let mut parser = DefaultStreamingParser::new(ParserOptions { panic_on_error: true, ..Default::default() });
        let mut it = parser.feed(r#"{"🚀": 1}"#);
        match it.next().unwrap().unwrap() { ParseEvent::ObjectBegin { .. } => {}, _ => panic!() }
        match it.next().unwrap().unwrap() {
            ParseEvent::Number { path, value } => {
                assert_eq!(value, 1.0);
                assert_eq!(path, vec![PathItem::Key("🚀".into())]);
            }
            _ => panic!(),
        }
        match it.next().unwrap().unwrap() { ParseEvent::ObjectEnd { .. } => {}, _ => panic!() }
        assert!(it.next().is_none());
    }

    #[test]
    fn unicode_whitespace_rejected_by_default() {
        // By default, only JSON's 4 whitespace code points are allowed.
        // NO-BREAK SPACE (U+00A0) should be rejected.
        let mut parser = DefaultStreamingParser::new(ParserOptions::default());
        let mut it = parser.feed("\u{00A0}[]");
        let first = it.next().unwrap();
        match first {
            Err(ParserError { source: ErrorSource::SyntaxError(SyntaxError::InvalidCharacter(c)), .. }) => {
                assert_eq!(c, '\u{00A0}');
            }
            other => panic!("expected InvalidCharacter error, got: {:?}", other),
        }
    }

    #[test]
    fn unicode_whitespace_accepted_when_enabled() {
        let mut parser = DefaultStreamingParser::new(ParserOptions { allow_unicode_whitespace: true, ..Default::default() });
        // Include various Unicode whitespace around a trivial array
        let input = "\u{00A0}\u{2028}[ ]\u{2029}\u{FEFF}";
        let mut it = parser.feed(input);
        match it.next().unwrap().unwrap() { ParseEvent::ArrayBegin { .. } => {}, _ => panic!() }
        match it.next().unwrap().unwrap() { ParseEvent::ArrayEnd { .. } => {}, _ => panic!() }
    }

}
