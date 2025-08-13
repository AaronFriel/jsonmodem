#![cfg(test)]

use alloc::vec::Vec;

use crate::{options::BufferOptions, value::Value, BufferedEvent, JsonModemBuffers, ParserOptions};

#[test]
fn string_assembler_emits_prefixes_on_all_flushes() {
    // {"a":["hello"]}
    let mut b = JsonModemBuffers::string(ParserOptions::default(), BufferOptions::default());
    let mut out: Vec<BufferedEvent> = Vec::new();
    out.extend(b.feed("{\"a\":[\"he"));
    out.extend(b.feed("llo\"]}"));

    // Expect two String events: first with value Some("he"), final=false; second with Some("hello"), final=true
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
fn non_scalar_end_events_attach_values() {
    let mut b = JsonModemBuffers::new(ParserOptions::default(), BufferOptions::default());
    let mut out: Vec<BufferedEvent> = Vec::new();
    out.extend(b.feed("{\"a\":[\"he"));
    out.extend(b.feed("llo\",{\"k\":\"v\"}],\"b\":1}"));

    // Expect the final root ObjectEnd to carry value Some(...)
    let root_end = out.into_iter().rev().find_map(|e| match e { BufferedEvent::ObjectEnd { path, value } if path.is_empty() => Some(value), _ => None });
    let root = root_end.expect("expected root value");
    assert!(matches!(root, Some(Value::Object(_))));
}

#[test]
fn non_scalar_emits_all_container_values() {
    let mut b = JsonModemBuffers::new(ParserOptions::default(), BufferOptions::default());
    let mut out: Vec<BufferedEvent> = Vec::new();
    out.extend(b.feed("{\"a\":[\"he"));
    out.extend(b.feed("llo\",{\"k\":\"v\"}],\"b\":1}"));

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
fn std_values_do_not_attach_string_values() {
    // moderation.decision: "allow" split across three chunks "al","lo","w"
    let mut b = JsonModemBuffers::new(ParserOptions::default(), BufferOptions::default());
    let mut out: Vec<BufferedEvent> = Vec::new();
    out.extend(b.feed("{\"moderation\":{\"decision\":\"al"));
    out.extend(b.feed("lo"));
    out.extend(b.feed("w\",\"reason\":null}}{}"));

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
    assert!(strings[2].1.is_none());
    assert!(strings[2].2);

    // Ensure nested ObjectEnd has value in All mode
    let moderation_end = out.iter().find_map(|e| match e { BufferedEvent::ObjectEnd { path, value } if path.as_slice() == crate::path!["moderation"].as_slice() => Some(value), _ => None });
    assert!(moderation_end.is_some());
    assert!(moderation_end.unwrap().is_some());
}
