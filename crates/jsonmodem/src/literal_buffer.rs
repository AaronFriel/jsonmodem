use crate::parser::Token;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpectedLiteralValue {
    Null,
    True,
    False,
}

/// What happened after feeding one more character into the literal matcher?
pub enum Step {
    /// Character matched, but the literal is not finished yet.
    NeedMore,
    /// Character matched *and* we consumed the last byte of the literal.
    Done(Token),
    /// Character did **not** match the expected byte.
    Reject,
}

/// `None`  ➜  we are **not** in the middle of a literal
/// `Some`  ➜  `(remaining_bytes, token_kind)` while matching
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ExpectedLiteralBuffer(Option<(&'static [u8], ExpectedLiteralValue)>);

impl ExpectedLiteralBuffer {
    /// No literal is in flight
    pub fn none() -> Self {
        ExpectedLiteralBuffer(None)
    }

    /// Start matching after the *first* character (`n`, `t`, or `f`)
    pub fn new(first: char) -> Self {
        match first {
            'n' => ExpectedLiteralBuffer(Some((b"ull", ExpectedLiteralValue::Null))),
            't' => ExpectedLiteralBuffer(Some((b"rue", ExpectedLiteralValue::True))),
            'f' => ExpectedLiteralBuffer(Some((b"alse", ExpectedLiteralValue::False))),
            _ => ExpectedLiteralBuffer::none(),
        }
    }

    /// Give the matcher the next input character and learn what to do next.
    pub fn step(&mut self, c: char) -> Step {
        // If we are not in the middle of a literal, any char is a reject
        let Some((bytes, kind)) = self.0.take() else {
            return Step::Reject;
        };

        // Do we in fact expect `c`?
        if bytes.first().is_some_and(|b| *b as char == c) {
            // Safe: we just checked that `bytes` is non‑empty
            let (_, rest) = bytes.split_first().unwrap();

            if rest.is_empty() {
                // Literal finished – emit a token
                Step::Done(match kind {
                    ExpectedLiteralValue::Null => Token::Null,
                    ExpectedLiteralValue::True => Token::Boolean(true),
                    ExpectedLiteralValue::False => Token::Boolean(false),
                })
            } else {
                // Still more to go – remember the rest
                self.0 = Some((rest, kind));
                Step::NeedMore
            }
        } else {
            // Mismatch – restore the state we took at the top
            self.0 = Some((bytes, kind));
            Step::Reject
        }
    }
}
