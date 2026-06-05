use crate::{
    backend::{StdBackend, StdValueAssembler},
    buffer_options::BufferOptions,
    context::{BuilderCtx, EventCtx, PathCtx},
    jsonmodem_buffers::{
        BorrowedBufferedEvent, BufferError, JsonModemBuffers, JsonModemBuffersClosed,
        JsonModemBuffersIter, PathRoot, RootedBufferAssembler,
    },
    lending_iterator::LendingIterator,
    parser::ParserOptions,
    value::Value,
};

/// Value yielded by the streaming adapter.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamingValue<V = Value> {
    /// Monotonic identifier for the completed root.
    pub index: usize,
    /// Borrowed or owned JSON value.
    pub value: V,
    /// Whether the adapter has finished building the root.
    pub is_final: bool,
}

/// Configuration toggles for [`JsonModemValues`].
#[derive(Debug, Clone, Copy, Default)]
#[non_exhaustive]
pub struct ValuesOptions {
    /// Emit non-final snapshots while the root is still being parsed.
    pub(crate) partial: bool,
}

impl ValuesOptions {
    /// Enables emission of intermediate root snapshots.
    #[must_use]
    pub fn with_partial(mut self, partial: bool) -> Self {
        self.partial = partial;
        self
    }
}

/// Error surfaced while streaming values.
#[derive(Debug)]
pub enum ValuesError<Ctx: EventCtx> {
    /// Error reported by the underlying streaming parser.
    Parser(crate::parser::ParserError<Ctx>),
    /// Error reported while converting a parsed event into backend values.
    Assembler(Ctx::Error),
}

/// High-level adapter that maps streaming events to root values.
pub struct JsonModemValues<Ctx = StdBackend, A = StdValueAssembler>
where
    Ctx: BuilderCtx<Value = Value> + EventCtx + Default,
    Ctx::Path: PathRoot,
    A: RootedBufferAssembler<Ctx>,
{
    buffers: JsonModemBuffers<Ctx, A>,
    options: ValuesOptions,
    next_index: usize,
}

impl JsonModemValues<StdBackend, StdValueAssembler>
where
    <StdBackend as PathCtx>::Path: PathRoot,
{
    #[must_use]
    /// Construct a values adapter with default emission options.
    pub fn new(options: ParserOptions) -> Self {
        Self::with_options(options, ValuesOptions::default())
    }

    #[must_use]
    /// Construct a values adapter with explicit emission options.
    pub fn with_options(options: ParserOptions, opts: ValuesOptions) -> Self {
        let parser_options = options.with_allow_multiple_json_values(true);
        let buffers = JsonModemBuffers::new(parser_options, buffer_options_from_values(opts));
        Self {
            buffers,
            options: opts,
            next_index: 0,
        }
    }
}

impl<Ctx, A> JsonModemValues<Ctx, A>
where
    Ctx: BuilderCtx<Value = Value> + EventCtx + Default,
    Ctx::Path: PathRoot,
    A: RootedBufferAssembler<Ctx>,
{
    #[must_use]
    /// Build a values adapter using a custom buffers assembler.
    pub fn with_buffer_builder(options: ParserOptions, opts: ValuesOptions, builder: A) -> Self {
        Self {
            buffers: JsonModemBuffers::with_builder(options, builder),
            options: opts,
            next_index: 0,
        }
    }

    #[must_use]
    /// Returns a read-only view of the current root value.
    pub fn view_root(&self) -> &Ctx::Value {
        self.buffers.read_root()
    }

    /// Feeds a chunk of JSON text and returns a lending iterator over values.
    pub fn feed<'a>(&'a mut self, chunk: &'a str) -> JsonModemValuesIter<'a, Ctx, A> {
        JsonModemValuesIter {
            events: self.buffers.feed(chunk),
            options: self.options,
            next_index: &mut self.next_index,
        }
    }

    /// Marks the end of the stream and drains any pending values.
    pub fn finish(self) -> JsonModemValuesClosed<Ctx, A> {
        JsonModemValuesClosed {
            events: self.buffers.finish(),
            options: self.options,
            next_index: self.next_index,
        }
    }
}

/// Lending iterator yielding streaming value references.
pub struct JsonModemValuesIter<'a, Ctx, A>
where
    Ctx: BuilderCtx<Value = Value> + EventCtx,
    Ctx::Path: PathRoot,
    A: RootedBufferAssembler<Ctx>,
{
    events: JsonModemBuffersIter<'a, Ctx, A>,
    options: ValuesOptions,
    next_index: &'a mut usize,
}

