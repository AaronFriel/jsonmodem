use alloc::{boxed::Box, collections::BTreeMap, sync::Arc, vec::Vec};

use crate::{
    buffered::{value::Value, value_zipper::ZipperValueBuilder},
    parser::{EventBuilder, ParseEvent, StdFactory},
};

pub trait ValueAssembler<V: ValueBuilder>: Default {
    // apply one core event; update internal stack/tree
    fn apply(&mut self, evt: &ParseEvent<V>) -> ApplyOutcome<'_, V>;

    // root accessors for adapters
    fn root(&self) -> Option<&V::Value>;
    fn into_root(self) -> Option<V::Value>;
}

pub trait ValueBuilder: EventBuilder {
    type Value: Clone + core::fmt::Debug + PartialEq;
    type Array: Clone + core::fmt::Debug + PartialEq;
    type Object: Clone + core::fmt::Debug + PartialEq;

    type Assembler: ValueAssembler<Self>;
}

pub enum AppliedRef<'a, B: ValueBuilder> {
    Nothing,
    String(&'a <B as EventBuilder>::Str),
    Array(&'a B::Array),
    Object(&'a B::Object),
}

pub struct ApplyOutcome<'a, B: ValueBuilder> {
    pub just: AppliedRef<'a, B>,
    pub root_completed: bool,
}

impl ValueBuilder for StdFactory {
    type Value = Value;
    type Array = Vec<Value>;
    type Object = BTreeMap<Arc<str>, Value>;

    type Assembler = ZipperValueBuilder;
}
