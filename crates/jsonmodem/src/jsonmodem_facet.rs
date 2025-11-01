use crate::{
    ParseEvent,
    backend::{
        StdBackend,
        facet::{FacetApplyError, FacetAssembler, FacetEvent, FacetOptions, facet_api::Facet},
    },
    lending_iterator::LendingIterator,
    parser::{JsonModem, JsonModemClosed, JsonModemIterator, ParserError, ParserOptions},
};

/// Error returned by [`JsonModemFacet`] while processing a stream.
#[derive(Debug)]
pub enum FacetError {
    /// The underlying parser reported a syntax or decode error.
    Parser(ParserError<StdBackend>),
    /// Updating the target value failed.
    Assembler(FacetApplyError),
}

/// Streaming adapter that applies JSON events directly to a facet target.
pub struct JsonModemFacet<'root, Root: Facet> {
    parser: JsonModem<StdBackend>,
    assembler: FacetAssembler<'root, Root>,
}

impl<'root, Root> JsonModemFacet<'root, Root>
where
    Root: Facet,
{
    /// Creates a new facet adapter using the provided parser and facet options.
    #[must_use]
    pub fn new(root: &'root mut Root, options: ParserOptions, facet_options: FacetOptions) -> Self {
        Self {
            parser: JsonModem::new(options.with_allow_multiple_json_values(true)),
            assembler: FacetAssembler::new(root, facet_options),
        }
    }

    /// Feeds a chunk of text and returns an iterator over facet events.
    pub fn feed<'parser, 'src>(
        &'parser mut self,
        chunk: &'src str,
    ) -> JsonModemFacetIter<'parser, 'src, 'root, Root> {
        JsonModemFacetIter {
            parser: self.parser.feed(chunk),
            assembler: &mut self.assembler,
        }
    }

    /// Finalises the stream and drains any remaining events.
    #[must_use]
    pub fn finish(self) -> JsonModemFacetClosed<'root, Root> {
        JsonModemFacetClosed {
            parser: self.parser.finish(),
            assembler: self.assembler,
        }
    }

    /// Returns the options used by the assembler.
    #[must_use]
    pub fn options(&self) -> FacetOptions {
        self.assembler.options()
    }
}

/// Lending iterator over facet events produced for a chunk of input.
pub struct JsonModemFacetIter<'parser, 'src, 'root, Root>
where
    Root: Facet,
{
    parser: JsonModemIterator<'parser, 'src, StdBackend>,
    assembler: &'parser mut FacetAssembler<'root, Root>,
}

impl<Root> JsonModemFacetIter<'_, '_, '_, Root>
where
    Root: Facet,
{
    fn next_event(&mut self) -> Option<Result<FacetEvent<Root>, FacetError>> {
        while let Some(raw_event) = self.parser.next() {
            let event = match raw_event {
                Ok(ev) => ParseEvent::from(ev),
                Err(err) => return Some(Err(FacetError::Parser(err))),
            };

            match self
                .assembler
                .on_event(event)
                .map_err(FacetError::Assembler)
            {
                Ok(Some(evt)) => return Some(Ok(evt)),
                Ok(None) => {}
                Err(err) => return Some(Err(err)),
            }
        }
        None
    }
}

impl<Root> LendingIterator for JsonModemFacetIter<'_, '_, '_, Root>
where
    Root: Facet,
{
    type Item<'a>
        = Result<FacetEvent<Root>, FacetError>
    where
        Self: 'a;

    fn next(&mut self) -> Option<Self::Item<'_>> {
        self.next_event()
    }
}

/// Iterator draining remaining events after the stream has finished.
pub struct JsonModemFacetClosed<'root, Root>
where
    Root: Facet,
{
    parser: JsonModemClosed<'static, StdBackend>,
    assembler: FacetAssembler<'root, Root>,
}

impl<Root> JsonModemFacetClosed<'_, Root>
where
    Root: Facet,
{
    fn next_event(&mut self) -> Option<Result<FacetEvent<Root>, FacetError>> {
        while let Some(raw_event) = self.parser.next() {
            let event = match raw_event {
                Ok(ev) => ParseEvent::from(ev),
                Err(err) => return Some(Err(FacetError::Parser(err))),
            };

            match self
                .assembler
                .on_event(event)
                .map_err(FacetError::Assembler)
            {
                Ok(Some(evt)) => return Some(Ok(evt)),
                Ok(None) => {}
                Err(err) => return Some(Err(err)),
            }
        }
        None
    }
}

impl<Root> LendingIterator for JsonModemFacetClosed<'_, Root>
where
    Root: Facet,
{
    type Item<'a>
        = Result<FacetEvent<Root>, FacetError>
    where
        Self: 'a;

    fn next(&mut self) -> Option<Self::Item<'_>> {
        self.next_event()
    }
}
