use alloc::vec::Vec;
use core::fmt::Debug;

use crate::{
    ParseEvent,
    factory::JsonFactory,
    value_zipper::{ValueBuilder, ZipperError},
};

#[derive(Debug)]
pub(crate) struct EventStack<V: JsonFactory> {
    events: Vec<ParseEvent<V>>,
    builder: Option<ValueBuilder<V>>,
}

impl<V: JsonFactory> EventStack<V> {
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

    pub(crate) fn push(&mut self, mut event: ParseEvent<V>) -> Result<(), ZipperError> {
        if let Some(ref mut builder) = self.builder {
            match &mut event {
                // scalars
                ParseEvent::Null { path } => {
                    builder.set(path.last(), V::from_null(V::new_null()))?;
                }
                ParseEvent::Boolean { path, value } => {
                    builder.set(path.last(), V::from_bool(*value))?;
                }
                ParseEvent::Number { path, value } => {
                    builder.set(path.last(), V::from_num(*value))?;
                }
                ParseEvent::String { fragment, path, .. } => {
                    builder.mutate_with(
                        path.last(),
                        || V::from_str(V::Str::default()),
                        |v| {
                            if let Some(s) = V::as_string_mut(v) {
                                V::push_string(s, fragment);
                                Ok(())
                            } else {
                                Err(ZipperError::ExpectedString)
                            }
                        },
                    )?;
                }

                // ── container starts ───────────────────────────────────────
                ParseEvent::ObjectBegin { path } => {
                    builder.enter_with(path.last(), || V::from_object(V::new_object()))?;
                }
                ParseEvent::ArrayStart { path } => {
                    builder.enter_with(path.last(), || V::from_array(V::new_array()))?;
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
