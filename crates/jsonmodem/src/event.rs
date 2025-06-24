//! Types and events emitted by the streaming JSON parser.
//!
//! `ParseEvent` enumerates parser outputs, indicating the JSON value type and
//! its path within the document. `PathComponent` represents a key or index in a
//! JSON path.
//!
//! # Examples
//!
//! Basic streaming parse example:
//!
//! ```
//! use jsonmodem::{
//!     ParseEvent, ParserError, ParserOptions, PathComponent, StreamingParser, Value,
//! };
//!
//! let mut parser = StreamingParser::new(ParserOptions::default());
//! parser.feed("[\"foo\"]");
//! let events: Vec<_> = parser.finish().into_iter().collect();
//! assert_eq!(
//!     events,
//!     vec![
//!         Ok(ParseEvent::ArrayStart { path: vec![] }),
//!         Ok(ParseEvent::String {
//!             path: vec![PathComponent::Index(0)],
//!             value: None,
//!             fragment: "foo".to_string(),
//!             is_final: true,
//!         }),
//!         Ok(ParseEvent::ArrayEnd {
//!             path: vec![],
//!             value: None,
//!         }),
//!     ]
//! );
//! ```
use alloc::{
    string::{String, ToString},
    vec::Vec,
};

use crate::value::Value;

// Helper used solely by serde `skip_serializing_if` to omit `is_final` when it
// is `false`.
#[doc(hidden)]
#[cfg(any(test, feature = "serde"))]
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(b: &bool) -> bool {
    !*b
}

/// A component in the path to a JSON value.
///
/// Paths are sequences of keys or indices (for objects and arrays,
/// respectively) used in `ParseEvent` to indicate the location of a value
/// within a JSON document.
///
/// # Examples
///
/// ```
/// use jsonmodem::PathComponent;
///
/// let key = PathComponent::Key("foo".to_string());
/// assert_eq!(key.as_key(), Some(&"foo".to_string()));
///
/// let idx = PathComponent::Index(3);
/// assert_eq!(idx.as_index(), Some(&3));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum PathComponent {
    Key(String),
    Index(usize),
}

// Convenient conversions so users can write `path![0, "foo"]` etc.
macro_rules! impl_from_int_for_pathcomponent {
    ($($t:ty),*) => {
        $(
            impl From<$t> for PathComponent {
                fn from(i: $t) -> Self {
                    #[allow(clippy::cast_possible_truncation)]
                    PathComponent::Index(i as usize)
                }
            }
        )*
    };
}

impl_from_int_for_pathcomponent!(u8, u16, u32, u64, usize);

impl From<&str> for PathComponent {
    fn from(s: &str) -> Self {
        Self::Key(s.to_string())
    }
}

impl From<String> for PathComponent {
    fn from(s: String) -> Self {
        Self::Key(s)
    }
}

#[doc(hidden)]
pub trait PathComponentFrom<T> {
    fn from_path_component(value: T) -> PathComponent;
}

// use macro_rules to implement for i8..i64, u8..u64, isize, usize, &str and
// String
macro_rules! impl_integer_as_path_component {
    ($($t:ty),+) => {
        $(
            impl PathComponentFrom<$t> for PathComponent {
                fn from_path_component(value: $t) -> Self {
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    PathComponent::Index(value as usize)
                }
            }
        )+
    };
}
impl_integer_as_path_component!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

impl PathComponentFrom<&str> for PathComponent {
    fn from_path_component(value: &str) -> Self {
        PathComponent::Key(value.to_string())
    }
}

impl PathComponentFrom<String> for PathComponent {
    fn from_path_component(value: String) -> Self {
        PathComponent::Key(value)
    }
}

// Custom (de)serialization so that a `Vec<PathComponent>` becomes e.g.
// `["foo", 0, "bar"]` instead of the default tagged representation.
#[cfg(any(test, feature = "serde"))]
mod serde_impls {
    use alloc::string::{String, ToString};
    use core::fmt;

    use serde::{
        Deserialize, Deserializer, Serialize, Serializer,
        de::{Error, Unexpected, Visitor},
    };

    use super::PathComponent;

