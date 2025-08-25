use alloc::vec::Vec;

use crate::{
    BufferedEvent, JsonModemBuffers, ParserOptions,
    options::{BufferOptions, BufferStringMode, NonScalarValueMode},
    value::Value,
};

#[test]
fn values_mode_emits_buffered_strings_on_all_flushes() {
    // {"a":["hello"]}
    let mut b = JsonModemBuffers::new(
        ParserOptions::default(),
        BufferOptions { string_values: BufferStringMode::Values, non_scalar_values: NonScalarValueMode::None },
    );
    let mut out: Vec<BufferedEvent> = Vec::new();
    out.extend(b.feed("{\"a\":[\"he").map(Result::unwrap));
    out.extend(b.feed("llo\"]}").map(Result::unwrap));

    // We expect two String events: first with value Some("he"), final=false; second with value Some("hello"), final=true
    let strings: Vec<_> = out
        .into_iter()
        .filter_map(|e| match e { BufferedEvent::String { path, fragment, value, is_final } => Some((path, fragment, value, is_final)), _ => None })
        .collect();
    assert_eq!(strings.len(), 2, "expected two string events");
    assert_eq!(strings[0].1.as_ref(), "he");
    assert_eq!(strings[0].2.as_deref(), Some("he"));
    assert!(!strings[0].3);
    assert_eq!(strings[1].1.as_ref(), "llo");
    assert_eq!(strings[1].2.as_deref(), Some("hello"));
    assert!(strings[1].3);
}

#[test]
fn non_scalar_roots_emits_root_value_only() {
    let mut b = JsonModemBuffers::new(
        ParserOptions::default(),
        BufferOptions { string_values: BufferStringMode::None, non_scalar_values: NonScalarValueMode::Roots },
    );
    let mut out: Vec<BufferedEvent> = Vec::new();
    out.extend(b.feed("{\"a\":[\"he").map(Result::unwrap));
    out.extend(b.feed("llo\",{\"k\":\"v\"}],\"b\":1}").map(Result::unwrap));

    // Expect the final root ObjectEnd to carry value Some(...)
    let root_end = out.into_iter().rev().find_map(|e| match e { BufferedEvent::ObjectEnd { path, value } if path.is_empty() => Some(value), _ => None });
    let root = root_end.expect("expected root value");
    assert!(matches!(root, Some(Value::Object(_))));
}

#[test]
fn non_scalar_all_emits_all_container_values() {
    let mut b = JsonModemBuffers::new(
        ParserOptions::default(),
        BufferOptions { string_values: BufferStringMode::None, non_scalar_values: NonScalarValueMode::All },
    );
    let mut out: Vec<BufferedEvent> = Vec::new();
    out.extend(b.feed("{\"a\":[\"he").map(Result::unwrap));
    out.extend(b.feed("llo\",{\"k\":\"v\"}],\"b\":1}").map(Result::unwrap));

    // Collect ArrayEnd/ObjectEnd values
    let mut array_end = None;
    let mut nested_obj_end = None;
    let mut root_end = None;
    for e in out {
        match e {
            BufferedEvent::ArrayEnd { path, value } if path.as_slice() == crate::path!["a"].as_slice() => array_end = Some(value),
            BufferedEvent::ObjectEnd { path, value } if path.as_slice() == crate::path!["a", 1].as_slice() => nested_obj_end = Some(value),
            BufferedEvent::ObjectEnd { path, value } if path.is_empty() => root_end = Some(value),
            _ => {}
        }
    }
    assert!(matches!(array_end.flatten(), Some(Value::Array(_))), "expected array value at ArrayEnd");
    assert!(matches!(nested_obj_end.flatten(), Some(Value::Object(_))), "expected object value at nested ObjectEnd");
    assert!(matches!(root_end.flatten(), Some(Value::Object(_))), "expected object value at root ObjectEnd");
}



#[test]
fn prefixes_mode_emits_value_only_on_final() {
    // moderation.decision: "allow" split across three chunks "al","lo","w"
    let mut b = JsonModemBuffers::new(
        ParserOptions::default(),
        BufferOptions { string_values: BufferStringMode::Prefixes, non_scalar_values: NonScalarValueMode::All },
    );
    let mut out: Vec<BufferedEvent> = Vec::new();
    out.extend(b.feed("{"moderation":{"decision":"al").map(Result::unwrap));
    out.extend(b.feed("lo").map(Result::unwrap));
    out.extend(b.feed("w","reason":null}}{}").map(Result::unwrap));

    let strings: Vec<_> = out
        .iter()
        .filter_map(|e| match e { BufferedEvent::String { path, fragment, value, is_final } if path.as_slice() == crate::path!["moderation","decision"].as_slice() => Some((fragment.clone(), value.clone(), *is_final)), _ => None })
        .collect();
    assert_eq!(strings.len(), 3);
    assert_eq!(strings[0].0.as_ref(), "al");
    assert!(strings[0].1.is_none());
    assert!(!strings[0].2);
    assert_eq!(strings[1].0.as_ref(), "lo");
    assert!(strings[1].1.is_none());
    assert!(!strings[1].2);
    assert_eq!(strings[2].0.as_ref(), "w");
    assert_eq!(strings[2].1.as_deref(), Some("allow"));
    assert!(strings[2].2);

    // Ensure nested ObjectEnd has value in All mode
    let moderation_end = out.iter().find_map(|e| match e { BufferedEvent::ObjectEnd { path, value } if path.as_slice() == crate::path!["moderation"].as_slice() => Some(value), _ => None });
    assert!(moderation_end.is_some());
    assert!(moderation_end.unwrap().is_some());
}
