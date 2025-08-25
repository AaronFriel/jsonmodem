
#![allow(clippy::inline_always)]

use core::fmt::Debug;

/// Kinds of JSON values.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum ValueKind {
    Null,
    Bool,
    Num,
    Str,
    Array,
    Object,
}

/// Abstraction over JSON values without constructors.
pub trait JsonValue: Debug + Clone + PartialEq + Default {
    type Str: Debug + Clone + PartialEq + Eq + Default;
    type Num: Debug + Copy + Clone + PartialEq;
    type Bool: Debug + Copy + Clone + PartialEq;
    type Null: Debug + Copy + Clone + PartialEq;
    type Array: Debug + Clone + Default + PartialEq;
    type Object: Debug + Clone + Default + PartialEq;

    type Path: JsonPath + Debug + Clone + Debug + PartialEq;

    fn kind(v: &Self) -> ValueKind;
    fn as_string_mut(v: &mut Self) -> Option<&mut Self::Str>;
    fn as_array_mut(v: &mut Self) -> Option<&mut Self::Array>;
    fn as_object_mut(v: &mut Self) -> Option<&mut Self::Object>;
    fn object_get_mut<'a>(
        obj: &'a mut Self::Object,
        key: &<Self::Path as JsonPath>::Key,
    ) -> Option<&'a mut Self>;
    fn array_get_mut(
        arr: &mut Self::Array,
        idx: <Self::Path as JsonPath>::Index,
    ) -> Option<&mut Self>;
    fn array_len(arr: &Self::Array) -> usize;

    fn into_array(v: Self) -> Option<Self::Array>
    where
        Self: Sized;

    fn into_object(v: Self) -> Option<Self::Object>
    where
        Self: Sized;
}

// TODO - UNUSED?
pub type JsonValuePathComponent<V> = PathComponent<
    <<V as JsonValue>::Path as JsonPath>::Key,
    <<V as JsonValue>::Path as JsonPath>::Index,
>;

/// Factory trait that creates and mutates JSON values.
pub trait JsonValueFactory {
    type Value: JsonValue;

    fn new_null(&mut self) -> <Self::Value as JsonValue>::Null;
    fn new_bool(&mut self, b: bool) -> <Self::Value as JsonValue>::Bool;
    fn new_number(&mut self, n: f64) -> <Self::Value as JsonValue>::Num;
    fn new_string(&mut self, s: &str) -> <Self::Value as JsonValue>::Str;
    fn new_array(&mut self) -> <Self::Value as JsonValue>::Array;
    fn new_object(&mut self) -> <Self::Value as JsonValue>::Object;

    fn push_string(
        &mut self,
        string: &mut <Self::Value as JsonValue>::Str,
        val: &<Self::Value as JsonValue>::Str,
    );
    fn push_str(&mut self, string: &mut <Self::Value as JsonValue>::Str, val: &str);
    fn push_array(&mut self, array: &mut <Self::Value as JsonValue>::Array, val: Self::Value);
    fn insert_object(
        &mut self,
        obj: &mut <Self::Value as JsonValue>::Object,
        key: &str,
        val: Self::Value,
    );

    fn build_from_str(&mut self, s: <Self::Value as JsonValue>::Str) -> Self::Value;
    fn build_from_num(&mut self, n: <Self::Value as JsonValue>::Num) -> Self::Value;
    fn build_from_bool(&mut self, b: <Self::Value as JsonValue>::Bool) -> Self::Value;
    fn build_from_null(&mut self, n: <Self::Value as JsonValue>::Null) -> Self::Value;
    fn build_from_array(&mut self, a: <Self::Value as JsonValue>::Array) -> Self::Value;
    fn build_from_object(&mut self, o: <Self::Value as JsonValue>::Object) -> Self::Value;

    fn object_insert<'a, 'b: 'a>(
        &'a mut self,
        obj: &'b mut <Self::Value as JsonValue>::Object,
        key: <<Self::Value as JsonValue>::Path as JsonPath>::Key,
        val: Self::Value,
    ) -> &'b mut Self::Value;
    fn array_push<'a, 'b: 'a>(
        &'a mut self,
        arr: &'b mut <Self::Value as JsonValue>::Array,
        val: Self::Value,
    ) -> &'b mut Self::Value;
}

/// Standard zero-cost factory for the built-in [`Value`] type.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct StdValueFactory;

use alloc::{collections::BTreeMap, vec::Vec};

use crate::{
    JsonPath, Path, PathComponent, Str,
    path_component::{Index, Key},
    value::Value,
};

