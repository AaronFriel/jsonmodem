#![allow(missing_docs)]

/// Convert a value that may borrow (has a lifetime param) into a fully owned
/// one.
pub trait IntoOwned {
    type Owned;
    fn into_owned(self) -> Self::Owned;
}

pub trait LendingIterator {
    type Item<'a>
    where
        Self: 'a;

    fn next(&mut self) -> Option<Self::Item<'_>>;
}
