use alloc::{borrow::Cow, string::String};

use crate::{parser::Token, value::write_escaped_string};

pub fn write_rendered_tokens<W: core::fmt::Write>(
    tokens: &[Token],
    f: &mut W,
) -> Result<(), core::fmt::Error> {
    // We render these tokens back into the JSON they represent, with the special
    // caveat that adjacent "string" tokens are merged into a single string
    let mut writing_string = false;
    for token in tokens {
        if writing_string {
            if let Some(fragment) = string_fragment(token) {
                write_escaped_string(&fragment, f)?;
                continue;
            }
            f.write_char('"')?;
            writing_string = false;
        }

        match &token {
            Token::Eof => break,
            Token::PropertyName(value) => {
                f.write_char('"')?;
                write_escaped_string(value, f)?;
                f.write_char('"')?;
            }
            Token::PropertyNameRaw(raw) => {
                let text = String::from_utf8_lossy(raw);
                f.write_char('"')?;
                write_escaped_string(&text, f)?;
                f.write_char('"')?;
            }
            token if string_fragment(token).is_some() => {
                f.write_char('"')?;
                writing_string = true;
                if let Some(fragment) = string_fragment(token) {
                    write_escaped_string(&fragment, f)?;
                }
            }
            Token::Boolean(b) => write!(f, "{b}")?,
            Token::Null => write!(f, "null")?,
            Token::NumberBorrowed(n) => write!(f, "{n}")?,
            Token::Number(n) => write!(f, "{n}")?,
            Token::Punctuator(p) => f.write_char(*p as char)?,
            _ => unreachable!(),
        }
    }

    Ok(())
}

fn string_fragment<'a>(token: &'a Token<'a>) -> Option<Cow<'a, str>> {
    match token {
        Token::StringBorrowed(fragment) => Some(Cow::Borrowed(fragment)),
        Token::StringOwned(fragment) => Some(Cow::Borrowed(fragment)),
        Token::StringRaw(bytes) => Some(String::from_utf8_lossy(bytes)),
        _ => None,
    }
}

fn render_tokens(tokens: &[Token]) -> Result<String, core::fmt::Error> {
    let mut rendered = String::new();
    write_rendered_tokens(tokens, &mut rendered)?;
    Ok(rendered)
}

#[test]
fn roundtrip_rendered_tokens() {
    let tokens = [
        Token::NumberBorrowed("123"),
        Token::Punctuator(b','),
        Token::Boolean(true),
        Token::Punctuator(b','),
        Token::Null,
        Token::Eof,
    ];

    let rendered = render_tokens(&tokens).expect("render tokens");
    assert_eq!(rendered, "123,true,null");
}