impl<Ctx, A> JsonModemValuesIter<'_, Ctx, A>
where
    Ctx: BuilderCtx<Value = Value> + EventCtx,
    Ctx::Path: PathRoot,
    A: RootedBufferAssembler<Ctx>,
{
    fn next_value_ref(&mut self) -> Option<Result<StreamingValue<&Ctx::Value>, ValuesError<Ctx>>> {
        next_value_for_source(&mut self.events, self.options, self.next_index)
    }

    #[allow(clippy::wrong_self_convention)]
    pub fn to_iter(
        mut self,
    ) -> impl Iterator<Item = Result<StreamingValue<Value>, ValuesError<Ctx>>> {
        core::iter::from_fn(move || Iterator::next(&mut self))
    }
}

impl<Ctx, A> Iterator for JsonModemValuesIter<'_, Ctx, A>
where
    Ctx: BuilderCtx<Value = Value> + EventCtx,
    Ctx::Path: PathRoot,
    A: RootedBufferAssembler<Ctx>,
{
    type Item = Result<StreamingValue<Value>, ValuesError<Ctx>>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_value_ref().map(clone_streaming_value)
    }
}

impl<Ctx, A> LendingIterator for JsonModemValuesIter<'_, Ctx, A>
where
    Ctx: BuilderCtx<Value = Value> + EventCtx,
    Ctx::Path: PathRoot,
    A: RootedBufferAssembler<Ctx>,
{
    type Item<'b>
        = Result<StreamingValue<&'b Ctx::Value>, ValuesError<Ctx>>
    where
        Self: 'b;

    fn next(&mut self) -> Option<Self::Item<'_>> {
        self.next_value_ref()
    }
}

/// Iterator draining remaining values after finishing the stream.
pub struct JsonModemValuesClosed<Ctx, A>
where
    Ctx: BuilderCtx<Value = Value> + EventCtx,
    Ctx::Path: PathRoot,
    A: RootedBufferAssembler<Ctx>,
{
    events: JsonModemBuffersClosed<Ctx, A>,
    options: ValuesOptions,
    next_index: usize,
}

impl<Ctx, A> JsonModemValuesClosed<Ctx, A>
where
    Ctx: BuilderCtx<Value = Value> + EventCtx,
    Ctx::Path: PathRoot,
    A: RootedBufferAssembler<Ctx>,
{
    fn next_value_ref(&mut self) -> Option<Result<StreamingValue<&Ctx::Value>, ValuesError<Ctx>>> {
        next_value_for_source(&mut self.events, self.options, &mut self.next_index)
    }

    #[allow(clippy::wrong_self_convention)]
    pub fn to_iter(
        mut self,
    ) -> impl Iterator<Item = Result<StreamingValue<Value>, ValuesError<Ctx>>> {
        core::iter::from_fn(move || Iterator::next(&mut self))
    }
}

impl<Ctx, A> Iterator for JsonModemValuesClosed<Ctx, A>
where
    Ctx: BuilderCtx<Value = Value> + EventCtx,
    Ctx::Path: PathRoot,
    A: RootedBufferAssembler<Ctx>,
{
    type Item = Result<StreamingValue<Value>, ValuesError<Ctx>>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_value_ref().map(clone_streaming_value)
    }
}

impl<Ctx, A> LendingIterator for JsonModemValuesClosed<Ctx, A>
where
    Ctx: BuilderCtx<Value = Value> + EventCtx,
    Ctx::Path: PathRoot,
    A: RootedBufferAssembler<Ctx>,
{
    type Item<'b>
        = Result<StreamingValue<&'b Ctx::Value>, ValuesError<Ctx>>
    where
        Self: 'b;

    fn next(&mut self) -> Option<Self::Item<'_>> {
        self.next_value_ref()
    }
}

fn buffer_options_from_values(_opts: ValuesOptions) -> BufferOptions {
    BufferOptions::default()
}

fn convert_error<Ctx: EventCtx>(err: BufferError<Ctx>) -> ValuesError<Ctx> {
    match err {
        BufferError::Parser(err) => ValuesError::Parser(err),
        BufferError::Assembler(err) => ValuesError::Assembler(err),
    }
}

fn clone_streaming_value<Ctx>(
    result: Result<StreamingValue<&Ctx::Value>, ValuesError<Ctx>>,
) -> Result<StreamingValue<Value>, ValuesError<Ctx>>
where
    Ctx: BuilderCtx<Value = Value> + EventCtx,
{
    result.map(|borrowed| StreamingValue {
        index: borrowed.index,
        value: borrowed.value.clone(),
        is_final: borrowed.is_final,
    })
}

trait ValueSource<Ctx>
where
    Ctx: BuilderCtx<Value = Value> + EventCtx,
    Ctx::Path: PathRoot,
{
    fn next_event(&mut self) -> Option<Result<BorrowedBufferedEvent<'_, Ctx>, BufferError<Ctx>>>;
    fn root(&self) -> &Ctx::Value;
}

