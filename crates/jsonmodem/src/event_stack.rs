use alloc::vec::Vec;

use crate::{
    ParseEvent, Value,
    factory::{JsonFactory, StdFactory},
    value_zipper::{ValueBuilder, ZipperError},
};

#[derive(Debug)]
pub(crate) struct EventStack<F: JsonFactory<Any = Value> = StdFactory> {
    events: Vec<ParseEvent>,
    builder: Option<ValueBuilder<F>>,
}

impl<F: JsonFactory<Any = Value> + Default> EventStack<F> {
    pub(crate) fn new(events: Vec<ParseEvent>, builder: Option<ValueBuilder<F>>) -> Self {
        Self { events, builder }
    }

    #[cfg(any(test, feature = "fuzzing"))]
    pub(crate) fn len(&self) -> usize {
        self.events.len()
    }

    pub(crate) fn pop(&mut self) -> Option<ParseEvent> {
        self.events.pop()
    }

    pub(crate) fn push(&mut self, mut event: ParseEvent) -> Result<(), ZipperError> {
        if let Some(ref mut builder) = self.builder {
            match &mut event {
                // scalars
                ParseEvent::Null { path } => {
                    let v = {
                        let f = builder.factory();
                        f.any_from_null(f.new_null())
                    };
                    builder.set(path.last(), v)?;
                }
                ParseEvent::Boolean { path, value } => {
                    let v = {
                        let f = builder.factory();
                        f.any_from_bool(f.new_bool(*value))
                    };
                    builder.set(path.last(), v)?;
                }
                ParseEvent::Number { path, value } => {
                    let v = {
                        let f = builder.factory();
                        f.any_from_num(f.new_number(*value))
                    };
                    builder.set(path.last(), v)?;
                }
                ParseEvent::String { fragment, path, .. } => {
                    let init = {
                        let f = builder.factory();
                        f.any_from_str(f.new_string(""))
                    };
                    builder.mutate_with(
                        path.last(),
                        || init,
                        |v| {
                            if let Value::String(s) = v {
                                s.push_str(fragment);
                                Ok(())
                            } else {
                                Err(ZipperError::ExpectedString)
                            }
                        },
                    )?;
                }

                // ── container starts ───────────────────────────────────────
                ParseEvent::ObjectBegin { path } => {
                    let init = {
                        let f = builder.factory();
                        f.any_from_object(f.new_object())
                    };
                    builder.enter_with(path.last(), || init)?;
                }
                ParseEvent::ArrayStart { path } => {
                    let init = {
                        let f = builder.factory();
                        f.any_from_array(f.new_array())
                    };
                    builder.enter_with(path.last(), || init)?;
                }

                // ── container ends ─────────────────────────────────────────
                ParseEvent::ArrayEnd { path, value } => {
                    if path.is_empty() {
                        // Path is empty, use the root:
                        let root = core::mem::take(builder).into_value();
                        if let Some(Value::Array(root_array)) = root {
                            value.replace(root_array);
                        } else {
                            #[cfg(test)]
                            panic!("Expected root to be an array");

                            #[cfg(not(test))]
                            return Err(ZipperError::ExpectedArray);
                        }
                    } else {
                        // don't pop past root
                        if let Value::Array(leaf_array) = builder.pop()? {
                            value.replace(leaf_array.clone());
                        } else {
                            return Err(ZipperError::ExpectedArray);
                        }
                    }
                }
                ParseEvent::ObjectEnd { path, value } => {
                    if path.is_empty() {
                        // Path is empty, use the root:
                        let root = core::mem::take(builder).into_value();
                        if let Some(Value::Object(root_object)) = root {
                            value.replace(root_object);
                        } else {
                            #[cfg(test)]
                            panic!("Expected root to be an array");
                            #[cfg(not(test))]
                            return Err(ZipperError::ExpectedObject);
                        }
                    } else {
                        // don't pop past root
                        if let Value::Object(leaf_object) = builder.pop()? {
                            value.replace(leaf_object.clone());
                        } else {
                            return Err(ZipperError::ExpectedObject);
                        }
                    }
                }
            }
        }

        self.events.push(event);
        Ok(())
    }

    pub(crate) fn read_root(&self) -> Option<&Value> {
        self.builder.as_ref().and_then(|x| x.read_root())
    }
}
