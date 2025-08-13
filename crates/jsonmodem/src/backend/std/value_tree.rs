#![allow(dead_code)]
use alloc::{collections::BTreeMap, string::String, sync::Arc, vec::Vec};

use super::value::Value;
use crate::path::{Path, PathItem};

#[derive(Debug)]
pub(crate) struct ValueTree {
    root: Value,
}

impl ValueTree {
    pub(crate) fn insert_value(&mut self, path: &Path, value: Value) {
        if path.is_empty() {
            self.root = value;
            return;
        }

        let mut current = &mut self.root;
        for component in &path[..path.len() - 1] {
            current = match component {
                PathItem::Key(key) => {
                    let map = ensure_object(current);
                    map.entry(key.clone()).or_insert(Value::Null)
                }
                PathItem::Index(index) => {
                    let array = ensure_array(current);
                    let idx = *index;
                    if idx >= array.len() {
                        array.resize(idx + 1, Value::Null);
                    }
                    &mut array[idx]
                }
            };
        }

        match path.last().expect("path is non-empty") {
            PathItem::Key(key) => {
                let map = ensure_object(current);
                map.insert(key.clone(), value);
            }
            PathItem::Index(index) => {
                let array = ensure_array(current);
                let idx = *index;
                if idx >= array.len() {
                    array.resize(idx + 1, Value::Null);
                }
                array[idx] = value;
            }
        }
    }

    pub(crate) fn append_string(&mut self, path: &Path, fragment: &str) {
        if path.is_empty() {
            match &mut self.root {
                Value::String(buffer) => buffer.push_str(fragment),
                _ => self.root = Value::String(fragment.into()),
            }
            return;
        }

        let mut current = &mut self.root;
        for component in &path[..path.len() - 1] {
            current = match component {
                PathItem::Key(key) => {
                    let map = ensure_object(current);
                    map.entry(key.clone()).or_insert(Value::Null)
                }
                PathItem::Index(index) => {
                    let array = ensure_array(current);
                    let idx = *index;
                    if idx >= array.len() {
                        array.resize(idx + 1, Value::Null);
                    }
                    &mut array[idx]
                }
            };
        }

        match path.last().expect("path is non-empty") {
            PathItem::Key(key) => {
                let map = ensure_object(current);
                let entry = map
                    .entry(key.clone())
                    .or_insert_with(|| Value::String(String::new()));
                if let Value::String(buffer) = entry {
                    buffer.push_str(fragment);
                } else {
                    *entry = Value::String(fragment.into());
                }
            }
            PathItem::Index(index) => {
                let array = ensure_array(current);
                let idx = *index;
                if idx >= array.len() {
                    array.resize(idx + 1, Value::Null);
                }
                match &mut array[idx] {
                    Value::String(buffer) => buffer.push_str(fragment),
                    slot => *slot = Value::String(fragment.into()),
                }
            }
        }
    }

    pub(crate) fn clone_at_path(&self, path: &Path) -> Option<Value> {
        let mut current = &self.root;
        for component in path {
            current = match (component, current) {
                (PathItem::Key(key), Value::Object(map)) => map.get(key)?,
                (PathItem::Index(index), Value::Array(array)) => array.get(*index)?,
                _ => return None,
            };
        }
        Some(current.clone())
    }

    pub(crate) fn value_at_path(&self, path: &Path) -> Option<&Value> {
        let mut current = &self.root;
        for component in path {
            current = match (component, current) {
                (PathItem::Key(key), Value::Object(map)) => map.get(key)?,
                (PathItem::Index(index), Value::Array(array)) => array.get(*index)?,
                _ => return None,
            };
        }
        Some(current)
    }

    #[allow(dead_code)]
    pub(crate) fn take_root(&mut self) -> Value {
        core::mem::replace(&mut self.root, Value::Null)
    }

    #[allow(dead_code)]
    pub(crate) fn root(&self) -> &Value {
        &self.root
    }
}

impl Default for ValueTree {
    fn default() -> Self {
        Self { root: Value::Null }
    }
}

fn ensure_array(value: &mut Value) -> &mut Vec<Value> {
    if let Value::Array(array) = value {
        array
    } else {
        *value = Value::Array(Vec::new());
        match value {
            Value::Array(array) => array,
            _ => unreachable!(),
        }
    }
}

fn ensure_object(value: &mut Value) -> &mut BTreeMap<Arc<str>, Value> {
    if let Value::Object(map) = value {
        map
    } else {
        *value = Value::Object(BTreeMap::new());
        match value {
            Value::Object(map) => map,
            _ => unreachable!(),
        }
    }
}