    impl Serialize for PathComponent {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            match self {
                PathComponent::Key(k) => serializer.serialize_str(k),
                PathComponent::Index(i) => serializer.serialize_u64(*i as u64),
            }
        }
    }

    struct PathComponentVisitor;

    impl Visitor<'_> for PathComponentVisitor {
        type Value = PathComponent;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a string or unsigned integer")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(PathComponent::Key(value.to_string()))
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(PathComponent::Key(value))
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: Error,
        {
            #[allow(clippy::cast_possible_truncation)]
            Ok(PathComponent::Index(value as usize))
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: Error,
        {
            if value < 0 {
                return Err(Error::invalid_value(
                    Unexpected::Signed(value),
                    &"non-negative index",
                ));
            }

            #[allow(clippy::cast_sign_loss)]
            #[allow(clippy::cast_possible_truncation)]
            Ok(PathComponent::Index(value as usize))
        }
    }

    impl<'de> Deserialize<'de> for PathComponent {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserializer.deserialize_any(PathComponentVisitor)
        }
    }
}

impl PathComponent {
    #[must_use]
    /// Returns the index if this component is an index, otherwise `None`.
    pub fn as_index(&self) -> Option<&usize> {
        if let Self::Index(v) = self {
            Some(v)
        } else {
            None
        }
    }

    #[must_use]
    /// Returns the key if this component is a key, otherwise `None`.
    pub fn as_key(&self) -> Option<&String> {
        if let Self::Key(v) = self {
            Some(v)
        } else {
            None
        }
    }
}

/// An event generated by the streaming JSON parser.
///
/// Represents a JSON parsing event, indicating the type of value, any relevant
/// data, and the path to that value within the document.
///
/// The `path` is a sequence of `PathComponent` starting at the root. For
/// example, the first element in an array has path `[PathComponent::Index(0)]`.
///
/// # Examples
///
/// ```
/// use jsonmodem::{ParseEvent, PathComponent, Value};
///
/// let evt = ParseEvent::Null { path: Vec::new() };
/// assert_eq!(evt, ParseEvent::Null { path: Vec::new() });
/// ```
#[cfg_attr(
    any(test, feature = "serde"),
    derive(serde::Serialize, serde::Deserialize)
)]
#[cfg_attr(any(test, feature = "serde"), serde(tag = "kind"))]
#[derive(Debug, Clone, PartialEq)]
pub enum ParseEvent {
    /// A JSON `null` value.
    Null {
        /// The path to the value.
        path: Vec<PathComponent>,
    },
    /// A JSON `true` or `false` value.
    Boolean {
        /// The path to the value.
        path: Vec<PathComponent>,
        /// The boolean value.
        value: bool,
    },
    /// A JSON number value.
    ///
    /// TODO: Support an `arbitrary_precision` feature that allows this value to
    /// be an arbitrarily large.
    Number {
        /// The path to the value.
        path: Vec<PathComponent>,
        /// The number value.
        value: f64,
    },
    /// A JSON string value.
    String {
        /// The path to the string value.
        path: Vec<PathComponent>,
        /// The value of the string. The interpretation of this value depends on
        /// the `string_value_mode` used to create the parser.
        ///
        /// This value is not set when the mode is `StringValueMode::None`.
        #[cfg_attr(
            any(test, feature = "serde"),
            serde(skip_serializing_if = "Option::is_none")
        )]
        value: Option<String>,
        /// A fragment of a string value.
        fragment: String,
        /// Whether this is the final fragment of a string value. Implied when
        /// `value` is set.
        #[cfg_attr(
            any(test, feature = "serde"),
            serde(skip_serializing_if = "crate::event::is_false")
        )]
        is_final: bool,
    },
    /// Marks the start of a JSON array.
    ArrayStart {
        /// The path to the value.
        path: Vec<PathComponent>,
    },
    /// Marks the end of a JSON array, optionally including its value.
    ArrayEnd {
        /// The path to the value.
        path: Vec<PathComponent>,
        /// The value of the array.
        ///
        /// This value is not set when option `emit_non_scalar_values` is false.
        #[cfg_attr(
            any(test, feature = "serde"),
            serde(skip_serializing_if = "Option::is_none")
        )]
        value: Option<Vec<Value>>,
    },
    /// Marks the start of a JSON object.
    ObjectBegin {
        /// The path to the value.
        path: Vec<PathComponent>,
    },
    /// Marks the end of a JSON object, optionally including its value.
    ObjectEnd {
        /// The path to the value.
        path: Vec<PathComponent>,
        /// The value of the object.
        ///
        /// This value is not set when option `emit_non_scalar_values` is false.
        #[cfg_attr(
            any(test, feature = "serde"),
            serde(skip_serializing_if = "Option::is_none")
        )]
        value: Option<BTreeMap<String, Value>>,
    },
}

use alloc::collections::BTreeMap;

