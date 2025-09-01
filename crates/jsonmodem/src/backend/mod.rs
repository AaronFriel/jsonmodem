pub mod raw;
mod rust;

use alloc::string::String;
use core::{
    error::Error,
    fmt::{Debug, Display},
};

pub use raw::RawContext;
pub use rust::RustContext;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RawStrHint {
    StrictUnicode,
    SurrogatePreserving,
    ReplaceInvalid,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PathKind {
    Key,
    Index,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PathError {
    NotArrayFrame,
    Empty,
}

/// Minimal, batch-local path capability: Frozen (lifetimeless) <-> Thawed
/// (token-bound), and O(1) mutations the parser needs.
pub trait PathCtx {
    type Frozen: Default + 'static;
    type Thawed: Clone;

    /// Create a new empty Frozen path (token may be required for host
    /// allocators).
    fn frozen_new(&mut self) -> Self::Frozen;

    /// Move Frozen -> Thawed at batch start; Thawed -> Frozen at batch end.
    fn thaw(&mut self, frozen: Self::Frozen) -> Self::Thawed;
    fn freeze(&mut self, thawed: Self::Thawed) -> Self::Frozen;

    // O(1) ops the parser uses
    fn push_key_from_str(&mut self, t: &mut Self::Thawed, key: &str);
    fn push_index_zero(&mut self, t: &mut Self::Thawed);
    fn bump_last_index(&mut self, t: &mut Self::Thawed) -> Result<(), PathError>;
    fn pop_kind(&mut self, t: &mut Self::Thawed) -> Option<PathKind>;
    fn last_kind(&self, t: &Self::Thawed) -> Option<PathKind>;
}

pub trait EventCtx {
    type Null;
    type Bool;
    /// A number value; typically owned but may borrow for to support, for
    /// example, arbitrary precision.
    type Num<'src>;
    /// A string *fragment*; may borrow from the input ('src) or allocate.
    type Str<'src>;
    type Error: Error + Debug + Display + PartialEq;

    fn new_null(&mut self) -> Result<Self::Null, Self::Error>;
    fn new_bool(&mut self, b: bool) -> Result<Self::Bool, Self::Error>;
    fn new_number<'src>(&mut self, n: &'src str) -> Result<Self::Num<'src>, Self::Error>;
    fn new_number_owned<'a>(&mut self, n: String) -> Result<Self::Num<'a>, Self::Error>;

    /// Turn a fragment of the input into a backend string.
    /// Backends can return a borrow (Rust) or allocate (Python/JS).
    fn new_str<'src>(&mut self, frag: &'src str) -> Result<Self::Str<'src>, Self::Error>;
    fn new_str_owned<'a>(&mut self, frag: String) -> Result<Self::Str<'a>, Self::Error>;

    /// Create a string fragment from raw bytes (WTF-8 or other non-UTF8).
    ///
    /// This is only used when the parser is configured to preserve
    /// unpaired surrogates (DecodeMode::SurrogatePreserving) and hence cannot
    /// represent the fragment as valid UTF-8. Implementations may choose to
    /// preserve the bytes, replace invalid sequences with U+FFFD, or error,
    /// guided by the provided hint.
    fn new_str_raw_owned<'a>(
        &mut self,
        bytes: alloc::vec::Vec<u8>,
        hint: RawStrHint,
    ) -> Result<Self::Str<'a>, Self::Error>;
}

// // Extends EventCtx to build compound values.
// pub trait ValueCtx: EventCtx {
//     type Value;
//     type Array;
//     type Object;

//     fn array_new(&mut self, cap: usize) -> Self::Array;
//     fn array_push_value(&mut self, arr: &mut Self::Array, v: Self::Value);

//     fn object_new(&mut self, cap: usize) -> Self::Object;
//     fn object_insert_value(
//         &mut self,
//         obj: &mut Self::Object,
//         key: &'static str, // TODO
//         val: Self::Value,
//     );

//     // Lift primitives into a Value
//     fn value_null(&mut self) -> Self::Value;
//     fn value_bool(&mut self, b: <Self as EventCtx<'cx, B>>::Bool) ->
// Self::Value;     fn value_num<'src>(&mut self, n: <Self as EventCtx<'cx,
// B>>::Num<'src>) -> Self::Value;     fn value_str<'src>(&mut self, s: <Self as
// EventCtx<'cx, B>>::Str<'src>) -> Self::Value; }

// pub type ValueOf<'cx, B> = <<B as ParserContext>::Ctx<'cx> as ValueCtx<'cx,
// B>>::Value; pub type ArrayOf<'cx, B> = <<B as ParserContext>::Ctx<'cx> as
// ValueCtx<'cx, B>>::Array; pub type ObjectOf<'cx, B> = <<B as
// ParserContext>::Ctx<'cx> as ValueCtx<'cx, B>>::Object;
