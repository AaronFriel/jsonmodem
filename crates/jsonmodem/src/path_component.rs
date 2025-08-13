use alloc::sync::Arc;

use crate::json_path::Successor;

pub type Key = Arc<str>;
pub type Index = usize;

impl Successor for Index {
    fn successor(&self) -> Index {
        *self + 1
    }
}

/// A component in the path to a JSON value.
///
/// Paths are sequences of keys or indices (for objects and arrays,
/// respectively) used in `ParseEvent` to indicate the location of a value
/// within a JSON document.
#[derive(Debug, Clone, PartialEq)]
pub enum PathComponent<K = Key, I = Index> {
    Key(K),
    Index(I),
}

// Convenient conversions so users can write `path![0, "foo"]` etc.
macro_rules! impl_from_int_for_pathcomponent {
    ($($t:ty),*) => {
        $(
            impl From<$t> for PathComponent {
                fn from(i: $t) -> Self {
                    #[allow(clippy::cast_possible_truncation)]
                    PathComponent::Index(i as Index)
                }
            }
        )*
    };
}

impl_from_int_for_pathcomponent!(u8, u16, u32, u64, usize);

impl From<&str> for PathComponent {
    fn from(s: &str) -> Self {
        Self::Key(s.into())
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
                    PathComponent::Index(value as Index)
                }
            }
        )+
    };
}
impl_integer_as_path_component!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

impl PathComponentFrom<&str> for PathComponent {
    fn from_path_component(value: &str) -> Self {
        PathComponent::Key(value.into())
    }
}

// Custom (de)serialization so that a `Vec<PathComponent>` becomes e.g.
// `["foo", 0, "bar"]` instead of the default tagged representation.
#[cfg(any(test, feature = "serde"))]
mod serde_impls {
    use alloc::string::String;
    use core::fmt;

    use serde::{
        Deserialize, Deserializer, Serialize, Serializer,
        de::{Error, Unexpected, Visitor},
    };

    use super::PathComponent;
    use crate::Index;

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
            Ok(PathComponent::Key(value.into()))
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(PathComponent::Key(value.into()))
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: Error,
        {
            #[expect(clippy::cast_possible_truncation)]
            Ok(PathComponent::Index(value as Index))
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

            #[expect(clippy::cast_sign_loss)]
            #[expect(clippy::cast_possible_truncation)]
            Ok(PathComponent::Index(value as Index))
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
    pub fn as_index(&self) -> Option<Index> {
        if let Self::Index(v) = self {
            Some(*v)
        } else {
            None
        }
    }

    #[must_use]
    /// Returns the key if this component is a key, otherwise `None`.
    pub fn as_key(&self) -> Option<Key> {
        if let Self::Key(v) = self {
            Some(v.clone())
        } else {
            None
        }
    }
}
