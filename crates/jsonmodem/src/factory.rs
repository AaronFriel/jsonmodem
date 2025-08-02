#![allow(clippy::inline_always)]

use alloc::{borrow::ToOwned, collections::BTreeMap, string::String, vec::Vec};

use crate::value::Value;

/// Abstraction over JSON value construction.
pub trait JsonFactory {
    type Str;
    type Num;
    type Bool;
    type Null;
    type Array;
    type Object;
    type Any;

    fn new_null(&self) -> Self::Null;
    fn new_bool(&self, b: bool) -> Self::Bool;
    fn new_number(&self, n: f64) -> Self::Num;
    fn new_string(&self, s: &str) -> Self::Str;
    fn new_array(&self) -> Self::Array;
    fn new_object(&self) -> Self::Object;

    fn push_array(&self, array: &mut Self::Array, val: Self::Any);
    fn insert_object(&self, obj: &mut Self::Object, key: &str, val: Self::Any);

    fn any_from_str(&self, s: Self::Str) -> Self::Any;
    fn any_from_num(&self, n: Self::Num) -> Self::Any;
    fn any_from_bool(&self, b: Self::Bool) -> Self::Any;
    fn any_from_null(&self, n: Self::Null) -> Self::Any;
    fn any_from_array(&self, a: Self::Array) -> Self::Any;
    fn any_from_object(&self, o: Self::Object) -> Self::Any;
}

/// Factory producing standard Rust values.
#[derive(Clone, Copy, Debug, Default)]
pub struct StdFactory;

impl JsonFactory for StdFactory {
    type Str = String;
    type Num = f64;
    type Bool = bool;
    type Null = ();
    type Array = Vec<Value>;
    type Object = BTreeMap<String, Value>;
    type Any = Value;

    #[inline(always)]
    fn new_null(&self) -> Self::Null {
        // unit type has a default return
    }

    #[inline(always)]
    fn new_bool(&self, b: bool) -> Self::Bool {
        b
    }

    #[inline(always)]
    fn new_number(&self, n: f64) -> Self::Num {
        n
    }

    #[inline(always)]
    fn new_string(&self, s: &str) -> Self::Str {
        s.to_owned()
    }

    #[inline(always)]
    fn new_array(&self) -> Self::Array {
        Vec::new()
    }

    #[inline(always)]
    fn new_object(&self) -> Self::Object {
        BTreeMap::new()
    }

    #[inline(always)]
    fn push_array(&self, array: &mut Self::Array, val: Self::Any) {
        array.push(val);
    }

    #[inline(always)]
    fn insert_object(&self, obj: &mut Self::Object, key: &str, val: Self::Any) {
        obj.insert(key.to_owned(), val);
    }

    #[inline(always)]
    fn any_from_str(&self, s: Self::Str) -> Self::Any {
        Value::String(s)
    }

    #[inline(always)]
    fn any_from_num(&self, n: Self::Num) -> Self::Any {
        Value::Number(n)
    }

    #[inline(always)]
    fn any_from_bool(&self, b: Self::Bool) -> Self::Any {
        Value::Boolean(b)
    }

    #[inline(always)]
    fn any_from_null(&self, _n: Self::Null) -> Self::Any {
        Value::Null
    }

    #[inline(always)]
    fn any_from_array(&self, a: Self::Array) -> Self::Any {
        Value::Array(a)
    }

    #[inline(always)]
    fn any_from_object(&self, o: Self::Object) -> Self::Any {
        Value::Object(o)
    }
}
