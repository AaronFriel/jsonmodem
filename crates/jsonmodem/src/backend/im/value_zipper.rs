use alloc::sync::Arc;

use super::{
    ImPath,
    value::{Array, Map, Str, Value},
};
use crate::path::PathItem;

#[derive(Debug)]
pub struct ValueZipper {
    root: Value,
}

impl ValueZipper {
    pub fn new() -> Self {
        Self { root: Value::Null }
    }

    pub fn take_root(&mut self) -> Value {
        core::mem::replace(&mut self.root, Value::Null)
    }

    pub fn read_root(&self) -> &Value {
        &self.root
    }

    pub fn with_leaf_mut<'a, F>(
        &'a mut self,
        path: &'a ImPath,
        mutate: F,
    ) -> (&'a ImPath, &'a Value)
    where
        F: FnOnce(&mut Value),
    {
        let slot = align_path(&mut self.root, path);
        mutate(slot);
        let leaf: &Value = slot;
        (path, leaf)
    }

    pub fn with_leaf<'a>(&'a mut self, path: &'a ImPath) -> (&'a ImPath, &'a Value) {
        let slot = align_path(&mut self.root, path) as *mut Value;
        let leaf = unsafe { &*slot };
        (path, leaf)
    }
}

impl Default for ValueZipper {
    fn default() -> Self {
        Self::new()
    }
}

fn ensure_array(value: &mut Value) -> &mut Array {
    if !matches!(value, Value::Array(_)) {
        *value = Value::Array(Array::new_sync());
    }

    match value {
        Value::Array(array) => array,
        _ => unreachable!(),
    }
}

fn ensure_array_index(array: &mut Array, index: usize) -> &mut Value {
    let mut len = array.len();
    while len <= index {
        array.push_back_mut(Value::Null);
        len += 1;
    }

    array
        .get_mut(index)
        .unwrap_or_else(|| panic!("index ensured but missing: {index}"))
}

fn ensure_object(value: &mut Value) -> &mut Map {
    if !matches!(value, Value::Object(_)) {
        *value = Value::Object(Map::new_sync());
    }

    match value {
        Value::Object(map) => map,
        _ => unreachable!(),
    }
}

fn ensure_object_key<'a>(map: &'a mut Map, key: &Arc<str>) -> &'a mut Value {
    let key_str: &str = key.as_ref();
    if map.contains_key(key_str) {
        map.get_mut(key_str)
            .expect("key exists but could not be fetched mutably")
    } else {
        map.insert_mut(Str::from(key_str), Value::Null);
        map.get_mut(key_str)
            .expect("key inserted but missing from map")
    }
}

fn align_path<'a>(root: &'a mut Value, path: &ImPath) -> &'a mut Value {
    let mut current = root;

    for component in path {
        match component {
            PathItem::Index(index) => {
                let array = ensure_array(current);
                current = ensure_array_index(array, *index);
            }
            PathItem::Key(key) => {
                let map = ensure_object(current);
                current = ensure_object_key(map, key);
            }
        }
    }

    current
}
