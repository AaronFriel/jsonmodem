use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::{cmp::Ordering, ptr::NonNull};

use super::value_builder::ApplyOutcome;
#[cfg(test)]
use crate::Path;
use crate::{
    PathItem,
    buffered::{
        value::Value,
        value_builder::{ValueAssembler, ValueBuilder},
    },
    parser::{ParseEvent, StdFactory},
};

#[derive(Debug)]
pub struct ValueZipper {
    root: Box<Value>,
    path: Vec<NonNull<Value>>,
    #[cfg(test)]
    path_components: Path,
}

impl ValueZipper {
    #[inline]
    pub fn new(value: Value) -> Self {
        Self {
            root: Box::new(value),
            path: Vec::with_capacity(8),
            #[cfg(test)]
            path_components: Vec::new(),
        }
    }

    #[inline]
    fn current_mut(&mut self) -> &mut Value {
        match self.path.last().copied().as_mut() {
            // SAFETY: `ptr` came from `NonNull::from` on a `&mut Value` (see
            // `enter_*_lazy`).  It is still valid because:
            //
            //   * While it is stored in `self.path`, the collection that owns that element is
            //     *never* mutated (the invariant above).
            //   * We hold `&mut self`, so no other mutable reference to the same element can exist
            //     simultaneously (unique-access rule).
            //
            // Consequently `ptr` is non-null, properly aligned, and points to live
            // memory for the duration of this call.
            Some(ptr) => unsafe { ptr.as_mut() },
            None => self.root.as_mut(),
        }
    }

    // ─── public clone‑free operations ──────────────────────────────────────

    #[inline]
    pub fn enter_lazy<FN, FFac>(
        &mut self,
        pc: PathItem,
        f: &mut FFac,
        make_child: FN,
    ) -> Result<(), ZipperError>
    where
        FFac: ValueBuilder<Value = Value>,
        FN: FnOnce(&mut FFac) -> Value,
    {
        match pc {
            PathItem::Key(k) => self.enter_key_lazy(k, f, make_child),
            PathItem::Index(i) => self.enter_index_lazy(i, f, make_child),
        }
    }

