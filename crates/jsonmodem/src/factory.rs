use alloc::{borrow::ToOwned, collections::BTreeMap, string::String, vec::Vec};

use crate::value::Value;

/// Abstraction over JSON value construction.
#[allow(clippy::wrong_self_convention)]
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

    fn into_any_str(&self, s: Self::Str) -> Self::Any;
    fn into_any_num(&self, n: Self::Num) -> Self::Any;
    fn into_any_bool(&self, b: Self::Bool) -> Self::Any;
    fn into_any_null(&self, n: Self::Null) -> Self::Any;
    fn into_any_array(&self, a: Self::Array) -> Self::Any;
    fn into_any_object(&self, o: Self::Object) -> Self::Any;
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

    #[inline]
    fn new_null(&self) -> Self::Null {
        // unit type has a default return
    }

    #[inline]
    fn new_bool(&self, b: bool) -> Self::Bool {
        b
    }

    #[inline]
    fn new_number(&self, n: f64) -> Self::Num {
        n
    }

    #[inline]
    fn new_string(&self, s: &str) -> Self::Str {
        s.to_owned()
    }

    #[inline]
    fn new_array(&self) -> Self::Array {
        Vec::new()
    }

    #[inline]
    fn new_object(&self) -> Self::Object {
        BTreeMap::new()
    }

    #[inline]
    fn push_array(&self, array: &mut Self::Array, val: Self::Any) {
        array.push(val);
    }

    #[inline]
    fn insert_object(&self, obj: &mut Self::Object, key: &str, val: Self::Any) {
        obj.insert(key.to_owned(), val);
    }

    #[inline]
    fn into_any_str(&self, s: Self::Str) -> Self::Any {
        Value::String(s)
    }

    #[inline]
    fn into_any_num(&self, n: Self::Num) -> Self::Any {
        Value::Number(n)
    }

    #[inline]
    fn into_any_bool(&self, b: Self::Bool) -> Self::Any {
        Value::Boolean(b)
    }

    #[inline]
    fn into_any_null(&self, _n: Self::Null) -> Self::Any {
        Value::Null
    }

    #[inline]
    fn into_any_array(&self, a: Self::Array) -> Self::Any {
        Value::Array(a)
    }

    #[inline]
    fn into_any_object(&self, o: Self::Object) -> Self::Any {
        Value::Object(o)
    }
}
