use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use core::{fmt, fmt::Write as _};

#[cfg(test)]
use quickcheck::{Arbitrary, Gen};

/// In-memory representation of JSON values used by buffered adapters.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// JSON null.
    Null,
    /// JSON boolean value.
    Boolean(bool),
    /// JSON number value represented as `f64`.
    Number(f64),
    /// JSON string value.
    String(String),
    /// JSON array value.
    Array(Vec<Value>),
    /// JSON object value.
    Object(BTreeMap<Arc<str>, Value>),
}

impl Value {
    /// Returns the contained boolean when this value is a boolean.
    #[must_use]
    pub fn as_boolean(&self) -> Option<&bool> {
        if let Self::Boolean(value) = self {
            Some(value)
        } else {
            None
        }
    }

    /// Returns the contained number when this value is a number.
    #[must_use]
    pub fn as_number(&self) -> Option<&f64> {
        if let Self::Number(value) = self {
            Some(value)
        } else {
            None
        }
    }

    /// Returns the contained string reference when this value is a string.
    #[must_use]
    pub fn as_string(&self) -> Option<&String> {
        if let Self::String(value) = self {
            Some(value)
        } else {
            None
        }
    }

    /// Returns the contained array when this value is an array.
    #[must_use]
    pub fn as_array(&self) -> Option<&Vec<Value>> {
        if let Self::Array(values) = self {
            Some(values)
        } else {
            None
        }
    }

    /// Returns a mutable reference to the array when this value is an array.
    #[must_use]
    pub fn as_array_mut(&mut self) -> Option<&mut Vec<Value>> {
        if let Self::Array(values) = self {
            Some(values)
        } else {
            None
        }
    }

    /// Returns the contained object when this value is an object.
    #[must_use]
    pub fn as_object(&self) -> Option<&BTreeMap<Arc<str>, Value>> {
        if let Self::Object(map) = self {
            Some(map)
        } else {
            None
        }
    }

    /// Returns a mutable reference to the object when this value is an object.
    #[must_use]
    pub fn as_object_mut(&mut self) -> Option<&mut BTreeMap<Arc<str>, Value>> {
        if let Self::Object(map) = self {
            Some(map)
        } else {
            None
        }
    }
}

/// Escapes a string and writes it into the supplied formatter.
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

/// Returns the escaped representation of a string.
#[must_use]
pub fn escape_string(src: &str) -> String {
    let mut escaped = String::with_capacity(src.len());
    write_escaped_string(src, &mut escaped).expect("escape_string write failure");
    escaped
}

impl core::fmt::Display for Value {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Null => f.write_str("null"),
            Self::Boolean(value) => f.write_str(if *value { "true" } else { "false" }),
            Self::Number(value) => f.write_str(&value.to_string()),
            Self::String(value) => {
                write!(f, "\"")?;
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
impl quickcheck::Arbitrary for Value {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        match usize::arbitrary(g) % 6 {
            0 => Value::Null,
            1 => Value::Boolean(bool::arbitrary(g)),
            2 => {
                let mut number = f64::arbitrary(g);
                while !number.is_finite() {
                    number = f64::arbitrary(g);
                }
                number = number.rem_euclid(1_000_000.0);
                Value::Number(number)
            }
            3 => Value::String(random_ascii_string(g)),
            4 => {
                let len = usize::arbitrary(g) % 4;
                let mut items = Vec::with_capacity(len);
                for _ in 0..len {
                    items.push(Value::arbitrary(g));
                }
                Value::Array(items)
            }
            _ => {
                let len = usize::arbitrary(g) % 4;
                let mut map = BTreeMap::new();
                for _ in 0..len {
                    map.insert(Arc::from(random_ascii_string(g)), Value::arbitrary(g));
                }
                Value::Object(map)
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