    #[inline]
    pub fn set_at<FFac: ValueBuilder<Value = Value>>(
        &mut self,
        pc: PathItem,
        value: Value,
        f: &mut FFac,
    ) -> Result<(), ZipperError> {
        match pc {
            PathItem::Key(k) => self.modify_or_insert_key(
                f,
                k,
                value,
                |v, _| v,
                |new, entry, _| {
                    if let Some(e) = entry {
                        *e = new;
                        Ok(())
                    } else {
                        Err(ZipperError::ExpectedNonEmptyPath)
                    }
                },
            ),
            PathItem::Index(i) => self.modify_or_insert_index(
                f,
                i,
                value,
                |v, _| v,
                |new, entry, _| {
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

    #[inline]
    pub fn mutate_lazy<D, M, FFac>(
        &mut self,
        pc: PathItem,
        f: &mut FFac,
        make_default: D,
        mutator: M,
    ) -> Result<(), ZipperError>
    where
        FFac: ValueBuilder<Value = Value>,
        D: FnOnce(&mut FFac) -> Value,
        M: FnOnce(&mut Value, &mut FFac) -> Result<(), ZipperError>,
    {
        match pc {
            PathItem::Key(k) => self.modify_or_insert_key(
                f,
                k,
                (), // zero‑sized token
                |(), fac| make_default(fac),
                |(), entry, fac| {
                    if let Some(v) = entry {
                        mutator(v, fac)?;
                    }
                    Ok(())
                },
            ),
            PathItem::Index(i) => self.modify_or_insert_index(
                f,
                i,
                (),
                |(), fac| make_default(fac),
                |(), entry, fac| {
                    if let Some(v) = entry {
                        mutator(v, fac)?;
                    }
                    Ok(())
                },
            ),
        }
    }

    #[inline]
    pub fn pop(&mut self) -> &mut Value {
        let leaf = match self.path.pop().as_mut() {
            // SAFETY: identical reasoning as in `current_mut`:
            //
            //   * The pointer was created with `NonNull::from(&mut value)` and remained valid
            //     because we did not touch its parent container until *after* removing it from
            //     `self.path`.
            //   * We still hold `&mut self`, so there is no aliasing.
            //
            // Note that `pop` hands the caller an `&mut Value` whose lifetime is tied
            // to `&mut self`.  The borrow checker therefore prevents the caller from
            // calling any other `&mut self` methods on the zipper while the returned
            // reference is alive, upholding Rust’s exclusive-access guarantee.
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

    #[inline]
    fn modify_or_insert_key<T, Init, Func, FFac>(
        &mut self,
        f: &mut FFac,
        k: Arc<str>,
        default: T,
        initializer: Init,
        func: Func,
    ) -> Result<(), ZipperError>
    where
        FFac: ValueBuilder<Value = Value>,
        T: Clone,
        Init: FnOnce(T, &mut FFac) -> Value,
        Func: FnOnce(T, Option<&mut Value>, &mut FFac) -> Result<(), ZipperError>,
    {
        let Some(obj) = Value::as_object_mut(self.current_mut()) else {
            return Err(ZipperError::ExpectedObject);
        };

        if let Some(child) = obj.get_mut(&k) {
            return func(default, Some(child), f);
        }

        let cloned_default = default.clone();
        let new_child = initializer(default, f);
        let child_ref = obj.entry(k.clone()).or_insert(new_child);
        func(cloned_default, Some(child_ref), f)
    }

    #[inline]
    fn modify_or_insert_index<T, Init, Func, FFac>(
        &mut self,
        f: &mut FFac,
        index: usize,
        default: T,
        initializer: Init,
        func: Func,
    ) -> Result<(), ZipperError>
    where
        FFac: ValueBuilder<Value = Value>,
        T: Clone,
        Init: FnOnce(T, &mut FFac) -> Value,
        Func: FnOnce(T, Option<&mut Value>, &mut FFac) -> Result<(), ZipperError>,
    {
        let Some(arr) = Value::as_array_mut(self.current_mut()) else {
            return Err(ZipperError::ExpectedArray);
        };

        let index_usize: usize = index.into();
        match index_usize.cmp(&arr.len()) {
            core::cmp::Ordering::Less => {
                let elem = arr.get_mut(index).ok_or(ZipperError::InvalidArrayIndex)?;
                func(default, Some(elem), f)
            }
            core::cmp::Ordering::Equal => {
                let cloned_default = default.clone();
                let new_child = initializer(default, f);
                arr.push(new_child);
                let elem_ref = arr.last_mut().ok_or(ZipperError::InvalidArrayIndex)?;
                func(cloned_default, Some(elem_ref), f)
            }
            core::cmp::Ordering::Greater => Err(ZipperError::InvalidArrayIndex),
        }
    }

    #[inline]
    fn enter_key_lazy<FN, FFac>(
        &mut self,
        k: Arc<str>,
        f: &mut FFac,
        make_child: FN,
    ) -> Result<(), ZipperError>
    where
        FFac: ValueBuilder<Value = Value>,
        FN: FnOnce(&mut FFac) -> Value,
    {
        #[cfg(test)]
        self.path_components.push(PathItem::Key(k.clone()));

        let obj = Value::as_object_mut(self.current_mut()).ok_or(ZipperError::ExpectedObject)?;

        let child_ptr = if let Some(child) = obj.get_mut(&k) {
            core::ptr::NonNull::from(child)
        } else {
            let new_child = make_child(f);
            let child_ref = obj.entry(k).or_insert(new_child);
            core::ptr::NonNull::from(child_ref)
        };

        self.path.push(child_ptr);
        Ok(())
    }

    #[inline]
    fn enter_index_lazy<FN, FFac>(
        &mut self,
        index: usize,
        f: &mut FFac,
        make_child: FN,
    ) -> Result<(), ZipperError>
    where
        FFac: ValueBuilder<Value = Value>,
        FN: FnOnce(&mut FFac) -> Value,
    {
        #[cfg(test)]
        self.path_components.push(PathItem::Index(index));

        let arr = Value::as_array_mut(self.current_mut()).ok_or(ZipperError::ExpectedArray)?;

        let child_ptr = match index.cmp(&arr.len()) {
            Ordering::Less => {
                let elem = arr.get_mut(index).ok_or(ZipperError::InvalidArrayIndex)?;
                NonNull::from(elem)
            }
            Ordering::Equal => {
                let val = make_child(f);
                arr.push(val);
                let elem = arr.last_mut().ok_or(ZipperError::InvalidArrayIndex)?;
                NonNull::from(elem)
            }
            Ordering::Greater => return Err(ZipperError::InvalidArrayIndex),
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
        write!(
            f,
            "{}",
            match self {
                Self::ExpectedObject => "expected an object at the current path",
                Self::ExpectedArray => "expected an array at the current path",
                Self::InvalidArrayIndex => "invalid array index",
                Self::ExpectedEmptyPath => "operation requires an empty path",
                Self::ExpectedNonEmptyPath => "operation would pop past the root",
                Self::ExpectedString => "expected the root to be a string",
                #[cfg(test)]
                Self::ParserError => "parser error occurred",
            }
        )
    }
}
impl core::error::Error for ZipperError {}

#[derive(Debug)]
enum BuilderState {
    Empty,
    Ready(ValueZipper),
}

#[derive(Debug)]
pub struct ZipperValueBuilder {
    state: BuilderState,
}

impl Default for ZipperValueBuilder {
    fn default() -> Self {
        Self {
            state: BuilderState::Empty,
        }
    }
}

macro_rules! raise {
    ($err:expr) => {
        return Err($err)
    };
}

impl ValueAssembler<StdFactory> for ZipperValueBuilder {
    fn apply(&mut self, _evt: &ParseEvent<StdFactory>) -> ApplyOutcome<'_, StdFactory> {
        todo!()
    }

    fn root(&self) -> Option<&<StdFactory as ValueBuilder>::Value> {
        todo!()
    }

    fn into_root(self) -> Option<<StdFactory as ValueBuilder>::Value> {
        todo!()
    }
}

impl ZipperValueBuilder {
    #[inline]
    pub fn enter_with<G, FFac>(
        &mut self,
        pc: Option<&PathItem>,
        f: &mut FFac,
        make_child: G,
    ) -> Result<(), ZipperError>
    where
        FFac: ValueBuilder<Value = Value>,
        G: FnOnce(&mut FFac) -> Value,
    {
        match pc {
            None if matches!(self.state, BuilderState::Empty) => {
                self.state = BuilderState::Ready(ValueZipper::new(make_child(f)));
                Ok(())
            }
            None => {
                raise!(ZipperError::ExpectedEmptyPath)
            }
            Some(pc) => match &mut self.state {
                BuilderState::Ready(z) => z.enter_lazy(pc.clone(), f, make_child),
                BuilderState::Empty => raise!(ZipperError::ExpectedNonEmptyPath),
            },
        }
    }

    #[inline]
    pub fn set<FFac: ValueBuilder<Value = Value>>(
        &mut self,
        pc: Option<&PathItem>,
        value: Value,
        f: &mut FFac,
    ) -> Result<(), ZipperError> {
        match pc {
            None => {
                self.state = BuilderState::Ready(ValueZipper::new(value));
                Ok(())
            }
            Some(pc) => match &mut self.state {
                BuilderState::Ready(z) => z.set_at(pc.clone(), value, f),
                #[cfg_attr(coverage_nightly, coverage(off))]
                BuilderState::Empty => raise!(ZipperError::ExpectedEmptyPath),
            },
        }
    }

    #[inline]
    pub fn mutate_with<D, M, FFac>(
        &mut self,
        f: &mut FFac,
        pc: Option<&PathItem>,
        make_default: D,
        mutator: M,
    ) -> Result<(), ZipperError>
    where
        FFac: ValueBuilder<Value = Value>,
        D: FnOnce(&mut FFac) -> Value,
        M: FnOnce(&mut Value, &mut FFac) -> Result<(), ZipperError>,
    {
        match pc {
            None if matches!(self.state, BuilderState::Empty) => {
                let mut v = make_default(f);
                mutator(&mut v, f)?;
                self.state = BuilderState::Ready(ValueZipper::new(v));
                Ok(())
            }
            None => match &mut self.state {
                BuilderState::Ready(z) => mutator(z.current_mut(), f),
                #[cfg_attr(coverage_nightly, coverage(off))]
                BuilderState::Empty => raise!(ZipperError::ExpectedEmptyPath),
            },
            Some(pc) => match &mut self.state {
                BuilderState::Ready(z) => z.mutate_lazy(pc.clone(), f, make_default, mutator),
                #[cfg_attr(coverage_nightly, coverage(off))]
                BuilderState::Empty => raise!(ZipperError::ExpectedNonEmptyPath),
            },
        }
    }

    #[inline]
    pub fn pop(&mut self) -> Result<&mut Value, ZipperError> {
        match &mut self.state {
            BuilderState::Ready(z) => Ok(z.pop()),
            BuilderState::Empty => raise!(ZipperError::ExpectedNonEmptyPath),
        }
    }

    #[inline]
    pub fn read_root(&self) -> Option<&Value> {
        match &self.state {
            BuilderState::Ready(z) => Some(z.read_root()),
            BuilderState::Empty => None,
        }
    }

    #[inline]
    pub fn into_value(self) -> Option<Value> {
        match self.state {
            BuilderState::Ready(z) => Some(z.into_value()),
            BuilderState::Empty => None,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  4. StreamingParserBuilder – user‑facing façade
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
pub mod test_util {
    use alloc::{collections::BTreeMap, string::String, vec::Vec};

    use super::*;
    use crate::{
        parser::{EventBuilder, ParseEvent, ParserOptions, StdFactory, StreamingParserImpl},
        *,
    };
    type DefaultStreamingParser = StreamingParserImpl<StdFactory>;

    pub struct StreamingParserBuilder {
        parser: DefaultStreamingParser,
        state: ZipperValueBuilder,
    }

    impl StreamingParserBuilder {
        pub fn new(options: ParserOptions) -> Self {
            Self {
                parser: DefaultStreamingParser::new(options),
                state: ZipperValueBuilder::default(),
            }
        }

        /// Feed more bytes.  Returns `(root_ref, events)` if any event was
        /// produced.
        pub fn parse_incremental(
            &mut self,
            buffer: &str,
        ) -> Result<Option<(&Value, Vec<ParseEvent>)>, ZipperError> {
            let mut events: Vec<ParseEvent> = Vec::new();
            for evt in self.parser.feed(buffer) {
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
                        self.state.set(path.last(), Value::Null, &mut StdFactory)?;
                    }
                    ParseEvent::Boolean { path, value } => {
                        self.state
                            .set(path.last(), Value::Boolean(*value), &mut StdFactory)?;
                    }
                    ParseEvent::Number { path, value } => {
                        self.state
                            .set(path.last(), Value::Number(*value), &mut StdFactory)?;
                    }
                    ParseEvent::String { fragment, path, .. } => {
                        self.state.mutate_with(
                            &mut StdFactory,
                            path.last(),
                            |_| Value::String(String::new()),
                            |v, _| {
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
                        self.state.enter_with(path.last(), &mut StdFactory, |_| {
                            Value::Object(BTreeMap::new())
                        })?;
                    }
                    ParseEvent::ArrayBegin { path } => {
                        self.state.enter_with(path.last(), &mut StdFactory, |_| {
                            Value::Array(Vec::new())
                        })?;
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
}

#[cfg(all(test, feature = "todo"))]
mod tests {
    use alloc::vec;
    use core::time::Duration;

    use rstest::*;

    use super::*;
    use crate::{value_zipper::test_util::StreamingParserBuilder, *};

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
        let mut parser = DefaultStreamingParser::new(default_opts());
        let events: Vec<_> = parser.feed("123 ").collect();
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
            .enter_lazy(PathItem::Key("foo".into()), &mut StdFactory, |_| {
                Value::Array(vec![])
            })
            .unwrap();
        zipper
            .enter_lazy(PathItem::Index(0), &mut StdFactory, |_| {
                Value::String("bar".into())
            })
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
            .set_at(
                PathItem::Key("k".into()),
                Value::Number(1.into()),
                &mut StdFactory,
            )
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
            .set_at(
                PathItem::Key("k".into()),
                Value::Number(2.into()),
                &mut StdFactory,
            )
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
                PathItem::Key("s".into()),
                &mut StdFactory,
                |_| Value::String(Str::new()),
                |v, _| {
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
            zipper.enter_lazy(PathItem::Key("k".into()), &mut StdFactory, |_| {
                Value::Null
            }),
            Err(ZipperError::ExpectedObject)
        );
        assert_eq!(
            zipper.enter_lazy(PathItem::Index(0), &mut StdFactory, |_| { Value::Null }),
            Err(ZipperError::ExpectedArray)
        );
    }

    #[test]
    fn builder_usage_simple() {
        let mut builder = ValueBuilder::default();
        assert!(builder.read_root().is_none());
        // Initialize root as an object
        builder
            .enter_with(None, &mut StdFactory, |_| Value::Object(Map::new()))
            .unwrap();
        assert_eq!(builder.read_root(), Some(&Value::Object(Map::new())));
        // Enter and set a boolean child
        builder
            .enter_with(Some(&PathItem::Key("a".into())), &mut StdFactory, |_| {
                Value::Boolean(true)
            })
            .unwrap();
        assert_eq!(
            builder.into_value(),
            Some(Value::Object([("a".into(), Value::Boolean(true))].into()))
        );
    }

    #[test]
    fn builder_pop_errors() {
        let mut builder = ValueBuilder::<Value>::default();
        // Popping when empty should yield an error
        assert_eq!(builder.pop(), Err(ZipperError::ExpectedNonEmptyPath));
    }
}
