#![allow(dead_code)]
use alloc::{boxed::Box, collections::BTreeMap, sync::Arc, vec::Vec};
use core::{cmp::Ordering, ptr::NonNull};

use super::{StdPath, value::Value};
use crate::path::{Path, PathItem};

#[derive(Debug)]
/// Caches pointers along the parser's current branch of an owned value tree.
/// Descendant pointers must be discarded before replacing or growing an
/// ancestor.
pub struct ValueZipper {
    root: Box<Value>,
    path_nodes: Vec<NonNull<Value>>,
    path_components: Vec<PathItem>,
}

impl Default for ValueZipper {
    fn default() -> Self {
        Self::new()
    }
}

impl ValueZipper {
    #[inline]
    pub fn new() -> Self {
        Self {
            root: Box::new(Value::Null),
            path_nodes: Vec::with_capacity(8),
            path_components: Vec::with_capacity(8),
        }
    }

    #[inline]
    pub fn insert_value(&mut self, path: &Path, value: Value) {
        let slot = self.align_path(path);
        *slot = value;
    }

    #[inline]
    pub fn take_root(&mut self) -> Value {
        self.path_nodes.clear();
        self.path_components.clear();
        core::mem::replace(self.root.as_mut(), Value::Null)
    }

    #[inline]
    pub fn read_root(&self) -> &Value {
        &self.root
    }

    #[inline]
    pub(crate) fn with_leaf_mut<'a, F>(
        &'a mut self,
        path: &Path,
        mutate: F,
    ) -> (&'a StdPath, &'a Value)
    where
        F: FnOnce(&mut Value),
    {
        let slot = self.align_path(path);
        mutate(slot);
        let slot_ptr = core::ptr::from_mut::<Value>(slot);
        let path = &self.path_components;
        // SAFETY: align_path returned this live leaf; only the separate path
        // vector was borrowed afterward. The result borrows the whole zipper.
        let leaf = unsafe { &*slot_ptr };
        (path, leaf)
    }

    #[inline]
    pub(crate) fn with_leaf<'a>(&'a mut self, path: &Path) -> (&'a StdPath, &'a Value) {
        let slot = core::ptr::from_mut::<Value>(self.align_path(path));
        let path = &self.path_components;
        // SAFETY: borrowing path_components does not move the tree's values.
        let leaf = unsafe { &*slot };
        (path, leaf)
    }

    #[inline]
    fn align_path(&mut self, path: &Path) -> &mut Value {
        let current_depth = self.path_components.len();
        let target_depth = path.len();

        match target_depth.cmp(&current_depth) {
            Ordering::Greater => {
                #[cfg(any(fuzzing, debug_assertions))]
                assert_eq!(
                    target_depth,
                    current_depth + 1,
                    "parser path depth increased by more than one"
                );
                let mut parent_ptr = self.current_ptr();
                let component = path
                    .last()
                    .expect("path depth greater than current depth implies non-empty path");
                // SAFETY: the current node is live and exclusively borrowed.
                // No descendant pointers exist while its container may grow.
                let child = descend_one(unsafe { parent_ptr.as_mut() }, component);
                let child_ptr = NonNull::from(child);
                self.path_nodes.push(child_ptr);
                self.path_components.push(component.clone());
            }
            Ordering::Less => {
                #[cfg(any(fuzzing, debug_assertions))]
                assert_eq!(
                    current_depth,
                    target_depth + 1,
                    "parser path depth decreased by more than one"
                );
                self.path_nodes.truncate(target_depth);
                self.path_components.truncate(target_depth);
            }
            Ordering::Equal => {
                if target_depth == 0 {
                    // Root path – nothing to do.
                } else if let Some(last) = path.last() {
                    let matches_existing = self.path_components.last() == Some(last);
                    if !matches_existing {
                        self.path_nodes.pop();
                        self.path_components.pop();
                        let mut parent_ptr = self.current_ptr();
                        // SAFETY: the old child pointer was removed before
                        // descend_one can grow the parent's container.
                        let child = descend_one(unsafe { parent_ptr.as_mut() }, last);
                        let child_ptr = NonNull::from(child);
                        self.path_nodes.push(child_ptr);
                        self.path_components.push(last.clone());
                    }
                }
            }
        }

        // SAFETY: the branches above retain only pointers to live ancestors
        // and the current leaf. The exclusive borrow prevents concurrent access.
        unsafe { self.current_ptr().as_mut() }
    }

    #[inline]
    fn current_ptr(&mut self) -> NonNull<Value> {
        match self.path_nodes.last().copied() {
            Some(ptr) => ptr,
            None => NonNull::from(self.root.as_mut()),
        }
    }
}

