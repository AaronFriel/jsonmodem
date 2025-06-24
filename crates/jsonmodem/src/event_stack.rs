use alloc::{string::String, vec::Vec};

use crate::{
    ParseEvent, Value,
    value::Map,
    value_zipper::{ValueBuilder, ZipperError},
};

#[derive(Debug)]
pub(crate) struct EventStack {
    events: Vec<ParseEvent>,
    builder: Option<ValueBuilder>,
}

impl EventStack {
    pub(crate) fn new(events: Vec<ParseEvent>, builder: Option<ValueBuilder>) -> Self {
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
                    builder.set(path.last(), Value::Null)?;
                }
                ParseEvent::Boolean { path, value } => {
                    builder.set(path.last(), (*value).into())?;
                }
                ParseEvent::Number { path, value } => {
                    builder.set(path.last(), (*value).into())?;
                }
                ParseEvent::String { fragment, path, .. } => {
                    builder.mutate_with(
                        path.last(),
                        || Value::String(String::new()),
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
                    builder.enter_with(path.last(), || Value::Object(Map::new()))?;
                }
                ParseEvent::ArrayStart { path } => {
                    builder.enter_with(path.last(), || Value::Array(Vec::new()))?;
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