/// Reconstructs the fully materialised JSON root values from a stream of
/// `ParseEvent`s.
///
/// The returned vector contains **one entry per root** in the order they
/// appeared in the input. For example `1 2 [3]` yields `[Value::Number(1.0),
/// Value::Number(2.0), Value::Array([3])]`.
///
/// ---
///
/// The streaming parser purposefully avoids building up complete `Value` trees
/// while it tokenises the input.  For use-cases that need the fully
/// materialised document (e.g. property-based round- trip tests) the crate
/// exposes a small, allocation-friendly helper that rebuilds one or more
/// `Value`s from the flat `ParseEvent` stream.
///
/// The algorithm is deliberately simple:
/// 1. Maintain a single mutable `Value` representing the *current* root that is
///    under construction.  A `Vec<Value>` is used to collect finished roots so
///    that multi-value streams like `1 2 3` are supported.
/// 2. For every `StartArray` / `StartObject` and `PrimitiveValue` event we
///    *insert* the matching placeholder / leaf at the provided `path`.  A
///    lightweight `insert_at_path` routine grows intermediate structures
///    on-demand and resizes arrays when necessary.
/// 3. A root is considered *finished* when we receive either
///
///     - `PrimitiveValue` with an empty `path` (primitive roots), or
///     - `EndArray` / `EndObject` with an empty `path` (composite roots).
///
///    At that point a clone of the currently built value is pushed onto the
///    result vector and the buffer is reset.
///
/// This avoids any expensive deep copies – only the final `clone()` at root
/// completion is required and unavoidable because the caller may retain the
/// returned list while more events are fed in.
#[cfg(test)]
pub fn reconstruct_values<I>(events: I) -> Vec<Value>
where
    I: IntoIterator<Item = ParseEvent>,
{
    use crate::value::Map;

    let mut finished_roots = Vec::new();
    let mut current_root = Value::Null;
    let mut building_root = false;

    for evt in events {
        match &evt {
            // ----------------------------------------------------------------------------------
            // Container open – insert an empty placeholder so that later children have a slot to
            // land in.
            ParseEvent::ArrayStart { path } => {
                insert_at_path(&mut current_root, path, Value::Array(Vec::new()));
                if path.is_empty() {
                    building_root = true;
                }
            }
            ParseEvent::ObjectBegin { path } => {
                insert_at_path(&mut current_root, path, Value::Object(Map::new()));
                if path.is_empty() {
                    building_root = true;
                }
            }

            // ----------------------------------------------------------------------------------
            // Leaf value – insert at its destination path.  If the path is empty we finish the
            // root.
            ParseEvent::Null { path } => {
                insert_at_path(&mut current_root, path, Value::Null);
                if path.is_empty() {
                    finished_roots.push(Value::Null);
                    current_root = Value::Null;
                    building_root = false;
                }
            }
            ParseEvent::Boolean { path, value } => {
                insert_at_path(&mut current_root, path, Value::Boolean(*value));
                if path.is_empty() {
                    finished_roots.push(Value::Boolean(*value));
                    current_root = Value::Null;
                    building_root = false;
                }
            }
            ParseEvent::Number { path, value } => {
                insert_at_path(&mut current_root, path, Value::Number(*value));
                if path.is_empty() {
                    finished_roots.push(Value::Number(*value));
                    current_root = Value::Null;
                    building_root = false;
                }
            }
            // ----------------------------------------------------------------------------------
            // Streaming string fragments – accumulate string content and start a root on first
            // fragment.
            ParseEvent::String {
                path,
                value,
                fragment,
                is_final,
                ..
            } => {
                if let Some(value) = value {
                    insert_at_path(&mut current_root, path, Value::String(value.clone()));
                } else {
                    // Append or insert string fragment at the given path
                    append_string_at_path(&mut current_root, path, fragment);
                }

                if *is_final && path.is_empty() {
                    finished_roots.push(current_root.clone());
                    current_root = Value::Null;
                    building_root = false;
                } else if path.is_empty() {
                    building_root = true;
                }
            }

            // ----------------------------------------------------------------------------------
            // Container close – push the fully built root when the closed container sits at the top
            // level.
            ParseEvent::ArrayEnd { path, .. } | ParseEvent::ObjectEnd { path, .. } => {
                if path.is_empty() && building_root {
                    finished_roots.push(current_root.clone());
                    current_root = Value::Null;
                    building_root = false;
                }
            }
        }
    }

    // If a top-level string (or other root) was started but never terminated via
    // Complete, treat it as finished at end-of-stream.
    if building_root {
        finished_roots.push(current_root);
    }
    finished_roots
}

