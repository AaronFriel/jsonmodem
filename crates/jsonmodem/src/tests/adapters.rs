use alloc::vec::Vec;

use crate::{
    JsonModem, JsonModemBuffers, JsonModemValues, ParseEvent,
    ParserOptions, options::{BufferOptions, BufferStringMode}
};

#[test]
fn jsonmodem_core_strings_are_fragments_only() {
    let mut m = JsonModem::new(ParserOptions::default());
    // Even if caller requests Values, core overrides to None
    let events: Vec<_> = m.feed("\"ab").collect();
    assert!(events.is_empty());
    let events: Vec<_> = m.feed("c\"").collect();
    let evs: Vec<_> = events.into_iter().map(Result::unwrap).collect();
    assert!(matches!(evs[0], ParseEvent::String { is_final: true, .. }));
    if let ParseEvent::String { value, .. } = &evs[0] { assert!(value.is_none()); }
}

#[test]
fn jsonmodem_buffers_values_mode() {
    let mut b = JsonModemBuffers::new(ParserOptions::default(), BufferOptions { string_values: BufferStringMode::Values, non_scalar_values: crate::options::NonScalarValueMode::None });
    // two chunks: expect no value until final; test iterator, too
    let out: Vec<_> = b.feed("\"hel").collect();
    assert!(out.is_empty());
    let out: Vec<_> = b.feed("lo\"").collect();
    assert_eq!(out.len(), 1);
    match &out[0] {
        crate::BufferedEvent::String { value, is_final, .. } => {
            assert_eq!(value.as_deref(), Some("hello"));
            assert!(*is_final);
        }
        _ => panic!("expected string"),
    }
}

#[test]
fn jsonmodem_buffers_prefixes_mode() {
    let mut b = JsonModemBuffers::new(ParserOptions::default(), BufferOptions { string_values: BufferStringMode::Prefixes, non_scalar_values: crate::options::NonScalarValueMode::None });
    let out: Vec<_> = b.feed("\"ab").collect();
    assert!(out.is_empty());
    let out: Vec<_> = b.feed("c\"").collect();
    assert_eq!(out.len(), 1);
    match &out[0] {
        crate::BufferedEvent::String { value, is_final, .. } => {
            assert_eq!(value.as_deref(), Some("abc"));
            assert!(*is_final);
        }
        _ => panic!("expected string"),
    }
}

#[test]
fn jsonmodem_values_emits_complete_roots() {
    let mut v = JsonModemValues::new(ParserOptions::default());
    let out: Vec<_> = v.feed("1 2").map(Result::unwrap).collect();
    assert!(out.iter().all(|sv| sv.is_final));
    let vals: Vec<_> = out.into_iter().map(|sv| sv.value).collect();
    assert_eq!(vals.len(), 2);
}

#[test]
fn buffers_iter_flushes_on_non_string_event() {
    use crate::options::{BufferOptions, BufferStringMode};
    // {"a":"ab","b":1}
    let mut b = crate::JsonModemBuffers::new(
        ParserOptions::default(),
        BufferOptions { string_values: BufferStringMode::Values, non_scalar_values: crate::options::NonScalarValueMode::None },
    );
    let mut out: Vec<crate::BufferedEvent> = Vec::new();
    out.extend(b.feed("{\"a\":\"ab\",\"b\":1}").map(Result::unwrap));
    // Expect at least one String event for a, then other events including b's number
    assert!(out.iter().any(|e| matches!(e, crate::BufferedEvent::String { path, fragment, value, is_final } if path == &crate::path!["a"] && fragment.as_ref() == "ab" && value.as_deref() == Some("ab") && *is_final)));
}

#[test]
fn buffers_iter_flushes_at_end_for_prefixes() {
    use crate::options::{BufferOptions, BufferStringMode};
    let mut b = crate::JsonModemBuffers::new(
        ParserOptions::default(),
        BufferOptions { string_values: BufferStringMode::Prefixes, non_scalar_values: crate::options::NonScalarValueMode::None },
    );
    // Incomplete string at end of chunk
    let out: Vec<_> = b.feed("\"he").map(Result::unwrap).collect();
    // Should flush pending with is_final=false and value as prefix
    assert_eq!(out.len(), 1);
    match &out[0] {
        crate::BufferedEvent::String { fragment, value, is_final, .. } => {
            assert_eq!(fragment.as_ref(), "he");
            assert_eq!(value.as_deref(), Some("he"));
            assert!(!is_final);
        }
        _ => panic!("expected string"),
    }
}
