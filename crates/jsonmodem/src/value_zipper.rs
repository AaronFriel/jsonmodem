#![allow(clippy::enum_glob_use)]

use alloc::{boxed::Box, collections::btree_map::Entry, string::String, vec::Vec};
use core::{cmp::Ordering, ptr::NonNull};

#[cfg(test)]
use crate::{ParseEvent, ParserOptions, StreamingParser};
use crate::{event::PathComponent, value::Value};

#[derive(Debug)]
pub struct ValueZipper {
    root: Box<Value>,
    path: Vec<NonNull<Value>>, // 0 = root, last = current leaf
    #[cfg(test)]
    path_components: Vec<PathComponent>,
}

impl ValueZipper {
    pub fn new(value: Value) -> Self {
        Self {
            root: Box::new(value),
            path: Vec::new(),
            #[cfg(test)]
            path_components: Vec::new(),
        }
    }

    #[inline]
    fn current_mut(&mut self) -> &mut Value {
        match self.path.last().copied().as_mut() {
            Some(ptr) => unsafe { ptr.as_mut() },
            None => self.root.as_mut(),
        }
    }

    // ─── public clone‑free operations ──────────────────────────────────────

    pub fn enter_lazy<F>(&mut self, pc: PathComponent, make_child: F) -> Result<(), ZipperError>
    where
        F: FnOnce() -> Value,
    {
        match pc {
            PathComponent::Key(k) => self.enter_key_lazy(k, make_child),
            PathComponent::Index(i) => self.enter_index_lazy(i, make_child),
        }
    }

    pub fn set_at(&mut self, pc: PathComponent, value: Value) -> Result<(), ZipperError> {
        match pc {
            PathComponent::Key(k) => self.modify_or_insert_key(
                k,
                value,
                |v| v, // move
                |new, entry| {
                    if let Some(e) = entry {
                        *e = new;
                        Ok(())
                    } else {
                        Err(ZipperError::ExpectedNonEmptyPath)
                    }
                },
            ),
            PathComponent::Index(i) => self.modify_or_insert_index(
                i,
                value,
                |v| v,
                |new, entry| {
                    if let Some(e) = entry {
                        *e = new;
                        Ok(())
                    } else {
                        Err(ZipperError::ExpectedNonEmptyPath)
                    }
                },
            ),
        }
    }

    pub fn mutate_lazy<D, M>(
        &mut self,
        pc: PathComponent,
        make_default: D,
        mutator: M,
    ) -> Result<(), ZipperError>
    where
        D: FnOnce() -> Value,
        M: FnOnce(&mut Value) -> Result<(), ZipperError>,
    {
        match pc {
            PathComponent::Key(k) => self.modify_or_insert_key(
                k,
                (), // zero‑sized token
                |()| make_default(),
                |(), entry| {
                    if let Some(v) = entry {
                        mutator(v)?;
                    }
                    Ok(())
                },
            ),
            PathComponent::Index(i) => self.modify_or_insert_index(
                i,
                (),
                |()| make_default(),
                |(), entry| {
                    if let Some(v) = entry {
                        mutator(v)?;
                    }
                    Ok(())
                },
            ),
        }
    }

    pub fn pop(&mut self) -> &mut Value {
        let leaf = match self.path.pop().as_mut() {
            Some(ptr) => unsafe { ptr.as_mut() },
            None => self.root.as_mut(),
        };

        #[cfg(test)]
        self.path_components.pop();
        leaf
    }

    #[inline]
    pub fn read_root(&self) -> &Value {
        &self.root
    }

    #[inline]
    pub fn into_value(self) -> Value {
        *self.root
    }

    // ─── internal helpers (key / index) ────────────────────────────────────