impl JsonValue for Value {
    type Str = Str;
    type Num = f64;
    type Bool = bool;
    type Null = ();
    type Array = Vec<Value>;
    type Object = BTreeMap<Key, Value>;

    type Path = Path;

    #[inline(always)]
    fn kind(v: &Self) -> ValueKind {
        match v {
            Value::Null => ValueKind::Null,
            Value::Boolean(_) => ValueKind::Bool,
            Value::Number(_) => ValueKind::Num,
            Value::String(_) => ValueKind::Str,
            Value::Array(_) => ValueKind::Array,
            Value::Object(_) => ValueKind::Object,
        }
    }

    #[inline(always)]
    fn as_string_mut(v: &mut Self) -> Option<&mut Self::Str> {
        if let Value::String(s) = v {
            Some(s)
        } else {
            None
        }
    }

    #[inline(always)]
    fn as_array_mut(v: &mut Self) -> Option<&mut <self::Value as JsonValue>::Array> {
        if let Value::Array(a) = v {
            Some(a)
        } else {
            None
        }
    }

    #[inline(always)]
    fn as_object_mut(v: &mut Self) -> Option<&mut <self::Value as JsonValue>::Object> {
        if let Value::Object(o) = v {
            Some(o)
        } else {
            None
        }
    }

    #[inline(always)]
    fn object_get_mut<'a>(
        obj: &'a mut <self::Value as JsonValue>::Object,
        key: &Key,
    ) -> Option<&'a mut Self> {
        obj.get_mut(key)
    }

    #[inline(always)]
    fn array_get_mut(arr: &mut <self::Value as JsonValue>::Array, idx: Index) -> Option<&mut Self> {
        arr.get_mut(idx)
    }

    #[inline(always)]
    fn array_len(arr: &<self::Value as JsonValue>::Array) -> usize {
        arr.len()
    }

    #[inline(always)]
    fn into_array(v: Self) -> Option<<self::Value as JsonValue>::Array> {
        if let Value::Array(a) = v {
            Some(a)
        } else {
            None
        }
    }

    #[inline(always)]
    fn into_object(v: Self) -> Option<<self::Value as JsonValue>::Object> {
        if let Value::Object(o) = v {
            Some(o)
        } else {
            None
        }
    }
}

impl JsonValueFactory for StdValueFactory {
    type Value = Value;

    #[inline(always)]
    fn new_null(&mut self) -> <self::Value as JsonValue>::Null {}

    #[inline(always)]
    fn new_bool(&mut self, b: bool) -> <self::Value as JsonValue>::Bool {
        b
    }

    #[inline(always)]
    fn new_number(&mut self, n: f64) -> <self::Value as JsonValue>::Num {
        n
    }

    #[inline(always)]
    fn new_string(&mut self, s: &str) -> <self::Value as JsonValue>::Str {
        s.into()
    }

    #[inline(always)]
    fn new_array(&mut self) -> <self::Value as JsonValue>::Array {
        Vec::new()
    }

    #[inline(always)]
    fn new_object(&mut self) -> <self::Value as JsonValue>::Object {
        BTreeMap::new()
    }

    #[inline(always)]
    fn push_string(
        &mut self,
        string: &mut <self::Value as JsonValue>::Str,
        val: &<self::Value as JsonValue>::Str,
    ) {
        string.push_str(val);
    }

    #[inline(always)]
    fn push_str(&mut self, string: &mut <self::Value as JsonValue>::Str, val: &str) {
        string.push_str(val);
    }

    #[inline(always)]
    fn push_array(&mut self, array: &mut <self::Value as JsonValue>::Array, val: self::Value) {
        array.push(val);
    }

    #[inline(always)]
    fn insert_object(
        &mut self,
        obj: &mut <self::Value as JsonValue>::Object,
        key: &str,
        val: self::Value,
    ) {
        obj.insert(key.into(), val);
    }

    #[inline(always)]
    fn build_from_str(&mut self, s: <self::Value as JsonValue>::Str) -> self::Value {
        Value::String(s)
    }

    #[inline(always)]
    fn build_from_num(&mut self, n: <self::Value as JsonValue>::Num) -> self::Value {
        Value::Number(n)
    }

    #[inline(always)]
    fn build_from_bool(&mut self, b: <self::Value as JsonValue>::Bool) -> self::Value {
        Value::Boolean(b)
    }

    #[inline(always)]
    fn build_from_null(&mut self, _n: <self::Value as JsonValue>::Null) -> self::Value {
        Value::Null
    }

    #[inline(always)]
    fn build_from_array(&mut self, a: <self::Value as JsonValue>::Array) -> self::Value {
        Value::Array(a)
    }

