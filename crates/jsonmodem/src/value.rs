//! JSON value types and utilities.
//!
//! This module defines the [`Value`] enum, which represents any valid JSON
//! value, and provides helper functions for escaping JSON strings.
#![allow(clippy::inline_always)]
use alloc::{borrow::ToOwned, collections::BTreeMap, string::String, vec::Vec};

use crate::JsonFactory;

pub type Map = BTreeMap<String, Value>;
pub type Array = Vec<Value>;

/// A JSON value as defined by [RFC 8259].
///
/// The `Value` enum can represent any JSON data type:
///
/// - Null
/// - Boolean
/// - Number
/// - String
/// - Array
/// - Object
///
/// # Examples
///
/// ```
/// use jsonmodem::{Map, Value};
///
/// // Creating a JSON object:
/// let mut map = Map::new();
/// map.insert("key".to_string(), Value::String("value".into()));
/// let v = Value::Object(map);
/// assert_eq!(v.to_string(), r#"{"key":"value"}"#);
/// ```
///
/// [RFC 8259]: https://datatracker.ietf.org/doc/html/rfc8259
// Enable serde support for tests and when the optional `serde` feature is
// activated by downstream crates.  The `cfg_attr` conditional keeps the core
// crate free of a serde dependency in normal builds.
#[cfg_attr(
    any(test, feature = "serde"),
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Boolean(bool),
    Number(f64),
    String(String),
    Array(Array),
    Object(Map),
}

impl Default for Value {
    fn default() -> Self {
        Self::Null
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Self::Boolean(v)
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Self::Number(v)
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Self::String(v)
    }
}

impl From<Vec<Value>> for Value {
    fn from(v: Vec<Value>) -> Self {
        Self::Array(v)
    }
}

impl From<BTreeMap<String, Value>> for Value {
    fn from(v: BTreeMap<String, Value>) -> Self {
        Self::Object(v)
    }
}

impl Value {
    /// Returns `true` if the value is [`Null`].
    ///
    /// [`Null`]: Value::Null
    ///
    /// # Examples
    ///
    /// ```
    /// use jsonmodem::Value;
    ///
    /// assert!(Value::Null.is_null());
    /// assert!(!Value::Boolean(false).is_null());
    /// ```
    #[must_use]
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Returns `true` if the value is [`Boolean`].
    ///
    /// [`Boolean`]: Value::Boolean
    ///
    /// # Examples
    ///
    /// ```
    /// use jsonmodem::Value;
    ///
    /// assert!(Value::Boolean(true).is_bool());
    /// assert!(!Value::Null.is_bool());
    /// ```
    #[must_use]
    pub fn is_bool(&self) -> bool {
        matches!(self, Self::Boolean(..))
    }

    /// Returns `true` if the value is [`Number`].
    ///
    /// [`Number`]: Value::Number
    ///
    /// # Examples
    ///
    /// ```
    /// use jsonmodem::Value;
    ///
    /// assert!(Value::Number(42.0).is_number());
    /// assert!(!Value::Null.is_number());
    /// ```
    #[must_use]
    pub fn is_number(&self) -> bool {
        matches!(self, Self::Number(..))
    }

    /// Returns `true` if the value is [`String`].
    ///
    /// [`String`]: Value::String
    ///
    /// # Examples
    ///
    /// ```
    /// use jsonmodem::Value;
    ///
    /// assert!(Value::String("foo".into()).is_string());
    /// assert!(!Value::Null.is_string());
    /// ```
    #[must_use]
    pub fn is_string(&self) -> bool {
        matches!(self, Self::String(..))
    }

    /// Returns `true` if the value is [`Array`].
    ///
    /// [`Array`]: Value::Array
    ///
    /// # Examples
    ///
    /// ```
    /// use jsonmodem::Value;
    ///
    /// assert!(Value::Array(vec![Value::Null]).is_array());
    /// assert!(!Value::Null.is_array());
    /// ```
    #[must_use]
    pub fn is_array(&self) -> bool {
        matches!(self, Self::Array(..))
    }