impl<Ctx, A> ValueSource<Ctx> for JsonModemBuffersIter<'_, Ctx, A>
where
    Ctx: BuilderCtx<Value = Value> + EventCtx,
    Ctx::Path: PathRoot,
    A: RootedBufferAssembler<Ctx>,
{
    fn next_event(&mut self) -> Option<Result<BorrowedBufferedEvent<'_, Ctx>, BufferError<Ctx>>> {
        LendingIterator::next(self)
    }

    fn root(&self) -> &Ctx::Value {
        self.root()
    }
}

impl<Ctx, A> ValueSource<Ctx> for JsonModemBuffersClosed<Ctx, A>
where
    Ctx: BuilderCtx<Value = Value> + EventCtx,
    Ctx::Path: PathRoot,
    A: RootedBufferAssembler<Ctx>,
{
    fn next_event(&mut self) -> Option<Result<BorrowedBufferedEvent<'_, Ctx>, BufferError<Ctx>>> {
        LendingIterator::next(self)
    }

    fn root(&self) -> &Ctx::Value {
        self.root()
    }
}

fn next_value_for_source<'a, Ctx, S>(
    source: &'a mut S,
    options: ValuesOptions,
    next_index: &'a mut usize,
) -> Option<Result<StreamingValue<&'a Ctx::Value>, ValuesError<Ctx>>>
where
    Ctx: BuilderCtx<Value = Value> + EventCtx,
    Ctx::Path: PathRoot,
    S: ValueSource<Ctx>,
{
    let mut saw_partial = false;
    match next_emit_kind(source, options, &mut saw_partial) {
        Some(Ok(kind)) => {
            let is_final = kind == EmitKind::Final;
            let root = source.root();
            let streaming = emit_value::<Ctx>(root, next_index, is_final);
            Some(Ok(streaming))
        }
        Some(Err(err)) => Some(Err(err)),
        None => None,
    }
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum EmitKind {
    Partial,
    Final,
}

fn classify_event<Ctx>(
    event: &BorrowedBufferedEvent<'_, Ctx>,
    partial_enabled: bool,
    saw_partial: &mut bool,
) -> Option<EmitKind>
where
    Ctx: BuilderCtx + EventCtx,
    Ctx::Path: PathRoot,
{
    let path_is_root = |path: &&Ctx::Path| path.is_root();

    match event {
        BorrowedBufferedEvent::String { path, is_final, .. } => {
            if path_is_root(path) {
                if *is_final {
                    Some(EmitKind::Final)
                } else {
                    *saw_partial = true;
                    partial_enabled.then_some(EmitKind::Partial)
                }
            } else {
                *saw_partial = true;
                None
            }
        }
        BorrowedBufferedEvent::ArrayBegin { .. } | BorrowedBufferedEvent::ObjectBegin { .. } => {
            *saw_partial = true;
            None
        }
        BorrowedBufferedEvent::ArrayEnd { path, .. }
        | BorrowedBufferedEvent::ObjectEnd { path, .. }
        | BorrowedBufferedEvent::Null { path }
        | BorrowedBufferedEvent::Boolean { path, .. }
        | BorrowedBufferedEvent::Number { path, .. } => {
            if path_is_root(path) {
                Some(EmitKind::Final)
            } else {
                *saw_partial = true;
                None
            }
        }
    }
}

fn next_emit_kind<Ctx, S>(
    source: &mut S,
    options: ValuesOptions,
    saw_partial: &mut bool,
) -> Option<Result<EmitKind, ValuesError<Ctx>>>
where
    Ctx: BuilderCtx<Value = Value> + EventCtx,
    Ctx::Path: PathRoot,
    S: ValueSource<Ctx>,
{
    let mut saw_any = false;
    while let Some(event) = source.next_event() {
        match event {
            Ok(buffered) => {
                saw_any = true;
                if let Some(kind) = classify_event(&buffered, options.partial, saw_partial) {
                    return Some(Ok(kind));
                }
            }
            Err(err) => return Some(Err(convert_error(err))),
        }
    }

    if saw_any && *saw_partial && options.partial {
        Some(Ok(EmitKind::Partial))
    } else {
        None
    }
}

fn emit_value<'a, Ctx>(
    value: &'a Ctx::Value,
    next_index: &mut usize,
    is_final: bool,
) -> StreamingValue<&'a Ctx::Value>
where
    Ctx: BuilderCtx + EventCtx,
{
    let index = *next_index;
    if is_final {
        *next_index += 1;
    }
    StreamingValue {
        index,
        value,
        is_final,
    }
}
