use alloc::vec::Vec;
use core::fmt::Debug;

use crate::{
    JsonValue, JsonValueFactory, ParseEvent,
    value_zipper::{ValueBuilder, ZipperError},
};

#[derive(Debug)]
pub(crate) struct EventStack<V: JsonValue> {
    events: Vec<ParseEvent<V>>,
    builder: Option<ValueBuilder<V>>,
}

impl<V: JsonValue> EventStack<V> {
    pub(crate) fn new(events: Vec<ParseEvent<V>>, builder: Option<ValueBuilder<V>>) -> Self {
        Self { events, builder }
    }

    #[cfg(any(test, feature = "fuzzing"))]
    pub(crate) fn len(&self) -> usize {
        self.events.len()
    }

    pub(crate) fn pop(&mut self) -> Option<ParseEvent<V>> {
        self.events.pop()
    }

    pub(crate) fn push<F: JsonValueFactory<Value = V>>(
        &mut self,
        f: &mut F,
        mut event: ParseEvent<V>,
    ) -> Result<(), ZipperError> {
        if let Some(ref mut builder) = self.builder {
            match &mut event {
                // scalars
                ParseEvent::Null { path } => {
                    let v = f.new_null();
                    builder.set(path.last(), f.build_from_null(v), f)?;
                }
                ParseEvent::Boolean { path, value } => {
                    let v = f.build_from_bool(*value);
                    builder.set(path.last(), v, f)?;
                }
                ParseEvent::Number { path, value } => {
                    let v = f.build_from_num(*value);
                    builder.set(path.last(), v, f)?;
                }
                ParseEvent::String { fragment, path, .. } => {
                    builder.mutate_with(
                        f,
                        path.last(),
                        |fac| {
                            let v = fac.new_string("");
                            fac.build_from_str(v)
                        },
                        |v, fac| {
                            if let Some(s) = V::as_string_mut(v) {
                                fac.push_string(s, fragment);
                                Ok(())
                            } else {
                                Err(ZipperError::ExpectedString)
                            }
                        },
                    )?;
                }

                // ── container starts ───────────────────────────────────────
                ParseEvent::ObjectBegin { path } => {
                    builder.enter_with(path.last(), f, |fac| {
                        let v = fac.new_object();
                        fac.build_from_object(v)
                    })?;
                }
                ParseEvent::ArrayStart { path } => {
                    builder.enter_with(path.last(), f, |fac| {
                        let v = fac.new_array();
                        fac.build_from_array(v)
                    })?;
                }

                // ── container ends ─────────────────────────────────────────
                ParseEvent::ArrayEnd { path, value } => {
                    if path.is_empty() {
                        // Path is empty, use the root:
                        let root = core::mem::take(builder).into_value();
                        if let Some(Some(root_array)) = root.map(V::into_array) {
                            value.replace(root_array);
                        } else {
                            #[cfg(test)]
                            panic!("Expected root to be an array");

                            #[cfg(not(test))]
                            return Err(ZipperError::ExpectedArray);
                        }
                    } else if let Some(leaf_array) = V::as_array_mut(builder.pop()?) {
                        value.replace(leaf_array.clone());
                    } else {
                        return Err(ZipperError::ExpectedArray);
                    }
                }
                ParseEvent::ObjectEnd { path, value } => {
                    if path.is_empty() {
                        // Path is empty, use the root:
                        let root = core::mem::take(builder).into_value();
                        if let Some(Some(root_object)) = root.map(V::into_object) {
                            value.replace(root_object);
                        } else {
                            #[cfg(test)]
                            panic!("Expected root to be an object");
                            #[cfg(not(test))]
                            return Err(ZipperError::ExpectedObject);
                        }
                    } else if let Some(leaf_object) = V::as_object_mut(builder.pop()?) {
                        value.replace(leaf_object.clone());
                    } else {
                        return Err(ZipperError::ExpectedObject);
                    }
                }
            }
        }

        self.events.push(event);
        Ok(())
    }

    pub(crate) fn read_root(&self) -> Option<&V> {
        self.builder.as_ref().and_then(|x| x.read_root())
    }
}
