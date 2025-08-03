use alloc::{borrow::ToOwned, string::String};
use core::fmt::Debug;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum ValueKind {
    Null,
    Bool,
    Num,
    Str,
    Array,
    Object,
}

/// Abstraction over JSON value construction.
pub trait JsonFactory: Debug + Clone + PartialEq + Default {
    type Str: Debug + Clone + PartialEq + Eq + Default + ToOwned + AsRef<str>;
    type Num: Debug + Copy + Clone + PartialEq;
    type Bool: Debug + Copy + Clone + PartialEq;
    type Null: Debug + Copy + Clone + PartialEq;
    type Array: Debug + Clone + Default + PartialEq;
    type Object: Debug + Clone + Default + PartialEq;

    fn new_null() -> Self::Null;
    fn new_bool(b: bool) -> Self::Bool;
    fn new_number(n: f64) -> Self::Num;
    fn new_string(s: String) -> Self::Str;
    fn new_array() -> Self::Array;
    fn new_object() -> Self::Object;

    fn kind(v: &Self) -> ValueKind;
    fn as_string_mut(v: &mut Self) -> Option<&mut Self::Str>;
    fn as_array_mut(v: &mut Self) -> Option<&mut Self::Array>;
    fn as_object_mut(v: &mut Self) -> Option<&mut Self::Object>;

    fn push_string(string: &mut Self::Str, val: &Self::Str);
    fn push_str(string: &mut Self::Str, val: &str);
    fn push_array(array: &mut Self::Array, val: Self);
    fn insert_object(obj: &mut Self::Object, key: &str, val: Self);

    fn from_str(s: Self::Str) -> Self;
    fn from_num(n: Self::Num) -> Self;
    fn from_bool(b: Self::Bool) -> Self;
    fn from_null(n: Self::Null) -> Self;
    fn from_array(a: Self::Array) -> Self;
    fn from_object(o: Self::Object) -> Self;

    fn into_array(v: Self) -> Option<Self::Array>
    where
        Self: Sized;

    fn into_object(v: Self) -> Option<Self::Object>
    where
        Self: Sized;

    fn object_get_mut<'a>(obj: &'a mut Self::Object, key: &str) -> Option<&'a mut Self>;

    fn object_insert(obj: &mut Self::Object, key: String, val: Self) -> &mut Self;

    fn array_get_mut(arr: &mut Self::Array, idx: usize) -> Option<&mut Self>;

    fn array_push(arr: &mut Self::Array, val: Self) -> &mut Self;

    fn array_len(arr: &Self::Array) -> usize;
}
