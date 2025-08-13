#![allow(missing_docs)]

use alloc::vec::Vec;

use crate::{
    buffer_options::BufferOptions,
    context::{BuilderCtx, EventCtx, OwnedEventCtx, PathCtx},
    event::ParseEvent,
    lending_iterator::LendingIterator,
    parser::{JsonModem, JsonModemClosed, JsonModemIterator, ParserError, ParserOptions},
    path::PathItem,
};

/// Buffered representation of parse events emitted by [`JsonModemBuffers`].
///
/// All fields borrow from the adapter for the duration of a single
/// `LendingIterator::next` call. Use [`BufferedEvent::map_path`] to change the
/// path type when converting into an owned iterator.
#[derive(Debug)]
pub enum BufferedEvent<'ev, P, Ctx>
where
    Ctx: BuilderCtx + EventCtx,
{
    /// A `null` scalar.
    Null { path: P },
    /// A boolean scalar.
    Boolean { path: P, value: Ctx::Bool },
    /// A numeric scalar.
    Number { path: P, value: Ctx::Num<'ev> },
    /// A string fragment, optionally carrying a buffered value.
    String {
        path: P,
        fragment: Ctx::Str<'ev>,
        value: Option<Ctx::Str<'ev>>,
        is_initial: bool,
        is_final: bool,
    },
    /// Marks the start of an array.
    ArrayBegin { path: P },
    /// Marks the end of an array, optionally carrying buffered content.
    ArrayEnd {
        path: P,
        value: Option<&'ev Ctx::Array>,
    },
    /// Marks the start of an object.
    ObjectBegin { path: P },
    /// Marks the end of an object, optionally carrying buffered content.
    ObjectEnd {
        path: P,
        value: Option<&'ev Ctx::Object>,
    },
}

pub type BorrowedBufferedEvent<'ev, Ctx> = BufferedEvent<'ev, &'ev <Ctx as PathCtx>::Path, Ctx>;

impl<'ev, P, Ctx> BufferedEvent<'ev, P, Ctx>
where
    Ctx: BuilderCtx + EventCtx,
{
    #[must_use]
    pub fn path(&self) -> &P {
        match self {
            Self::Null { path }
            | Self::Boolean { path, .. }
            | Self::Number { path, .. }
            | Self::String { path, .. }
            | Self::ArrayBegin { path }
            | Self::ArrayEnd { path, .. }
            | Self::ObjectBegin { path }
            | Self::ObjectEnd { path, .. } => path,
        }
    }

    #[must_use]
    pub fn map_path<U>(self, f: impl FnOnce(P) -> U) -> BufferedEvent<'ev, U, Ctx> {
        match self {
            Self::Null { path } => BufferedEvent::Null { path: f(path) },
            Self::Boolean { path, value } => BufferedEvent::Boolean {
                path: f(path),
                value,
            },
            Self::Number { path, value } => BufferedEvent::Number {
                path: f(path),
                value,
            },
            Self::String {
                path,
                fragment,
                value,
                is_initial,
                is_final,
            } => BufferedEvent::String {
                path: f(path),
                fragment,
                value,
                is_initial,
                is_final,
            },
            Self::ArrayBegin { path } => BufferedEvent::ArrayBegin { path: f(path) },
            Self::ArrayEnd { path, value } => BufferedEvent::ArrayEnd {
                path: f(path),
                value,
            },
            Self::ObjectBegin { path } => BufferedEvent::ObjectBegin { path: f(path) },
            Self::ObjectEnd { path, value } => BufferedEvent::ObjectEnd {
                path: f(path),
                value,
            },
        }
    }

    #[must_use]
    pub fn with_path<U>(self, path: U) -> BufferedEvent<'ev, U, Ctx> {
        self.map_path(|_| path)
    }
}

impl<'ev, P, Ctx> From<BufferedEvent<'ev, &'_ P, Ctx>> for BufferedEvent<'ev, P, Ctx>
where
    P: Clone,
    Ctx: BuilderCtx + EventCtx,
{
    fn from(value: BufferedEvent<'ev, &'_ P, Ctx>) -> Self {
        value.map_path(Clone::clone)
    }
}

