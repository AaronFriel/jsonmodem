use alloc::{sync::Arc, vec::Vec};
use core::fmt::Debug;

/// A path to a JSON value.
pub type Path = Vec<PathItem<Arc<str>, usize>>;

/// A by-value view of a path component.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PathItem<K = Arc<str>, I = usize> {
    /// An index into a JSON object.
    Key(K),
    /// An index into a JSON array.
    Index(I),
}

/// Read-only path API used by adapters. No mutation here.
pub trait PathLike<K, I> {
    /// Push a key component onto the path.
    fn push_key(&mut self, k: K);

    /// Push an index component onto the path.
    fn push_index(&mut self, i: I);

    fn pop(&mut self) -> Option<PathItem<K, I>>;

    /// Last component, if any.
    fn last(&self) -> Option<PathItem<&K, &I>>;
}

impl PathLike<Arc<str>, usize> for Path {
    fn push_key(&mut self, k: Arc<str>) {
        self.push(PathItem::Key(k));
    }

    fn push_index(&mut self, i: usize) {
        self.push(PathItem::Index(i));
    }

    fn pop(&mut self) -> Option<PathItem<Arc<str>, usize>> {
        self.pop()
    }

    fn last(&self) -> Option<PathItem<&Arc<str>, &usize>> {
        match self.as_slice().last() {
            Some(PathItem::Key(k)) => Some(PathItem::Key(k)),
            Some(PathItem::Index(i)) => Some(PathItem::Index(i)),
            None => None,
        }
    }
}

impl<K, I> From<&str> for PathItem<K, I>
where
    K: for<'a> From<&'a str>,
{
    fn from(s: &str) -> Self {
        Self::Key(s.into())
    }
}

impl<K, I> From<usize> for PathItem<K, I>
where
    I: From<usize>,
{
    fn from(s: usize) -> Self {
        Self::Index(s.into())
    }
}

#[doc(hidden)]
pub trait PathItemFrom<K, I, T> {
    fn from_path_component(value: T) -> PathItem<K, I>;
}

// use macro_rules to implement for i8..i64, u8..u64, isize, usize, &str and
// String
macro_rules! impl_unsigned_as_path_component {
    ($($t:ty),+) => {
        $(
            impl<K, I> PathItemFrom<K, I, $t> for PathItem<K, I> where I: From<usize> {
                fn from_path_component(value: $t) -> Self {
                    #[allow(clippy::cast_lossless)]
                    PathItem::Index((value as usize).into())
                }
            }
        )+
    };
}
impl_unsigned_as_path_component!(u8, u16, u32, u64, usize);

macro_rules! impl_signed_as_path_component {
    ($($t:ty),+) => {
        $(
            impl<K, I> PathItemFrom<K, I, $t> for PathItem<K, I>
            where
                I: From<usize>,
            {
                fn from_path_component(value: $t) -> Self {
                    #[allow(clippy::cast_sign_loss)]
                    PathItem::Index((value.max(0) as usize).into())
                }
            }
        )+
    };
}
impl_signed_as_path_component!(i8, i16, i32, i64, isize);

impl<K, I> PathItemFrom<K, I, &str> for PathItem<K, I>
where
    K: for<'a> From<&'a str>,
{
    fn from_path_component(value: &str) -> Self {
        PathItem::Key(value.into())
    }
}

#[cfg(test)]
mod test {
    use alloc::{string::String, vec};

    use super::*;
    use crate::path;

    #[test]
    fn test_path_item_from() {
        let key: PathItem<String, usize> = PathItem::from_path_component("test");
        assert_eq!(key, PathItem::Key("test".into()));
        let index: PathItem<String, usize> = PathItem::from_path_component(8u8);
        assert_eq!(index, PathItem::Index(8usize));
    }

    #[test]
    fn test_path_item_macro() {
        let p: Path = path![0, "foo", 2];
        assert_eq!(
            p,
            vec![
                PathItem::Index(0),
                PathItem::Key("foo".into()),
                PathItem::Index(2)
            ]
        );
    }
}