#[cfg(test)]
/// Inserts `val` into `target` at the location described by `path`, creating
/// intermediate containers as necessary.  When the final path component denotes
/// an array index the underlying vector is automatically resized (filled with
/// `Value::Null`).
fn insert_at_path(target: &mut Value, path: &[PathComponent], val: Value) {
    use crate::value::Map;

    if path.is_empty() {
        *target = val;
        return;
    }

    let mut current = target;
    // Traverse all but the last component, creating intermediate containers
    // on-demand.
    for comp in &path[..path.len() - 1] {
        match comp {
            PathComponent::Key(k) => {
                if let Value::Object(map) = current {
                    current = map.entry(k.clone()).or_insert(Value::Null);
                } else {
                    *current = Value::Object(Map::new());
                    if let Value::Object(map) = current {
                        current = map.entry(k.clone()).or_insert(Value::Null);
                    }
                }
            }
            PathComponent::Index(i) => {
                if let Value::Array(vec) = current {
                    if *i >= vec.len() {
                        vec.resize(*i + 1, Value::Null);
                    }
                    current = &mut vec[*i];
                } else {
                    *current = Value::Array(Vec::new());
                    if let Value::Array(vec) = current {
                        if *i >= vec.len() {
                            vec.resize(*i + 1, Value::Null);
                        }
                        current = &mut vec[*i];
                    }
                }
            }
        }
    }

    // Set the final component.
    match path.last().unwrap() {
        PathComponent::Key(k) => {
            if let Value::Object(map) = current {
                map.insert(k.clone(), val);
            } else {
                // Replace the current slot with a new object containing the desired key/value.
                let mut map = Map::new();
                map.insert(k.clone(), val);
                *current = Value::Object(map);
            }
        }
        PathComponent::Index(i) => {
            if let Value::Array(vec) = current {
                if *i >= vec.len() {
                    vec.resize(*i + 1, Value::Null);
                }
                vec[*i] = val;
            } else {
                let mut vec = Vec::new();
                if *i >= vec.len() {
                    vec.resize(*i + 1, Value::Null);
                }
                vec[*i] = val;
                *current = Value::Array(vec);
            }
        }
    }
}

#[cfg(test)]
/// Insert or append a string fragment into `target` at the given `path`.
fn append_string_at_path(target: &mut Value, path: &[PathComponent], fragment: &str) {
    use crate::value::Map;

    if path.is_empty() {
        if let Value::String(s) = target {
            s.push_str(fragment);
        } else {
            *target = Value::String(String::from(fragment));
        }
        return;
    }
    let mut cur = target;
    // Traverse to the container for the final component
    for comp in &path[..path.len() - 1] {
        match comp {
            PathComponent::Key(k) => {
                if let Value::Object(map) = cur {
                    cur = map.entry(k.clone()).or_insert(Value::Null);
                } else {
                    *cur = Value::Object(Map::new());
                    if let Value::Object(map) = cur {
                        cur = map.entry(k.clone()).or_insert(Value::Null);
                    }
                }
            }
            PathComponent::Index(i) => {
                if let Value::Array(vec) = cur {
                    if *i >= vec.len() {
                        vec.resize(*i + 1, Value::Null);
                    }
                    cur = &mut vec[*i];
                } else {
                    *cur = Value::Array(Vec::new());
                    if let Value::Array(vec) = cur {
                        if *i >= vec.len() {
                            vec.resize(*i + 1, Value::Null);
                        }
                        cur = &mut vec[*i];
                    }
                }
            }
        }
    }
    // Append or insert at the final component
    match path.last().unwrap() {
        PathComponent::Key(k) => {
            if let Value::Object(map) = cur {
                if let Some(Value::String(s)) = map.get_mut(k) {
                    s.push_str(fragment);
                } else {
                    map.insert(k.clone(), Value::String(String::from(fragment)));
                }
            } else {
                let mut map = Map::new();
                map.insert(k.clone(), Value::String(String::from(fragment)));
                *cur = Value::Object(map);
            }
        }
        PathComponent::Index(i) => {
            if let Value::Array(vec) = cur {
                if *i < vec.len() {
                    if let Value::String(s) = &mut vec[*i] {
                        s.push_str(fragment);
                    } else {
                        vec[*i] = Value::String(String::from(fragment));
                    }
                } else {
                    vec.resize(*i + 1, Value::Null);
                    vec[*i] = Value::String(String::from(fragment));
                }
            } else {
                let mut vec = Vec::new();
                if *i >= vec.len() {
                    vec.resize(*i + 1, Value::Null);
                }
                vec[*i] = Value::String(String::from(fragment));
                *cur = Value::Array(vec);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_of_path_component() {
        use core::mem::size_of;
        assert_eq!(size_of::<PathComponent>(), 24);
    }

    #[test]
    fn size_of_parse_event() {
        use core::mem::size_of;
        assert_eq!(size_of::<ParseEvent>(), 80);
    }
}