#[derive(Debug)]
pub enum BufferError<Ctx: EventCtx> {
    Parser(ParserError<Ctx>),
    Assembler(Ctx::Error),
}

pub trait BufferAssembler<Ctx>
where
    Ctx: BuilderCtx + EventCtx,
{
    // 'src is the input source of the parse event, but as we buffer and reborrow
    // based on the assembler, we declare 'src: 'a.
    fn on_event<'a, 'src: 'a>(
        &'a mut self,
        event: ParseEvent<'src, &'a <Ctx as PathCtx>::Path, Ctx>,
    ) -> Result<BorrowedBufferedEvent<'a, Ctx>, Ctx::Error>;
}

pub trait RootedBufferAssembler<Ctx>: BufferAssembler<Ctx>
where
    Ctx: BuilderCtx + EventCtx,
    Ctx::Path: PathRoot,
{
    fn root(&self) -> &Ctx::Value;
}

pub trait PathRoot {
    fn is_root(&self) -> bool;
}

impl<T: PathRoot + ?Sized> PathRoot for &T {
    fn is_root(&self) -> bool {
        (**self).is_root()
    }
}

impl<K, I> PathRoot for Vec<PathItem<K, I>> {
    fn is_root(&self) -> bool {
        self.is_empty()
    }
}

impl<K, I> PathRoot for [PathItem<K, I>] {
    fn is_root(&self) -> bool {
        self.is_empty()
    }
}

/// Coalesces streaming events into buffered [`BufferedEvent`] values.
pub struct JsonModemBuffers<Ctx = crate::backend::StdBackend, A = crate::backend::StdValueAssembler>
where
    Ctx: BuilderCtx + EventCtx + Default,
    A: BufferAssembler<Ctx>,
{
    modem: JsonModem<Ctx>,
    builder: A,
}

impl JsonModemBuffers<crate::backend::StdBackend, crate::backend::StdValueAssembler> {
    /// Creates a new buffered adapter with the provided parser and buffering
    /// options.
    #[must_use]
    pub fn new(options: ParserOptions, buffer: BufferOptions) -> Self {
        Self::with_builder(options, crate::backend::StdValueAssembler::new(buffer))
    }
}

impl JsonModemBuffers<crate::backend::StdBackend, crate::backend::StdStringAssembler> {
    #[must_use]
    pub fn string(options: ParserOptions, buffer: BufferOptions) -> Self {
        Self::with_builder(options, crate::backend::StdStringAssembler::new(buffer))
    }
}

impl<Ctx, A> JsonModemBuffers<Ctx, A>
where
    Ctx: BuilderCtx + EventCtx + Default,
    A: BufferAssembler<Ctx>,
{
    /// Creates a new buffered adapter from parser options and a ready-made
    /// builder.
    #[must_use]
    pub fn with_builder(options: ParserOptions, builder: A) -> Self {
        Self {
            modem: JsonModem::new(options),
            builder,
        }
    }

    /// Feeds the next chunk of JSON text and returns a lending iterator over
    /// buffered events.
    pub fn feed<'a>(&'a mut self, chunk: &'a str) -> JsonModemBuffersIter<'a, Ctx, A> {
        JsonModemBuffersIter {
            inner: self.modem.feed(chunk),
            builder: &mut self.builder,
        }
    }

    /// Finishes the stream and drains the remaining buffered events.
    pub fn finish(self) -> JsonModemBuffersClosed<Ctx, A> {
        JsonModemBuffersClosed {
            builder: self.builder,
            inner: self.modem.finish(),
        }
    }

    pub fn read_root(&self) -> &Ctx::Value
    where
        A: RootedBufferAssembler<Ctx>,
        <Ctx as PathCtx>::Path: PathRoot,
    {
        self.builder.root()
    }
}