    fn modify_or_insert_key<T, F, G>(
        &mut self,
        k: String,
        default: T,
        initializer: F,
        f: G,
    ) -> Result<(), ZipperError>
    where
        T: Clone,
        F: FnOnce(T) -> Value,
        G: FnOnce(T, Option<&mut Value>) -> Result<(), ZipperError>,
    {
        // if self.path.is_empty() {
        //     return Err(ZipperError::ExpectedNonEmptyPath);
        // }
        let Value::Object(obj) = self.current_mut() else {
            return Err(ZipperError::ExpectedObject);
        };
        match obj.entry(k) {
            Entry::Occupied(mut occ) => {
                f(default, Some(occ.get_mut()))?;
            }
            Entry::Vacant(vac) => {
                // Insert a freshly initialised child and immediately pass a
                // mutable reference to the caller so the very first chunk of
                // data (e.g. the first PartialString segment) is not lost.
                // Clone `default` so we can pass ownership to both the
                // initializer (which consumes it) *and* to the caller-supplied
                // closure.  `T` is bounded by `Clone` so this is always
                // possible and cheap for the `()` case that accounts for the
                // partial-string pathway.
                let cloned_default = default.clone();
                let child_ref = vac.insert(initializer(default));
                f(cloned_default, Some(child_ref))?;
            }
        }
        Ok(())
    }

    fn modify_or_insert_index<T, F, G>(
        &mut self,
        index: usize,
        default: T,
        initializer: F,
        f: G,
    ) -> Result<(), ZipperError>
    where
        T: Clone,
        F: FnOnce(T) -> Value,
        G: FnOnce(T, Option<&mut Value>) -> Result<(), ZipperError>,
    {
        // if self.path.is_empty() {
        //     return Err(ZipperError::ExpectedNonEmptyPath);
        // }
        let Value::Array(arr) = self.current_mut() else {
            return Err(ZipperError::ExpectedArray);
        };

        match index.cmp(&arr.len()) {
            Ordering::Less => {
                f(default, Some(&mut arr[index]))?;
            }
            Ordering::Equal => {
                // Append a new child and immediately mutate it so that the first
                // incoming chunk (for example the opening segment of a streamed
                // string) is preserved instead of being overwritten on the next
                // call.
                // As with the key variant we need the value twice: once for
                // `initializer` and once for the caller-supplied closure.
                let cloned_default = default.clone();
                arr.push(initializer(default));
                let len = arr.len();
                f(cloned_default, Some(&mut arr[len - 1]))?;
            }
            Ordering::Greater => return Err(ZipperError::InvalidArrayIndex),
        }
        Ok(())
    }

    fn enter_key_lazy<F>(&mut self, k: String, make_child: F) -> Result<(), ZipperError>
    where
        F: FnOnce() -> Value,
    {
        #[cfg(test)]
        self.path_components.push(PathComponent::Key(k.clone()));
        let child_ptr = {
            let Value::Object(obj) = self.current_mut() else {
                return Err(ZipperError::ExpectedObject);
            };
            match obj.entry(k) {
                Entry::Occupied(mut occ) => NonNull::from(occ.get_mut()),
                Entry::Vacant(vac) => {
                    let v = make_child();
                    NonNull::from(vac.insert(v))
                }
            }
        };
        self.path.push(child_ptr);
        Ok(())
    }

    fn enter_index_lazy<F>(&mut self, index: usize, make_child: F) -> Result<(), ZipperError>
    where
        F: FnOnce() -> Value,
    {
        #[cfg(test)]
        self.path_components.push(PathComponent::Index(index));
        let child_ptr = {
            let Value::Array(arr) = self.current_mut() else {
                return Err(ZipperError::ExpectedArray);
            };

            match index.cmp(&arr.len()) {
                Ordering::Less => NonNull::from(&mut arr[index]),
                Ordering::Equal => {
                    arr.push(make_child());
                    NonNull::from(&mut arr[index])
                }
                Ordering::Greater => return Err(ZipperError::InvalidArrayIndex),
            }
        };
        self.path.push(child_ptr);
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  2. Error type
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ZipperError {
    ExpectedObject,
    ExpectedArray,
    InvalidArrayIndex,
    ExpectedEmptyPath,
    ExpectedNonEmptyPath,
    ExpectedString,
    #[cfg(test)]
    ParserError,
}

impl core::fmt::Display for ZipperError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use ZipperError::*;
        write!(
            f,
            "{}",
            match self {
                ExpectedObject => "expected an object at the current path",
                ExpectedArray => "expected an array at the current path",
                InvalidArrayIndex => "invalid array index",
                ExpectedEmptyPath => "operation requires an empty path",
                ExpectedNonEmptyPath => "operation would pop past the root",
                ExpectedString => "expected the root to be a string",
                #[cfg(test)]
                ParserError => "parser error occurred",
            }
        )
    }
}
impl core::error::Error for ZipperError {}

// ─────────────────────────────────────────────────────────────────────────────
//  3. BuilderState – hides Option choreography, but *does not clone*.
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum ValueBuilder {
    Empty,
    Ready(ValueZipper),
}