#[inline]
fn ensure_array(value: &mut Value) -> &mut Vec<Value> {
    if !matches!(value, Value::Array(_)) {
        *value = Value::Array(Vec::new());
    }
    match value {
        Value::Array(values) => values,
        _ => unreachable!(),
    }
}

#[inline]
fn ensure_object(value: &mut Value) -> &mut BTreeMap<Arc<str>, Value> {
    if !matches!(value, Value::Object(_)) {
        *value = Value::Object(BTreeMap::new());
    }
    match value {
        Value::Object(map) => map,
        _ => unreachable!(),
    }
}

#[inline]
fn descend_one<'a>(current: &'a mut Value, component: &PathItem) -> &'a mut Value {
    match component {
        PathItem::Key(key) => ensure_object(current)
            .entry(key.clone())
            .or_insert(Value::Null),
        PathItem::Index(index) => {
            let array = ensure_array(current);
            let idx = *index;
            match idx.cmp(&array.len()) {
                Ordering::Less => {}
                Ordering::Equal => array.push(Value::Null),
                Ordering::Greater => array.resize(idx + 1, Value::Null),
            }
            array.get_mut(idx).expect("array resized to contain index")
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::{format, vec};

    use super::*;

    #[test]
    fn memory_safety_array_growth_and_root_reuse() {
        let mut zipper = ValueZipper::new();
        for round in 0..3 {
            zipper.insert_value(&vec![], Value::Array(Vec::new()));
            for index in 0..64 {
                let path = vec![PathItem::Index(index)];
                let (actual_path, value) = zipper.with_leaf_mut(&path, |value| {
                    *value = Value::Number(f64::from(u32::try_from(round + index).unwrap()));
                });
                assert_eq!(actual_path, &path);
                assert_eq!(
                    value,
                    &Value::Number(f64::from(u32::try_from(round + index).unwrap()))
                );
            }
            zipper.with_leaf(&vec![]);
            let expected = Value::Array(
                (0..64)
                    .map(|index| Value::Number(f64::from(u32::try_from(round + index).unwrap())))
                    .collect(),
            );
            assert_eq!(zipper.read_root(), &expected);
            assert_eq!(zipper.take_root(), expected);
            assert_eq!(zipper.read_root(), &Value::Null);
            assert!(zipper.path_nodes.is_empty());
        }
    }

    #[test]
    fn memory_safety_map_growth_nested_replacement_and_read_root() {
        let mut zipper = ValueZipper::new();
        let mut expected = BTreeMap::new();
        for index in 0..64 {
            let key: Arc<str> = format!("key{index:03}").into();
            let parent = vec![PathItem::Key(key.clone())];
            zipper.insert_value(&parent, Value::Array(Vec::new()));
            for child in 0..8 {
                let path = vec![parent[0].clone(), PathItem::Index(child)];
                zipper.insert_value(&path, Value::Boolean(child % 2 == 0));
                assert!(matches!(zipper.read_root(), Value::Object(_)));
                assert_eq!(zipper.with_leaf(&path).1, &Value::Boolean(child % 2 == 0));
            }
            zipper.with_leaf(&parent);
            zipper.insert_value(&parent, Value::String(format!("replaced{index}")));
            expected.insert(key, Value::String(format!("replaced{index}")));
        }
        zipper.with_leaf(&vec![]);
        assert_eq!(zipper.take_root(), Value::Object(expected));
    }

    #[test]
    fn memory_safety_repeated_key_sparse_index_and_ancestor_replacement() {
        let mut zipper = ValueZipper::new();
        let parent = vec![PathItem::Key("key".into())];
        zipper.insert_value(&parent, Value::Null);
        let child = vec![parent[0].clone(), PathItem::Index(32)];
        zipper.insert_value(&child, Value::Boolean(true));
        zipper.with_leaf(&parent);
        let mut expected = vec![Value::Null; 33];
        expected[32] = Value::Boolean(true);
        assert_eq!(zipper.with_leaf(&parent).1, &Value::Array(expected));
        zipper.insert_value(&parent, Value::Null);
        zipper.insert_value(&child, Value::Boolean(false));
        zipper.with_leaf(&parent);
        zipper.with_leaf(&vec![]);
        zipper.insert_value(&vec![], Value::Null);
        assert_eq!(zipper.take_root(), Value::Null);
    }
}
