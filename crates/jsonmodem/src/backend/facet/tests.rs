use alloc::{string::String, vec::Vec};
use core::any::Any;

use crate::{
    backend::{
        facet::{facet_api, FacetEventKind, FacetOptions},
        StdPath,
    },
    jsonmodem_facet::{FacetError, JsonModemFacet},
    lending_iterator::LendingIterator,
    ParserOptions,
};

#[derive(Default)]
struct Config {
    title: String,
    flags: Vec<bool>,
    retries: Option<u32>,
}

impl facet_api::ValueDyn for Config {
    fn ty_name(&self) -> &'static str {
        "Config"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn ensure_container(&mut self, kind: facet_api::ContainerKind) -> Result<(), &'static str> {
        match kind {
            facet_api::ContainerKind::Map | facet_api::ContainerKind::Struct => Ok(()),
            _ => Err("config expects object container"),
        }
    }

    fn key_erased(&mut self, name: &str) -> Result<facet_api::ErasedPtr, &'static str> {
        match name {
            "title" => Ok(facet_api::ErasedPtr::new(&mut self.title)),
            "flags" => Ok(facet_api::ErasedPtr::new(&mut self.flags)),
            "retries" => Ok(facet_api::ErasedPtr::new(&mut self.retries)),
            _ => Err("unknown field"),
        }
    }
}

#[test]
fn updates_struct_fields() {
    let mut cfg = Config::default();
    let mut facet = JsonModemFacet::new(
        &mut cfg,
        ParserOptions::default(),
        FacetOptions::default(),
    );

    let mut kinds = Vec::new();
    {
        let mut iter = facet.feed("{\"title\":\"Hello\",\"flags\":[true,false],\"retries\":3}");
        while let Some(event) = iter.next() {
            let event = event.expect("event should parse");
            kinds.push(event.kind);
        }
    }
    let mut closed = facet.finish();
    while let Some(event) = closed.next() {
        let event = event.expect("finish should parse");
        kinds.push(event.kind);
    }

    assert!(matches!(kinds.first(), Some(FacetEventKind::MapBegin)));
    assert_eq!(cfg.title, "Hello");
    assert_eq!(cfg.flags, vec![true, false]);
    assert_eq!(cfg.retries, Some(3));
}

#[test]
fn string_fragments_append_when_enabled() {
    let mut cfg = Config::default();
    let mut facet = JsonModemFacet::new(
        &mut cfg,
        ParserOptions::default(),
        FacetOptions::default(),
    );

    let mut kinds = Vec::new();
    {
        let mut iter = facet.feed("{\"title\":\"ab\\ncd\"}");
        while let Some(event) = iter.next() {
            let event = event.expect("event should parse");
            kinds.push(event.kind);
        }
    }
    let mut closed = facet.finish();
    while let Some(event) = closed.next() {
        let event = event.expect("finish should parse");
        kinds.push(event.kind);
    }

    let fragments: Vec<_> = kinds
        .into_iter()
        .filter_map(|kind| match kind {
            FacetEventKind::StringFragment { is_initial, is_final } => Some((is_initial, is_final)),
            _ => None,
        })
        .collect();

    assert_eq!(fragments, vec![(true, false), (false, true)]);
    assert_eq!(cfg.title, "ab\ncd");
}

#[test]
fn rejecting_fractional_integer_without_coercion() {
    let mut cfg = Config::default();
    let mut facet = JsonModemFacet::new(
        &mut cfg,
        ParserOptions::default(),
        FacetOptions {
            allow_coerce_numbers: false,
            ..FacetOptions::default()
        },
    );

    let mut iter = facet.feed("{\"retries\":3.5}");
    let mut saw_error = false;
    while let Some(event) = iter.next() {
        match event {
            Ok(_) => {}
            Err(FacetError::Assembler(err)) => {
                saw_error = true;
                assert_eq!(err.expected, super::NUMBER_EXPECTATION_UNSIGNED);
                break;
            }
            Err(other) => panic!("unexpected error: {other:?}"),
        }
    }
    assert!(saw_error, "expected numeric coercion error");
}