impl Default for ValueBuilder {
    fn default() -> Self {
        Self::Empty
    }
}

macro_rules! raise {
    ($err:expr) => {{
        #[cfg(test)]
        {
            panic!("ZipperError: {}", $err);
        }

        #[cfg(not(test))]
        return Err($err);
    }};
}

impl ValueBuilder {
    // façade – these rely on the fact that root already exists; no clone needed
    pub fn enter_with<F>(
        &mut self,
        pc: Option<&PathComponent>,
        make_child: F,
    ) -> Result<(), ZipperError>
    where
        F: FnOnce() -> Value,
    {
        match pc {
            None if matches!(self, ValueBuilder::Empty) => {
                *self = ValueBuilder::Ready(ValueZipper::new(make_child()));
                Ok(())
            }
            None => {
                raise!(ZipperError::ExpectedEmptyPath)
            }
            Some(pc) => match self {
                ValueBuilder::Ready(z) => z.enter_lazy(pc.clone(), make_child),
                ValueBuilder::Empty => raise!(ZipperError::ExpectedNonEmptyPath),
            },
        }
    }

    pub fn set(&mut self, pc: Option<&PathComponent>, value: Value) -> Result<(), ZipperError> {
        match pc {
            None => {
                *self = ValueBuilder::Ready(ValueZipper::new(value));
                Ok(())
            }
            Some(pc) => match self {
                ValueBuilder::Ready(z) => z.set_at(pc.clone(), value),
                #[cfg_attr(coverage_nightly, coverage(off))]
                ValueBuilder::Empty => raise!(ZipperError::ExpectedEmptyPath),
            },
        }
    }

    pub fn mutate_with<D, M>(
        &mut self,
        pc: Option<&PathComponent>,
        make_default: D,
        mutator: M,
    ) -> Result<(), ZipperError>
    where
        D: FnOnce() -> Value,
        M: FnOnce(&mut Value) -> Result<(), ZipperError>,
    {
        match pc {
            None if matches!(self, ValueBuilder::Empty) => {
                let mut v = make_default();
                mutator(&mut v)?;
                *self = ValueBuilder::Ready(ValueZipper::new(v));
                Ok(())
            }
            None => match self {
                ValueBuilder::Ready(z) => mutator(z.current_mut()),
                #[cfg_attr(coverage_nightly, coverage(off))]
                ValueBuilder::Empty => raise!(ZipperError::ExpectedEmptyPath),
            },
            Some(pc) => match self {
                ValueBuilder::Ready(z) => z.mutate_lazy(pc.clone(), make_default, mutator),
                #[cfg_attr(coverage_nightly, coverage(off))]
                ValueBuilder::Empty => raise!(ZipperError::ExpectedNonEmptyPath),
            },
        }
    }

    #[allow(clippy::unnecessary_wraps)]
    pub fn pop(&mut self) -> Result<&mut Value, ZipperError> {
        match self {
            ValueBuilder::Ready(z) => Ok(z.pop()),
            ValueBuilder::Empty => raise!(ZipperError::ExpectedNonEmptyPath),
        }
    }

    #[inline]
    pub fn read_root(&self) -> Option<&Value> {
        match self {
            ValueBuilder::Ready(z) => Some(z.read_root()),
            ValueBuilder::Empty => None,
        }
    }