/// Iterator producing buffered events for a stream.
pub struct JsonModemBuffersIter<'a, Ctx, A>
where
    Ctx: BuilderCtx + EventCtx,
    A: BufferAssembler<Ctx>,
{
    inner: JsonModemIterator<'a, 'a, Ctx>,
    builder: &'a mut A,
}

impl<Ctx, A> JsonModemBuffersIter<'_, Ctx, A>
where
    Ctx: BuilderCtx + EventCtx,
    A: BufferAssembler<Ctx>,
{
    fn next_event(&mut self) -> Option<Result<BorrowedBufferedEvent<'_, Ctx>, BufferError<Ctx>>> {
        match self.inner.next() {
            Some(Ok(event)) => match self.builder.on_event(event) {
                Ok(buffered) => Some(Ok(buffered)),
                Err(err) => Some(Err(BufferError::Assembler(err))),
            },
            Some(Err(err)) => Some(Err(BufferError::Parser(err))),
            None => None,
        }
    }

    pub fn root(&self) -> &Ctx::Value
    where
        A: RootedBufferAssembler<Ctx>,
        <Ctx as PathCtx>::Path: PathRoot,
    {
        self.builder.root()
    }
}

impl<Ctx, A> LendingIterator for JsonModemBuffersIter<'_, Ctx, A>
where
    Ctx: BuilderCtx + EventCtx,
    A: BufferAssembler<Ctx>,
{
    type Item<'ev>
        = Result<BorrowedBufferedEvent<'ev, Ctx>, BufferError<Ctx>>
    where
        Self: 'ev;

    fn next(&mut self) -> Option<Self::Item<'_>> {
        self.next_event()
    }
}

/// Iterator draining buffered events after `finish` is called on the modem.
pub struct JsonModemBuffersClosed<Ctx, A>
where
    Ctx: BuilderCtx + EventCtx,
    A: BufferAssembler<Ctx>,
{
    builder: A,
    inner: JsonModemClosed<'static, Ctx>,
}

impl<Ctx, A> JsonModemBuffersClosed<Ctx, A>
where
    Ctx: BuilderCtx + EventCtx,
    A: BufferAssembler<Ctx>,
{
    fn next_event(&mut self) -> Option<Result<BorrowedBufferedEvent<'_, Ctx>, BufferError<Ctx>>> {
        match self.inner.next() {
            Some(Ok(event)) => match self.builder.on_event(event) {
                Ok(buffered) => Some(Ok(buffered)),
                Err(err) => Some(Err(BufferError::Assembler(err))),
            },
            Some(Err(err)) => Some(Err(BufferError::Parser(err))),
            None => None,
        }
    }

    pub fn root(&self) -> &Ctx::Value
    where
        A: RootedBufferAssembler<Ctx>,
        <Ctx as PathCtx>::Path: PathRoot,
    {
        self.builder.root()
    }
}

impl<Ctx, A> LendingIterator for JsonModemBuffersClosed<Ctx, A>
where
    Ctx: BuilderCtx + EventCtx,
    A: BufferAssembler<Ctx>,
{
    type Item<'ev>
        = Result<BorrowedBufferedEvent<'ev, Ctx>, BufferError<Ctx>>
    where
        Self: 'ev;

    fn next(&mut self) -> Option<Self::Item<'_>> {
        self.next_event()
    }
}

impl<Ctx, A> JsonModemBuffersIter<'_, Ctx, A>
where
    Ctx: BuilderCtx + OwnedEventCtx,
    A: BufferAssembler<Ctx>,
    <Ctx as PathCtx>::Path: Clone,
    <Ctx as OwnedEventCtx>::OwnedNum: Into<Ctx::Num<'static>>,
    <Ctx as OwnedEventCtx>::OwnedStr: Into<Ctx::Str<'static>>,
    <Ctx as BuilderCtx>::Array: 'static,
    <Ctx as BuilderCtx>::Object: 'static,
{
    #[allow(clippy::wrong_self_convention)]
    pub fn to_iter(
        self,
    ) -> impl Iterator<Item = Result<BufferedEvent<'static, Ctx::Path, Ctx>, BufferError<Ctx>>>
    {
        let mut iter = self;

        core::iter::from_fn(move || match iter.next_event() {
            Some(Ok(event)) => Some(Ok(buffered_event_into_owned(event))),
            Some(Err(err)) => Some(Err(err)),
            None => None,
        })
    }
}

