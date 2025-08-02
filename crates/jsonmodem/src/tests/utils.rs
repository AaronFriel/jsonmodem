use alloc::string::{String, ToString};

use quickcheck::QuickCheck;

use crate::{ParserOptions, StreamingParser, Value, parser::Token, value::write_escaped_string};

pub fn write_rendered_tokens<W: core::fmt::Write>(
    tokens: &[Token],
    f: &mut W,
) -> Result<(), core::fmt::Error> {
    // We render these tokens back into the JSON they represent, with the special
    // caveat that adjacent "string" tokens are merged into a single string
    let mut writing_string = false;
    for token in tokens {
        if writing_string {
            if let Token::String { fragment, .. } = &token {
                // If we are already writing a string, we append the next part of the string
                write_escaped_string(fragment, f)?;
                continue;
            }
            f.write_char('"')?;
            writing_string = false;
        }

        match &token {
            Token::Eof => break,
            Token::PropertyName { value } => {
                f.write_char('"')?;
                write_escaped_string(value, f)?;
                f.write_char('"')?;
            }
            Token::String { fragment, .. } => {
                f.write_char('"')?;
                writing_string = true;
                write_escaped_string(fragment, f)?;
            }
            Token::Boolean(b) => write!(f, "{b}")?,
            Token::Null => write!(f, "null")?,
            Token::Number(n) => write!(f, "{n}")?,
            Token::Punctuator(p) => f.write_char(*p as char)?,
        }
    }

    Ok(())
}

fn render_tokens(tokens: &[Token]) -> Result<String, core::fmt::Error> {
    let mut rendered = String::new();
    write_rendered_tokens(tokens, &mut rendered)?;
    Ok(rendered)
}

// struct TestValue {
//     value: Value,
//     tokens: Vec<Token>,
//     tokens_with_whitespace: Vec<Token>,
// }

// impl Arbitrary for TestValue {
//     fn arbitrary(g: &mut Gen) -> Self {
//         let value = Value::arbitrary(g);
//         let tokens = value.get_lexed_tokens();
//         let mut tokens_with_whitespace = Vec::new();

//         // intersperse whitespace tokens between non-whitespace tokens
//         for (i, token) in tokens.iter().enumerate() {
//             if i > 0 {
//                 // Add a whitespace token before the current token
//                 tokens_with_whitespace.push(Token {
//                     value: TokenValue::Whitespace(" ".to_string()),

//         TestValue {
//             value,
//             tokens,
//             tokens_with_whitespace,
//         }
//     }
// }
// }

#[test]
fn roundtrip_rendered_tokens() {
    #[allow(clippy::needless_pass_by_value)]
    fn prop(value: Value) -> bool {
        let mut parser = StreamingParser::new(ParserOptions::default());

        let str_repr = value.to_string();
        parser.feed(&str_repr);
        let mut parser = parser.finish();

        for _ in parser.by_ref() {}

        let tokens = parser.get_lexed_tokens();

        let rendered_tokens = render_tokens(tokens).expect("Failed to render tokens");

        str_repr == rendered_tokens
    }

    let tests = if cfg!(any(miri, feature = "test-fast")) {
        10
    } else if is_ci::cached() {
        10_000
    } else {
        1_000
    };

    QuickCheck::new()
        .tests(tests)
        .quickcheck(prop as fn(Value) -> bool);
}
