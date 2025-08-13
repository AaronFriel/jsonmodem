use alloc::{borrow::Cow, vec::Vec};

use crate::{
    JsonModem, JsonModemBuffers, ParseEvent, ParserOptions, StdBackend, options::BufferOptions,
};

#[test]
fn jsonmodem_core_strings_are_fragments_only() {
    let mut p: JsonModem<StdBackend> = JsonModem::new(ParserOptions::default());
    // Even if caller requests Values, core overrides to None
    let events: Vec<_> = p.feed("\"ab").to_iter().map(Result::unwrap).collect();
    assert_eq!(events.len(), 1);
    let first = events.into_iter().next().unwrap();
    assert!(matches!(
        first,
        ParseEvent::String {
            is_final: false,
            ..
        }
    ));

    let events: Vec<_> = p.feed("c\"").to_iter().map(Result::unwrap).collect();
    let evs: Vec<_> = events.into_iter().collect();
    assert!(matches!(evs[0], ParseEvent::String { is_final: true, .. }));
    if let ParseEvent::String { fragment, .. } = &evs[0] {
        let fragment: &Cow<'_, str> = fragment;
        assert_eq!(fragment.as_ref(), "c");
    }
}

#[test]
fn jsonmodem_buffers_does_not_attach_string_values() {
    let mut b = JsonModemBuffers::new(ParserOptions::default(), BufferOptions::default());
    // two chunks: expect no value until final; test iterator, too
    let out: Vec<_> = b.feed("\"hel").to_iter().map(Result::unwrap).collect();
    assert_eq!(out.len(), 1);
    match &out[0] {
        crate::BufferedEvent::String {
            fragment,
            value,
            is_final,
            ..
        } => {
            assert_eq!(fragment.as_ref(), "hel");
            assert!(value.is_none());
            assert!(!is_final);
        }
        _ => panic!("expected string"),
    }

    let out: Vec<_> = b.feed("lo\"").to_iter().map(Result::unwrap).collect();
    assert_eq!(out.len(), 1);
    match &out[0] {
        crate::BufferedEvent::String {
            value, is_final, ..
        } => {
            assert!(value.is_none());
            assert!(*is_final);
        }
        _ => panic!("expected string"),
    }
}

#[test]
fn jsonmodem_string_assembler_prefixes() {
    let mut b = JsonModemBuffers::string(ParserOptions::default(), BufferOptions::default());
    let out: Vec<_> = b.feed("\"ab").to_iter().map(Result::unwrap).collect();
    assert_eq!(out.len(), 1);
    match &out[0] {
        crate::BufferedEvent::String {
            fragment,
            value,
            is_final,
            ..
        } => {
            assert_eq!(fragment.as_ref(), "ab");
            assert_eq!(value.as_deref(), Some("ab"));
            assert!(!is_final);
        }
        _ => panic!("expected string"),
    }
    let out: Vec<_> = b.feed("c\"").to_iter().map(Result::unwrap).collect();
    assert_eq!(out.len(), 1);
    match &out[0] {
        crate::BufferedEvent::String {
            value, is_final, ..
        } => {
            assert_eq!(value.as_deref(), Some("abc"));
            assert!(*is_final);
        }
        _ => panic!("expected string"),
    }
}

#[test]
fn buffers_iter_flushes_on_non_string_event() {
    use crate::options::BufferOptions;
    // {"a":"ab","b":1}
    let mut b = crate::JsonModemBuffers::string(ParserOptions::default(), BufferOptions::default());
    let mut out = Vec::new();
    out.extend(
        b.feed("{\"a\":\"ab\",\"b\":1}")
            .to_iter()
            .map(Result::unwrap),
    );
    // Expect at least one String event for a, then other events including b's
    // number
    assert!(out.iter().any(|e| matches!(
        e,
        crate::BufferedEvent::String {
            path,
            fragment,
            value,
            is_final: true,
            ..
        } if path == &crate::path!["a"]
            && fragment.as_ref() == "ab"
            && value.as_deref() == Some("ab")
    )));
}

#[test]
fn buffers_iter_flushes_at_end_for_prefixes() {
    use crate::options::BufferOptions;
    let mut b = crate::JsonModemBuffers::string(ParserOptions::default(), BufferOptions::default());
    // Incomplete string at end of chunk should not emit an event until completed.
    let out: Vec<_> = b.feed("\"he").to_iter().map(Result::unwrap).collect();
    assert_eq!(out.len(), 1);
    match &out[0] {
        crate::BufferedEvent::String {
            fragment,
            value,
            is_final,
            ..
        } => {
            assert_eq!(fragment.as_ref(), "he");
            assert_eq!(value.as_deref(), Some("he"));
            assert!(!is_final);
        }
        _ => panic!("expected string"),
    }
    let out: Vec<_> = b.feed("llo\"").to_iter().map(Result::unwrap).collect();
    assert_eq!(out.len(), 1);
    match &out[0] {
        crate::BufferedEvent::String {
            fragment,
            value,
            is_final,
            ..
        } => {
            assert_eq!(fragment.as_ref(), "llo");
            assert_eq!(value.as_deref(), Some("hello"));
            assert!(*is_final);
        }
        _ => panic!("expected string"),
    }
}

#[test]
fn string_assembler_values_mode() {
    let mut buffers = JsonModemBuffers::string(ParserOptions::default(), BufferOptions::default());

    let out: Vec<_> = buffers
        .feed("\"hel")
        .to_iter()
        .map(Result::unwrap)
        .collect();
    assert_eq!(out.len(), 1);
    match &out[0] {
        crate::BufferedEvent::String {
            fragment,
            value,
            is_final,
            ..
        } => {
            assert_eq!(fragment.as_ref(), "hel");
            assert_eq!(value.as_deref(), Some("hel"));
            assert!(!is_final);
        }
        _ => panic!("expected string fragment"),
    }

    let out: Vec<_> = buffers.feed("lo\"").to_iter().map(Result::unwrap).collect();
    assert_eq!(out.len(), 1);
    match &out[0] {
        crate::BufferedEvent::String {
            value, is_final, ..
        } => {
            assert_eq!(value.as_deref(), Some("hello"));
            assert!(*is_final);
        }
        _ => panic!("expected terminating string"),
    }
}

#[test]
fn string_assembler_container_events_do_not_buffer_values() {
    let mut buffers = JsonModemBuffers::string(ParserOptions::default(), BufferOptions::default());

    let events: Vec<_> = buffers
        .feed("{\"items\":[\"x\",\"y\"]}")
        .to_iter()
        .map(Result::unwrap)
        .collect();

    assert!(events.iter().any(|event| matches!(event,
        crate::BufferedEvent::ArrayEnd { path, value } if path == &crate::path!["items"] && value.is_none()
    )));
    assert!(events.iter().any(|event| matches!(event,
        crate::BufferedEvent::ObjectEnd { path, value } if path.is_empty() && value.is_none()
    )));
}