    #[inline(always)]
    fn build_from_object(&mut self, o: <self::Value as JsonValue>::Object) -> self::Value {
        Value::Object(o)
    }

    #[inline(always)]
    fn object_insert<'a, 'b: 'a>(
        &'a mut self,
        obj: &'b mut <self::Value as JsonValue>::Object,
        key: Key,
        val: self::Value,
    ) -> &'b mut self::Value {
        use alloc::collections::btree_map::Entry;

        match obj.entry(key) {
            Entry::Occupied(occ) => {
                let slot = occ.into_mut();
                *slot = val;
                slot
            }
            Entry::Vacant(slot) => slot.insert(val),
        }
    }

    #[inline(always)]
    fn array_push<'a, 'b: 'a>(
        &mut self,
        arr: &'b mut <self::Value as JsonValue>::Array,
        val: self::Value,
    ) -> &'b mut self::Value {
        arr.push(val);
        // SAFETY: `arr` is guaranteed to be non-empty because we just pushed a value.
        unsafe { arr.last_mut().unwrap_unchecked() }
    }
}

impl<F: JsonValueFactory + ?Sized> JsonValueFactory for &mut F {
    type Value = F::Value;

    #[inline(always)]
    fn new_null(&mut self) -> <Self::Value as JsonValue>::Null {
        (**self).new_null()
    }

    #[inline(always)]
    fn new_bool(&mut self, b: bool) -> <Self::Value as JsonValue>::Bool {
        (**self).new_bool(b)
    }

    #[inline(always)]
    fn new_number(&mut self, n: f64) -> <Self::Value as JsonValue>::Num {
        (**self).new_number(n)
    }

    #[inline(always)]
    fn new_string(&mut self, s: &str) -> <Self::Value as JsonValue>::Str {
        (**self).new_string(s)
    }

    #[inline(always)]
    fn new_array(&mut self) -> <Self::Value as JsonValue>::Array {
        (**self).new_array()
    }

    #[inline(always)]
    fn new_object(&mut self) -> <Self::Value as JsonValue>::Object {
        (**self).new_object()
    }

    #[inline(always)]
    fn push_string(
        &mut self,
        string: &mut <Self::Value as JsonValue>::Str,
        val: &<Self::Value as JsonValue>::Str,
    ) {
        (**self).push_string(string, val);
    }

    #[inline(always)]
    fn push_str(&mut self, string: &mut <Self::Value as JsonValue>::Str, val: &str) {
        (**self).push_str(string, val);
    }

    #[inline(always)]
    fn push_array(&mut self, array: &mut <Self::Value as JsonValue>::Array, val: Self::Value) {
        (**self).push_array(array, val);
    }

    #[inline(always)]
    fn insert_object(
        &mut self,
        obj: &mut <Self::Value as JsonValue>::Object,
        key: &str,
        val: Self::Value,
    ) {
        (**self).insert_object(obj, key, val);
    }

    #[inline(always)]
    fn build_from_str(&mut self, s: <Self::Value as JsonValue>::Str) -> Self::Value {
        (**self).build_from_str(s)
    }

    #[inline(always)]
    fn build_from_num(&mut self, n: <Self::Value as JsonValue>::Num) -> Self::Value {
        (**self).build_from_num(n)
    }

    #[inline(always)]
    fn build_from_bool(&mut self, b: <Self::Value as JsonValue>::Bool) -> Self::Value {
        (**self).build_from_bool(b)
    }

    #[inline(always)]
    fn build_from_null(&mut self, n: <Self::Value as JsonValue>::Null) -> Self::Value {
        (**self).build_from_null(n)
    }

    #[inline(always)]
    fn build_from_array(&mut self, a: <Self::Value as JsonValue>::Array) -> Self::Value {
        (**self).build_from_array(a)
    }

    #[inline(always)]
    fn build_from_object(&mut self, o: <Self::Value as JsonValue>::Object) -> Self::Value {
        (**self).build_from_object(o)
    }

    #[inline(always)]
    fn object_insert<'a, 'b: 'a>(
        &'a mut self,
        obj: &'b mut <Self::Value as JsonValue>::Object,
        key: <<Self::Value as JsonValue>::Path as JsonPath>::Key,
        val: Self::Value,
    ) -> &'b mut Self::Value {
        (**self).object_insert(obj, key, val)
    }

    #[inline(always)]
    fn array_push<'a, 'b: 'a>(
        &'a mut self,
        arr: &'b mut <Self::Value as JsonValue>::Array,
        val: Self::Value,
    ) -> &'b mut Self::Value {
        (**self).array_push(arr, val)
    }
}
