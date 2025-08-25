use alloc::{collections::BTreeMap, string::String, sync::Arc, vec::Vec};

/// A JSON value as defined by [RFC 8259].
///
/// The `Value` enum can represent any JSON data type.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// A JSON `null`.
    Null,
    /// A JSON boolean, represented by a [`bool`].
    Boolean(bool),
    /// A JSON number, represented by a [`f64`].
    Number(f64),
    /// A JSON string, represented by a [`String`].
    String(String),
    /// A JSON array, represented by a [`Vec`] of values.
    Array(Vec<Value>),
    /// A JSON object, represented by a [`BTreeMap`] of string keys to values.
    Object(BTreeMap<Arc<str>, Value>),
}

impl Value {
    /// Borrows the inner value if this is a [`Value::Boolean`] or `None`
    /// otherwise.
    #[must_use]
    pub fn as_boolean(&self) -> Option<&bool> {
        if let Self::Boolean(v) = self {
            Some(v)
        } else {
            None
        }
    }

    /// Borrows the inner value if this is a [`Value::Number`] or `None`
    /// otherwise.
    #[must_use]
    pub fn as_number(&self) -> Option<&f64> {
        if let Self::Number(v) = self {
            Some(v)
        } else {
            None
        }
    }

    /// Borrows the inner value if this is a [`Value::String`] or `None`
    /// otherwise.
    #[must_use]
    pub fn as_string(&self) -> Option<&String> {
        if let Self::String(v) = self {
            Some(v)
        } else {
            None
        }
    }

    /// Borrows the inner value if this is a [`Value::Array`] or `None`
    /// otherwise.
    #[must_use]
    pub fn as_array(&self) -> Option<&Vec<Value>> {
        if let Self::Array(v) = self {
            Some(v)
        } else {
            None
        }
    }

    /// Mutably borrows the inner value if this is a [`Value::Array`] or `None`
    /// otherwise.
    #[must_use]
    pub fn as_array_mut(&mut self) -> Option<&mut Vec<Value>> {
        if let Self::Array(v) = self {
            Some(v)
        } else {
            None
        }
    }

    /// Borrows the inner value if this is a [`Value::Object`] or `None`
    /// otherwise.
    #[must_use]
    pub fn as_object(&self) -> Option<&BTreeMap<Arc<str>, Value>> {
        if let Self::Object(v) = self {
            Some(v)
        } else {
            None
        }
    }

    /// Mutably borrows the inner value if this is a [`Value::Object`] or `None`
    /// otherwise.
    #[must_use]
    pub fn as_object_mut(&mut self) -> Option<&mut BTreeMap<Arc<str>, Value>> {
        if let Self::Object(v) = self {
            Some(v)
        } else {
            None
        }
    }
}

/// Escapes control characters in a string for inclusion in a JSON string
/// literal.
///
/// This function writes to the provided formatter, replacing characters such as
/// quotes, backslashes, control characters (<= U+001F), and Unicode line
/// separators with their JSON escape sequences.
pub(crate) fn write_escaped_string<W: core::fmt::Write>(src: &str, f: &mut W) -> core::fmt::Result {
    for c in src.chars() {
        match c {
            '"' => f.write_str("\\\"")?,
            '\\' => f.write_str("\\\\")?,
            // Escape Unicode line separators which pre-2019 JSON parsers may not handle correctly
            '\u{2028}' | '\u{2029}' => {
                write!(f, "\\u{:04X}", c as u32)?;
            }
            // Escape control characters for maximum compatibility and readability, but only
            // up to the basic multilingual plane (BMP). JSON requires exactly 4 hex digits for
            // escapes, so we leave the encoding of characters outside the BMP to any
            // downstream processing. (e.g.: encoding as UTF-16 surrogates).
            c if c.is_ascii_control() || c.is_control() && c as u32 <= 0xFFFF => {
                write!(f, "\\u{:04X}", c as u32)?;
            }
            _ => f.write_char(c)?,
        }
    }
    Ok(())
}

/// Escapes control characters in a string for inclusion in a JSON string
/// literal and returns the result.
///
/// This function is a convenience wrapper around [`write_escaped_string`] that
/// returns a `String`.
pub(crate) fn escape_string(src: &str) -> String {
    let mut result = String::with_capacity(src.len() + 2); // +2 for surrounding quotes
    write_escaped_string(src, &mut result).expect("Failed to escape string");
    result
}

impl core::fmt::Display for Value {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Value::Null => f.write_str("null"),
            Value::Boolean(b) => f.write_str(if *b { "true" } else { "false" }),
            Value::Number(n) => {
                // Serialize numbers similar to serde_json::to_string.
                // `n` is finite by construction in our tests, so we can
                // safely use `to_string`.
                f.write_str(&alloc::string::ToString::to_string(&n))
            }
            Value::String(s) => {
                write!(f, "\"{}\"", escape_string(s))
            }
            Value::Array(arr) => {
                f.write_str("[")?;
                let mut first = true;
                for v in arr {
                    if !first {
                        f.write_str(",")?;
                    }
                    first = false;
                    write!(f, "{v}")?;
                }
                f.write_str("]")
            }
            Value::Object(map) => {
                f.write_str("{")?;
                let mut first = true;
                for (k, v) in map {
                    if !first {
                        f.write_str(",")?;
                    }
                    first = false;
                    write!(f, "\"{}\":{}", escape_string(k), v)?;
                }
                f.write_str("}")
            }
        }
    }
}