impl<Ctx, A> JsonModemBuffersClosed<Ctx, A>
where
    Ctx: BuilderCtx + OwnedEventCtx,
    A: BufferAssembler<Ctx>,
    <Ctx as PathCtx>::Path: Clone,
    <Ctx as OwnedEventCtx>::OwnedNum: Into<Ctx::Num<'static>>,
    <Ctx as OwnedEventCtx>::OwnedStr: Into<Ctx::Str<'static>>,
    <Ctx as BuilderCtx>::Array: 'static,
    <Ctx as BuilderCtx>::Object: 'static,
{
    #[allow(clippy::wrong_self_convention)]
    pub fn to_iter(
        self,
    ) -> impl Iterator<Item = Result<BufferedEvent<'static, Ctx::Path, Ctx>, BufferError<Ctx>>>
    {
        let mut iter = self;

        core::iter::from_fn(move || match iter.next_event() {
            Some(Ok(event)) => Some(Ok(buffered_event_into_owned(event))),
            Some(Err(err)) => Some(Err(err)),
            None => None,
        })
    }
}

fn convert_num_owned<Ctx>(value: Ctx::Num<'_>) -> Ctx::Num<'static>
where
    Ctx: OwnedEventCtx,
    <Ctx as OwnedEventCtx>::OwnedNum: Into<Ctx::Num<'static>>,
{
    <Ctx as OwnedEventCtx>::num_into_owned(value).into()
}

fn convert_str_owned<Ctx>(value: Ctx::Str<'_>) -> Ctx::Str<'static>
where
    Ctx: OwnedEventCtx,
    <Ctx as OwnedEventCtx>::OwnedStr: Into<Ctx::Str<'static>>,
{
    <Ctx as OwnedEventCtx>::str_into_owned(value).into()
}

fn buffered_event_into_owned<Ctx>(
    event: BorrowedBufferedEvent<'_, Ctx>,
) -> BufferedEvent<'static, Ctx::Path, Ctx>
where
    Ctx: OwnedEventCtx + BuilderCtx,
    <Ctx as PathCtx>::Path: Clone,
    <Ctx as OwnedEventCtx>::OwnedNum: Into<Ctx::Num<'static>>,
    <Ctx as OwnedEventCtx>::OwnedStr: Into<Ctx::Str<'static>>,
    <Ctx as BuilderCtx>::Array: 'static,
    <Ctx as BuilderCtx>::Object: 'static,
{
    match event {
        BufferedEvent::Null { path } => BufferedEvent::Null { path: path.clone() },
        BufferedEvent::Boolean { path, value } => BufferedEvent::Boolean {
            path: path.clone(),
            value,
        },
        BufferedEvent::Number { path, value } => BufferedEvent::Number {
            path: path.clone(),
            value: convert_num_owned::<Ctx>(value),
        },
        BufferedEvent::String {
            path,
            fragment,
            value,
            is_initial,
            is_final,
        } => BufferedEvent::String {
            path: path.clone(),
            fragment: convert_str_owned::<Ctx>(fragment),
            value: value.map(|owned| convert_str_owned::<Ctx>(owned)),
            is_initial,
            is_final,
        },
        BufferedEvent::ArrayBegin { path } => BufferedEvent::ArrayBegin { path: path.clone() },
        BufferedEvent::ArrayEnd { path, .. } => BufferedEvent::ArrayEnd {
            path: path.clone(),
            value: None,
        },
        BufferedEvent::ObjectBegin { path } => BufferedEvent::ObjectBegin { path: path.clone() },
        BufferedEvent::ObjectEnd { path, .. } => BufferedEvent::ObjectEnd {
            path: path.clone(),
            value: None,
        },
    }
}