    /// Returns `true` if the value is [`Object`].
    ///
    /// [`Object`]: Value::Object
    ///
    /// # Examples
    ///
    /// ```
    /// use jsonmodem::{Map, Value};
    ///
    /// let map = Map::new();
    /// let v = Value::Object(map);
    /// assert!(v.is_object());
    /// assert!(!Value::Null.is_object());
    /// ```
    #[must_use]
    pub fn is_object(&self) -> bool {
        matches!(self, Self::Object(..))
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

impl JsonFactory for Value {
    type Str = String;
    type Num = f64;
    type Bool = bool;
    type Null = ();
    type Array = Vec<Value>;
    type Object = BTreeMap<String, Value>;

    #[inline(always)]
    fn new_null() {
        // unit type has a default return
    }

    #[inline(always)]
    fn new_bool(b: bool) -> bool {
        b
    }

    #[inline(always)]
    fn new_number(n: f64) -> f64 {
        n
    }

    #[inline(always)]
    fn new_string(s: String) -> String {
        s
    }

    #[inline(always)]
    fn new_array() -> Vec<Value> {
        Vec::new()
    }

    #[inline(always)]
    fn new_object() -> BTreeMap<String, Value> {
        BTreeMap::new()
    }

    #[inline(always)]
    fn push_array(array: &mut Vec<Value>, val: Self) {
        array.push(val);
    }

    #[inline(always)]
    fn insert_object(obj: &mut BTreeMap<String, Value>, key: &str, val: Self) {
        obj.insert(key.to_owned(), val);
    }

    #[inline(always)]
    fn from_str(s: Self::Str) -> Value {
        Value::String(s)
    }

    #[inline(always)]
    fn from_num(n: Self::Num) -> Value {
        Value::Number(n)
    }

    #[inline(always)]
    fn from_bool(b: Self::Bool) -> Value {
        Value::Boolean(b)
    }

    #[inline(always)]
    fn from_null(_n: ()) -> Value {
        Value::Null
    }

    #[inline(always)]
    fn from_array(a: Vec<Value>) -> Value {
        Value::Array(a)
    }

    #[inline(always)]
    fn from_object(o: BTreeMap<String, Value>) -> Value {
        Value::Object(o)
    }

    #[inline(always)]
    fn kind(v: &Self) -> crate::factory::ValueKind {
        match v {
            Value::Null => crate::factory::ValueKind::Null,
            Value::Boolean(_) => crate::factory::ValueKind::Bool,
            Value::Number(_) => crate::factory::ValueKind::Num,
            Value::String(_) => crate::factory::ValueKind::Str,
            Value::Array(_) => crate::factory::ValueKind::Array,
            Value::Object(_) => crate::factory::ValueKind::Object,
        }
    }

    #[inline(always)]
    fn push_string(string: &mut Self::Str, val: &Self::Str) {
        string.push_str(val);
    }

    #[inline(always)]
    fn push_str(string: &mut Self::Str, val: &str) {
        string.push_str(val);
    }

    #[inline(always)]
    fn as_string_mut(v: &mut Self) -> Option<&mut String> {
        if let Value::String(s) = v {
            Some(s)
        } else {
            None
        }
    }

    #[inline(always)]
    fn as_array_mut(v: &mut Self) -> Option<&mut Vec<Value>> {
        if let Value::Array(a) = v {
            Some(a)
        } else {
            None
        }
    }

    #[inline(always)]
    fn as_object_mut(v: &mut Self) -> Option<&mut BTreeMap<String, Value>> {
        if let Value::Object(o) = v {
            Some(o)
        } else {
            None
        }
    }

    fn into_array(v: Self) -> Option<Vec<Value>>
    where
        Self: Sized,
    {
        if let Value::Array(arr) = v {
            Some(arr)
        } else {
            None
        }
    }

    fn into_object(v: Self) -> Option<BTreeMap<String, Value>>
    where
        Self: Sized,
    {
        if let Value::Object(obj) = v {
            Some(obj)
        } else {
            None
        }
    }

    #[inline(always)]
    fn object_get_mut<'a>(
        obj: &'a mut BTreeMap<String, Value>,
        key: &str,
    ) -> Option<&'a mut Value> {
        obj.get_mut(key)
    }

    #[inline(always)]
    fn object_insert(obj: &mut BTreeMap<String, Value>, key: String, val: Value) -> &mut Value {
        use alloc::collections::btree_map::Entry;

        match obj.entry(key) {
            Entry::Occupied(occ) => {
                let slot = occ.into_mut();
                *slot = val;
                slot
            }
            Entry::Vacant(slot) => slot.insert(val),
        }
    }

    #[inline(always)]
    fn array_get_mut(arr: &mut Vec<Value>, idx: usize) -> Option<&mut Value> {
        arr.get_mut(idx)
    }

    #[inline(always)]
    fn array_push(arr: &mut Vec<Value>, val: Value) -> &mut Value {
        arr.push(val);
        arr.last_mut().expect("just pushed")
    }

    #[inline(always)]
    fn array_len(arr: &Vec<Value>) -> usize {
        arr.len()
    }
}
