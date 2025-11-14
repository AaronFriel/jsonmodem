use alloc::string::{String, ToString};
use core::{fmt, fmt::Write as _};

use ecow::EcoString;
#[cfg(test)]
use quickcheck::{Arbitrary, Gen};
use rpds::{RedBlackTreeMapSync, VectorSync};

/// Owned string type for the immutable backend (`EcoString` with COW
/// semantics).
pub type Str = EcoString;
/// Default numeric representation (`f64`) used by the immutable backend.
pub type Number = f64;
/// Persistent vector storing array elements with structural sharing.
pub type Array = VectorSync<Value>;
/// Persistent red-black tree map storing object members keyed by `Str`.
pub type Map = RedBlackTreeMapSync<Str, Value>;

/// Immutable JSON value backed by persistent data structures.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// JSON `null`.
    Null,
    /// JSON boolean.
    Boolean(bool),
    /// JSON number.
    Number(Number),
    /// JSON string.
    String(Str),
    /// JSON array.
    Array(Array),
    /// JSON object.
    Object(Map),
}

impl Value {
    /// Returns the boolean payload if this value is a `Boolean`.
    #[must_use]
    pub fn as_boolean(&self) -> Option<&bool> {
        match self {
            Self::Boolean(value) => Some(value),
            _ => None,
        }
    }

    /// Returns the numeric payload if this value is a `Number`.
    #[must_use]
    pub fn as_number(&self) -> Option<&Number> {
        match self {
            Self::Number(value) => Some(value),
            _ => None,
        }
    }

    /// Returns the string payload if this value is a `String`.
    #[must_use]
    pub fn as_string(&self) -> Option<&Str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    /// Returns the array payload if this value is an `Array`.
    #[must_use]
    pub fn as_array(&self) -> Option<&Array> {
        match self {
            Self::Array(values) => Some(values),
            _ => None,
        }
    }

    /// Returns the object payload if this value is an `Object`.
    #[must_use]
    pub fn as_object(&self) -> Option<&Map> {
        match self {
            Self::Object(map) => Some(map),
            _ => None,
        }
    }
}

/// Writes a JSON string literal to `f`, escaping characters as needed.
///
/// # Errors
///
/// Returns any formatting error emitted by `f` while writing the escaped
/// string.
pub fn write_escaped_string(src: &str, f: &mut impl fmt::Write) -> fmt::Result {
    for ch in src.chars() {
        match ch {
            '"' => f.write_str("\\\"")?,
            '\\' => f.write_str("\\\\")?,
            '\u{2028}' | '\u{2029}' => {
                write!(f, "\\u{:04X}", ch as u32)?;
            }
            c if c.is_ascii_control() || (c.is_control() && (c as u32) <= 0xFFFF) => {
                write!(f, "\\u{:04X}", c as u32)?;
            }
            _ => f.write_char(ch)?,
        }
    }
    Ok(())
}

/// Returns a JSON-escaped string suitable for embedding in object keys.
///
/// # Panics
///
/// Panics only if writing to the internal buffer fails, which should be
/// unreachable because `String`'s formatter implementations cannot error.
#[must_use]
pub fn escape_string(src: &str) -> String {
    let mut escaped = String::with_capacity(src.len());
    write_escaped_string(src, &mut escaped).expect("escape_string write failure");
    escaped
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => f.write_str("null"),
            Self::Boolean(value) => f.write_str(if *value { "true" } else { "false" }),
            Self::Number(value) => f.write_str(&value.to_string()),
            Self::String(value) => {
                f.write_str("\"")?;
                write_escaped_string(value, f)?;
                f.write_char('"')
            }
            Self::Array(values) => {
                f.write_str("[")?;
                let mut first = true;
                for value in values {
                    if !first {
                        f.write_str(",")?;
                    }
                    first = false;
                    write!(f, "{value}")?;
                }
                f.write_str("]")
            }
            Self::Object(map) => {
                f.write_str("{")?;
                let mut first = true;
                for (key, value) in map {
                    if !first {
                        f.write_str(",")?;
                    }
                    first = false;
                    write!(f, "\"{}\":{value}", escape_string(key))?;
                }
                f.write_str("}")
            }
        }
    }
}

#[cfg(test)]
impl Arbitrary for Value {
    fn arbitrary(g: &mut Gen) -> Self {
        match usize::arbitrary(g) % 6 {
            0 => Self::Null,
            1 => Self::Boolean(bool::arbitrary(g)),
            2 => {
                let mut number = f64::arbitrary(g);
                while !number.is_finite() {
                    number = f64::arbitrary(g);
                }
                number = number.rem_euclid(1_000_000.0);
                Self::Number(number)
            }
            3 => Self::String(random_ascii_string(g).as_str().into()),
            4 => {
                let len = usize::arbitrary(g) % 4;
                let mut items = Array::new_sync();
                for _ in 0..len {
                    items.push_back_mut(Self::arbitrary(g));
                }
                Self::Array(items)
            }
            _ => {
                let len = usize::arbitrary(g) % 4;
                let mut map = Map::new_sync();
                for _ in 0..len {
                    map.insert_mut(Str::from(random_ascii_string(g)), Self::arbitrary(g));
                }
                Self::Object(map)
            }
        }
    }
}

#[cfg(test)]
fn random_ascii_string(g: &mut Gen) -> String {
    let len = usize::arbitrary(g) % 20;
    (0..len)
        .map(|_| char::from(b'_' + (u8::arbitrary(g) % (b'z' - b'_'))))
        .collect()
}