    #[inline]
    pub fn into_value(self) -> Option<Value> {
        match self {
            ValueBuilder::Ready(z) => Some(z.into_value()),
            ValueBuilder::Empty => None,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  4. StreamingParserBuilder – user‑facing façade
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
pub struct StreamingParserBuilder {
    parser: StreamingParser,
    state: ValueBuilder,
}

#[cfg(test)]
impl StreamingParserBuilder {
    pub fn new(options: ParserOptions) -> Self {
        Self {
            parser: StreamingParser::new(options),
            state: ValueBuilder::Empty,
        }
    }

    /// Feed more bytes.  Returns `(root_ref, events)` if any event was
    /// produced.
    pub fn parse_incremental(
        &mut self,
        buffer: &str,
    ) -> Result<Option<(&Value, Vec<ParseEvent>)>, ZipperError> {
        self.parser.feed(buffer);

        let mut events: Vec<ParseEvent> = Vec::new();
        for evt in self.parser.by_ref() {
            match evt {
                Ok(event) => events.push(event),
                Err(_) => {
                    // if the event is an error, we don't want to continue
                    return Err(ZipperError::ParserError);
                }
            }
        }

        for evt in &events {
            match evt {
                // scalars
                ParseEvent::Null { path } => {
                    self.state.set(path.last(), Value::Null)?;
                }
                ParseEvent::Boolean { path, value } => {
                    self.state.set(path.last(), (*value).into())?;
                }
                ParseEvent::Number { path, value } => {
                    self.state.set(path.last(), (*value).into())?;
                }
                ParseEvent::String { fragment, path, .. } => {
                    self.state.mutate_with(
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
                    use crate::value::Map;

                    self.state
                        .enter_with(path.last(), || Value::Object(Map::new()))?;
                }
                ParseEvent::ArrayStart { path } => {
                    self.state
                        .enter_with(path.last(), || Value::Array(Vec::new()))?;
                }

                // ── container ends ─────────────────────────────────────────
                ParseEvent::ArrayEnd { path, .. } | ParseEvent::ObjectEnd { path, .. } => {
                    if !path.is_empty() {
                        // don't pop past root
                        self.state.pop()?;
                    }
                }
            }
        }

        Ok(self.state.read_root().map(|v| (v, events)))
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec;
    use core::time::Duration;

    use rstest::*;

    use super::*; // bring StreamingParserBuilder etc.
    use crate::{
        event::PathComponent,
        value::{Map, Value},
    };

    fn default_opts() -> ParserOptions {
        ParserOptions {
            panic_on_error: true,
            ..ParserOptions::default()
        }
    }

    // ─────────────────────────────────────────────────────────────────────
    // 1. Root value is an object that contains nested arrays + partial string
    // ─────────────────────────────────────────────────────────────────────

    #[rstest]
    #[timeout(Duration::from_millis(1_000))]
    fn builds_complex_object_tree() {
        let mut b = StreamingParserBuilder::new(default_opts());

        // feed in two chunks – reproduces the example from the conversation
        b.parse_incremental("{\"a\":1, \"b\": [[\"foo\", [[1,2,3,\"fo")
            .unwrap();
        let (root, _) = b
            .parse_incremental("ur\"]]], \"bar\"]}")
            .unwrap()
            .expect("second call must produce events");

        // expected composite value
        let expected = Value::Object(
            [
                ("a".into(), Value::Number(1.into())),
                (
                    "b".into(),
                    Value::Array(vec![
                        Value::Array(vec![
                            Value::String("foo".into()),
                            Value::Array(vec![Value::Array(vec![
                                Value::Number(1.into()),
                                Value::Number(2.into()),
                                Value::Number(3.into()),
                                Value::String("four".into()),
                            ])]),
                        ]),
                        Value::String("bar".into()),
                    ]),
                ),
            ]
            .into_iter()
            .collect(),
        );

        assert_eq!(root, &expected);
    }

    // ─────────────────────────────────────────────────────────────────────
    // 2. Root value is a STRING streamed in two parts
    // ─────────────────────────────────────────────────────────────────────

    #[rstest]
    #[timeout(Duration::from_millis(250))]
    fn root_string_via_partial_chunks() {
        let mut b = StreamingParserBuilder::new(default_opts());

        // first chunk: opens quote + 3 chars
        b.parse_incremental("\"foo").unwrap();
        // second chunk: rest + closing quote
        let (root, _) = b
            .parse_incremental("bar\"")
            .unwrap()
            .expect("complete after two chunks");

        assert_eq!(root, &Value::String("foobar".into()));
    }

    // ─────────────────────────────────────────────────────────────────────
    // 3. Root value is a NUMBER (single chunk)
    // ─────────────────────────────────────────────────────────────────────

    #[rstest]
    #[timeout(Duration::from_millis(250))]
    fn root_number_single_chunk() {
        let mut b = StreamingParserBuilder::new(default_opts());
        let res = b.parse_incremental("123").unwrap();
        assert!(
            res.is_none(),
            "expected no events for single number chunk without EOF"
        );

        let mut b = StreamingParserBuilder::new(default_opts());
        let (root, _) = b
            .parse_incremental("123 ")
            .unwrap()
            .expect("events produced");

        assert_eq!(root, &Value::Number(123.into()));
    }

    #[rstest]
    #[timeout(Duration::from_millis(250))]
    fn root_number_single_chunk_repro_one() {
        let mut parser = StreamingParser::new(default_opts());
        parser.feed("123 ");

        let events: Vec<_> = parser.collect();
        assert!(events.iter().all(Result::is_ok), "all events should be ok");
        assert_eq!(
            events.len(),
            1,
            "expected one event for single number chunk with clear end"
        );
    }

    // ─────────────────────────────────────────────────────────────────────
    // 4. Empty input never produces events
    // ─────────────────────────────────────────────────────────────────────

    #[rstest]
    #[timeout(Duration::from_millis(250))]
    fn empty_call_returns_none() {
        let mut b = StreamingParserBuilder::new(default_opts());

        // assuming parse_incremental("") returns Ok(None)
        assert!(b.parse_incremental("").unwrap().is_none());
    }
    #[test]
    fn zipper_set_and_pop() {
        let mut zipper = ValueZipper::new(Value::Object(Map::new()));
        zipper
            .enter_lazy(PathComponent::Key("foo".into()), || Value::Array(vec![]))
            .unwrap();
        zipper
            .enter_lazy(PathComponent::Index(0), || Value::String("bar".into()))
            .unwrap();
        // Pop back to root
        zipper.pop();
        zipper.pop();
        let result = zipper.into_value();
        let expected = Value::Object(
            [(
                "foo".into(),
                Value::Array(vec![Value::String("bar".into())]),
            )]
            .into(),
        );
        assert_eq!(result, expected);
    }

    #[test]
    fn zipper_set_at_insert_and_overwrite() {
        let mut zipper = ValueZipper::new(Value::Object(Map::new()));
        // Insert new entry
        zipper
            .set_at(PathComponent::Key("k".into()), Value::Number(1.into()))
            .unwrap();
        // Consume zipper to inspect inserted value, then rebuild for overwrite test
        let v1 = zipper.into_value();
        assert_eq!(
            v1,
            Value::Object([("k".into(), Value::Number(1.into()))].into())
        );
        let mut zipper = ValueZipper::new(v1);
        // Overwrite existing entry
        zipper
            .set_at(PathComponent::Key("k".into()), Value::Number(2.into()))
            .unwrap();
        assert_eq!(
            zipper.into_value(),
            Value::Object([("k".into(), Value::Number(2.into()))].into())
        );
    }

    #[test]
    fn zipper_mutate_lazy_appends_to_string() {
        let mut zipper = ValueZipper::new(Value::Object(Map::new()));
        zipper
            .mutate_lazy(
                PathComponent::Key("s".into()),
                || Value::String(String::new()),
                |v| {
                    if let Value::String(s) = v {
                        s.push_str("hello");
                        Ok(())
                    } else {
                        Err(ZipperError::ExpectedString)
                    }
                },
            )
            .unwrap();
        let result = zipper.into_value();
        let expected = Value::Object([("s".into(), Value::String("hello".into()))].into());
        assert_eq!(result, expected);
    }

    #[test]
    fn zipper_errors_for_wrong_container() {
        let mut zipper = ValueZipper::new(Value::String("x".into()));
        assert_eq!(
            zipper.enter_lazy(PathComponent::Key("k".into()), || Value::Null),
            Err(ZipperError::ExpectedObject)
        );
        assert_eq!(
            zipper.enter_lazy(PathComponent::Index(0), || Value::Null),
            Err(ZipperError::ExpectedArray)
        );
    }

    #[test]
    fn builder_usage_simple() {
        let mut builder = ValueBuilder::default();
        assert!(builder.read_root().is_none());
        // Initialize root as an object
        builder
            .enter_with(None, || Value::Object(Map::new()))
            .unwrap();
        assert_eq!(builder.read_root(), Some(&Value::Object(Map::new())));
        // Enter and set a boolean child
        builder
            .enter_with(Some(&PathComponent::Key("a".into())), || {
                Value::Boolean(true)
            })
            .unwrap();
        assert_eq!(
            builder.into_value(),
            Some(Value::Object([("a".into(), Value::Boolean(true))].into()))
        );
    }

    #[test]
    #[should_panic(expected = "operation would pop past the root")]
    fn builder_pop_errors() {
        let mut builder = ValueBuilder::default();
        // Popping when empty should panic in test configuration
        builder.pop().unwrap();
    }
}
