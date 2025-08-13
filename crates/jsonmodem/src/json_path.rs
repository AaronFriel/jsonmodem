use core::{fmt::Debug, iter::Iterator};

use crate::PathComponent;

/// Abstraction over how JSON paths are stored and mutated.
///
/// This trait allows callers to provide custom path representations while the
/// parser operates on the trait object generically.
pub trait JsonPath: Clone + Default + Debug {
    /// Type used to represent object property keys.
    type Key: Clone + Debug + for<'a> From<&'a str>;
    /// Type used to represent array indices.
    type Index: Copy + Debug + Default + From<usize> + Into<usize> + Successor;

    /// Push a key component onto the path.
    fn push_key(&mut self, key: Self::Key);
    /// Push an index component onto the path.
    fn push_index(&mut self, index: Self::Index);
    /// Remove the last component from the path.
    fn pop(&mut self);
    /// Returns `true` if the path is empty.
    fn is_empty(&self) -> bool;
    /// Returns the length of the path.
    fn len(&self) -> usize;
    /// Returns the last component of the path, if it exists.
    fn last(&self) -> Option<&PathComponent<Self::Key, Self::Index>>;

    /// Return an iterator over the components of the path.
    fn iter(&self) -> impl Iterator<Item = &PathComponent<Self::Key, Self::Index>>;
}

pub trait Successor {
    fn successor(&self) -> Self;
}
