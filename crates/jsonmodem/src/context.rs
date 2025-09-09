use core::{
    error::Error,
    fmt::{Debug, Display},
};

use thiserror::Error;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PathKind {
    Key,
    Index,
}

#[derive(Copy, Clone, Error, Debug, PartialEq, Eq)]
pub enum PathError {
    #[error("not an array frame")]
    NotArrayFrame,
    #[error("empty path")]
    Empty,
}

/// Minimal, batch-local path capability: Frozen (lifetimeless) <-> Thawed
/// (token-bound), and O(1) mutations the parser needs.
pub trait PathCtx {
    type PathState: Debug + Default + 'static;
    type Path: Debug + Clone;

    /// Create a new empty Frozen path (token may be required for host
    /// allocators).
    fn frozen_new(&mut self) -> Self::PathState;

    /// Move Frozen -> Thawed at batch start; Thawed -> Frozen at batch end.
    fn thaw(&mut self, frozen: Self::PathState) -> Self::Path;
    fn freeze(&mut self, thawed: Self::Path) -> Self::PathState;

    // O(1) ops the parser uses
    fn push_key_from_str(&mut self, t: &mut Self::Path, key: &str);
    fn push_index_zero(&mut self, t: &mut Self::Path);
    fn bump_last_index(&mut self, t: &mut Self::Path) -> Result<(), PathError>;
    fn pop_kind(&mut self, t: &mut Self::Path) -> Option<PathKind>;
    fn last_kind(&self, t: &Self::Path) -> Option<PathKind>;
}

pub trait ValueCtx {
    type Null: Debug;
    type Bool: Debug;
    /// A number value; typically owned but may borrow.
    type Num<'src>: Debug;
    /// A string fragment borrowed or owned.
    type Str<'src>: Debug;
    type Value: Debug;
}

pub trait EventCtx: ValueCtx + PathCtx {
    type Error: Error + Debug + Display + PartialEq;

    fn push_key_from_raw_str(&mut self, t: &mut Self::Path, key: &[u8]);

    fn new_null(&mut self) -> Result<Self::Null, Self::Error>;
    fn new_bool(&mut self, b: bool) -> Result<Self::Bool, Self::Error>;
    fn new_number<'src>(&mut self, n: &'src str) -> Result<Self::Num<'src>, Self::Error>;
    fn new_number_owned<'a>(
        &mut self,
        n: alloc::string::String,
    ) -> Result<Self::Num<'a>, Self::Error>;

    fn new_str<'src>(&mut self, frag: &'src str) -> Result<Self::Str<'src>, Self::Error>;
    fn new_str_owned<'a>(
        &mut self,
        frag: alloc::string::String,
    ) -> Result<Self::Str<'a>, Self::Error>;

    fn new_str_raw_owned<'a>(
        &mut self,
        bytes: alloc::vec::Vec<u8>,
    ) -> Result<Self::Str<'a>, Self::Error>;
}

pub trait OwnedEventCtx: EventCtx {
    type OwnedNum;
    type OwnedStr;

    fn num_into_owned(n: Self::Num<'_>) -> Self::OwnedNum;
    fn str_into_owned(s: Self::Str<'_>) -> Self::OwnedStr;
}

/// Contexts that support assembling buffered values from streaming parse
/// events.
pub trait BuilderCtx: ValueCtx {
    type Array;
    type Object;
}
