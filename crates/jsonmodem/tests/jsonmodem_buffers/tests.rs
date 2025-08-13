use core::fmt::Write;
use std::{collections::BTreeMap, sync::Arc};

use insta::assert_snapshot;
use jsonmodem::{
    BufferOptions, BufferedEvent, JsonModemBuffers, ParserOptions, PathItem, StdBackend, Value,
};
use quickcheck::{Arbitrary, Gen, QuickCheck};

fn chunk_input(input: &str, chunk_size: usize) -> Vec<String> {
    let chunk_size = chunk_size.max(1);
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < input.len() {
        let end = core::cmp::min(start + chunk_size, input.len());
        chunks.push(input[start..end].to_owned());
        start = end;
    }
    if chunks.is_empty() {
        chunks.push(String::new());
    }
    chunks
}

type StdBufferedEvent = BufferedEvent<'static, Vec<PathItem>, StdBackend>;

fn build_modem(options: BufferOptions) -> JsonModemBuffers {
    JsonModemBuffers::new(
        ParserOptions::default().with_allow_multiple_json_values(true),
        options,
    )
}

fn random_ascii_string(g: &mut Gen) -> String {
    let len = usize::arbitrary(g) % 4;
    (0..len)
        .map(|_| {
            let byte = (u8::arbitrary(g) % 26) + b'a';
            byte as char
        })
        .collect()
}

fn arbitrary_value(g: &mut Gen, depth: usize) -> Value {
    if depth >= 3 {
        return Value::Null;
    }

    match usize::arbitrary(g) % 5 {
        0 => Value::Null,
        1 => Value::Boolean(bool::arbitrary(g)),
        2 => {
            let mut number = f64::arbitrary(g);
            while !number.is_finite() {
                number = f64::arbitrary(g);
            }
            number = number.rem_euclid(100_000.0);
            Value::Number(number)
        }
        3 => Value::String(random_ascii_string(g)),
        _ => {
            let len = usize::arbitrary(g) % 3;
            let mut items = Vec::with_capacity(len);
            for _ in 0..len {
                items.push(arbitrary_value(g, depth + 1));
            }
            Value::Array(items)
        }
    }
}

#[derive(Clone, Debug)]
struct ArbValue(Value);

impl Arbitrary for ArbValue {
    fn arbitrary(g: &mut Gen) -> Self {
        Self(arbitrary_value(g, 0))
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        Box::new(core::iter::empty())
    }
}

fn render_string_assembler_events(chunks: &[&str], options: BufferOptions) -> String {
    let mut modem = JsonModemBuffers::string(ParserOptions::default(), options);
    let mut lines = String::new();

    for chunk in chunks {
        for event in modem.feed(chunk).to_iter() {
            match event {
                Ok(event) => {
                    writeln!(lines, "{event:?}").unwrap();
                }
                Err(err) => panic!("chunk {chunk:?} yielded error: {err:?}"),
            }
        }
    }

    for event in modem.finish().to_iter() {
        match event {
            Ok(event) => {
                writeln!(lines, "{event:?}").unwrap();
            }
            Err(err) => panic!("finish yielded error: {err:?}"),
        }
    }

    lines
}

#[test]
fn buffers_emit_string_value_only_when_complete() {
    let mut modem = build_modem(BufferOptions::default());

    let early_events: Vec<_> = modem
        .feed("\"fragment")
        .to_iter()
        .map(Result::unwrap)
        .collect();
    assert_eq!(early_events.len(), 1);
    match &early_events[0] {
        BufferedEvent::String {
            value,
            is_final,
            fragment,
            ..
        } => {
            assert_eq!(fragment.as_ref(), "fragment");
            assert!(
                value.is_none(),
                "std value assembler does not attach string values"
            );
            assert!(!is_final);
        }
        other => panic!("unexpected buffered event for initial fragment: {other:?}"),
    }

    let mut late_events = Vec::new();
    late_events.extend(modem.feed("ed\"").to_iter().map(Result::unwrap));

    assert_eq!(late_events.len(), 1);
    match &late_events[0] {
        BufferedEvent::String {
            path,
            fragment,
            value,
            is_final,
            ..
        } => {
            assert!(path.is_empty());
            assert_eq!(fragment.as_ref(), "ed");
            assert!(
                value.is_none(),
                "std value assembler does not attach string values"
            );
            assert!(*is_final);
        }
        other => panic!("unexpected buffered event: {other:?}"),
    }
}

#[test]
fn snapshot_string_assembler_stream() {
    let chunks = [
        "{\"outer\":{\"inner\":\"span\\n",
        "line\",\"array\":[\"x\",\"y\"],",
        "\"flag\":true},\"tail\":\"done\"}",
    ];
    let rendered = render_string_assembler_events(&chunks, BufferOptions::default());

    assert_snapshot!(rendered, @r#"
    ObjectBegin { path: [] }
    ObjectBegin { path: [Key("outer")] }
    String { path: [Key("outer"), Key("inner")], fragment: "span", value: Some("span"), is_initial: true, is_final: false }
    String { path: [Key("outer"), Key("inner")], fragment: "\nline", value: Some("span\nline"), is_initial: false, is_final: true }
    ArrayBegin { path: [Key("outer"), Key("array")] }
    String { path: [Key("outer"), Key("array"), Index(0)], fragment: "x", value: Some("x"), is_initial: true, is_final: true }
    String { path: [Key("outer"), Key("array"), Index(1)], fragment: "y", value: Some("y"), is_initial: true, is_final: true }
    ArrayEnd { path: [Key("outer"), Key("array")], value: None }
    Boolean { path: [Key("outer"), Key("flag")], value: true }
    ObjectEnd { path: [Key("outer")], value: None }
    String { path: [Key("tail")], fragment: "done", value: Some("done"), is_initial: true, is_final: true }
    ObjectEnd { path: [], value: None }
    "#);
}

fn ensure_array(value: &mut Value) -> &mut Vec<Value> {
    if let Value::Array(array) = value {
        array
    } else {
        *value = Value::Array(Vec::new());
        match value {
            Value::Array(array) => array,
            _ => unreachable!(),
        }
    }
}

fn ensure_object(value: &mut Value) -> &mut BTreeMap<Arc<str>, Value> {
    if let Value::Object(map) = value {
        map
    } else {
        *value = Value::Object(BTreeMap::new());
        match value {
            Value::Object(map) => map,
            _ => unreachable!(),
        }
    }
}

fn insert_value_at_path(target: &mut Value, path: &[PathItem], value: Value) {
    if path.is_empty() {
        *target = value;
        return;
    }

    let mut current = target;
    for component in &path[..path.len() - 1] {
        current = match component {
            PathItem::Key(key) => ensure_object(current)
                .entry(key.clone())
                .or_insert(Value::Null),
            PathItem::Index(index) => {
                let array = ensure_array(current);
                let idx = *index;
                if idx >= array.len() {
                    array.resize(idx + 1, Value::Null);
                }
                &mut array[idx]
            }
        };
    }

    match path.last().expect("path is non-empty") {
        PathItem::Key(key) => {
            ensure_object(current).insert(key.clone(), value);
        }
        PathItem::Index(index) => {
            let array = ensure_array(current);
            let idx = *index;
            if idx >= array.len() {
                array.resize(idx + 1, Value::Null);
            }
            array[idx] = value;
        }
    }
}

fn append_fragment(target: &mut Value, path: &[PathItem], fragment: &str) {
    if path.is_empty() {
        match target {
            Value::String(buffer) => buffer.push_str(fragment),
            _ => *target = Value::String(fragment.into()),
        }
        return;
    }

    let mut current = target;
    for component in &path[..path.len() - 1] {
        current = match component {
            PathItem::Key(key) => ensure_object(current)
                .entry(key.clone())
                .or_insert(Value::Null),
            PathItem::Index(index) => {
                let array = ensure_array(current);
                let idx = *index;
                if idx >= array.len() {
                    array.resize(idx + 1, Value::Null);
                }
                &mut array[idx]
            }
        };
    }

    match path.last().expect("path is non-empty") {
        PathItem::Key(key) => {
            let entry = ensure_object(current)
                .entry(key.clone())
                .or_insert_with(|| Value::String(String::new()));
            if let Value::String(buffer) = entry {
                buffer.push_str(fragment);
            } else {
                *entry = Value::String(fragment.into());
            }
        }
        PathItem::Index(index) => {
            let array = ensure_array(current);
            let idx = *index;
            if idx >= array.len() {
                array.resize(idx + 1, Value::Null);
            }
            match &mut array[idx] {
                Value::String(buffer) => buffer.push_str(fragment),
                slot => *slot = Value::String(fragment.into()),
            }
        }
    }
}

fn reconstruct_from_events(events: &[StdBufferedEvent]) -> jsonmodem::Value {
    let mut root = Value::Null;

    for event in events {
        match event {
            BufferedEvent::Null { path } => insert_value_at_path(&mut root, path, Value::Null),
            BufferedEvent::Boolean { path, value } => {
                insert_value_at_path(&mut root, path, Value::Boolean(*value));
            }
            BufferedEvent::Number { path, value } => {
                insert_value_at_path(&mut root, path, Value::Number(*value));
            }
            BufferedEvent::String {
                path,
                fragment,
                value,
                ..
            } => {
                append_fragment(&mut root, path, fragment.as_ref());
                if let Some(full) = value.as_ref() {
                    insert_value_at_path(&mut root, path, Value::String(full.as_ref().to_owned()));
                }
            }
            BufferedEvent::ArrayBegin { path } => {
                insert_value_at_path(&mut root, path, Value::Array(Vec::new()));
            }
            BufferedEvent::ObjectBegin { path } => {
                insert_value_at_path(&mut root, path, Value::Object(BTreeMap::new()));
            }
            BufferedEvent::ArrayEnd { path, value } => {
                if let Some(value) = value {
                    insert_value_at_path(&mut root, path, Value::Array((*value).clone()));
                }
            }
            BufferedEvent::ObjectEnd { path, value } => {
                if let Some(value) = value {
                    insert_value_at_path(&mut root, path, Value::Object((*value).clone()));
                }
            }
        }
    }

    root
}

fn buffer_value_roundtrip(ArbValue(value): ArbValue, chunk_size: u8) -> bool {
    let json = value.to_string();
    let wrapped = format!("{{\"root\":{json}}}");
    let chunks = chunk_input(&wrapped, 1);

    let mut modem = build_modem(BufferOptions::default());

    let mut events = Vec::new();
    for chunk in &chunks {
        for event in modem.feed(chunk).to_iter() {
            let Ok(event) = event else { return false };
            events.push(event);
        }
    }

    for event in modem.finish().to_iter() {
        let Ok(event) = event else { return false };
        events.push(event);
    }

    match reconstruct_from_events(&events) {
        Value::Object(map) => {
            if let Some(reconstructed) = map.values().next() {
                if reconstructed != &value {
                    eprintln!(
                        "roundtrip mismatch value={value:?} reconstructed={reconstructed:?} chunk_size={chunk_size} wrapped={wrapped}"
                    );
                }
                reconstructed == &value
            } else {
                eprintln!("object root without values when reconstructing value={value:?}");
                false
            }
        }
        other => {
            eprintln!("unexpected reconstructed root value={other:?} (expected object wrapper)");
            false
        }
    }
}

#[test]
fn buffers_handles_numeric_root() {
    let value = Value::Number(541_376.0);
    let wrapped = format!("{{\"root\":{value}}}");
    let chunks = chunk_input(&wrapped, 1);

    let mut modem = build_modem(BufferOptions::default());

    let mut events = Vec::new();
    for chunk in &chunks {
        for event in modem.feed(chunk).to_iter() {
            events.push(event.unwrap());
        }
    }

    for event in modem.finish().to_iter() {
        events.push(event.unwrap());
    }

    let reconstructed = match reconstruct_from_events(&events) {
        Value::Object(map) => map.values().next().cloned(),
        other => Some(other),
    };

    assert_eq!(reconstructed, Some(value));
}

#[test]
fn prop_buffers_value_roundtrip() {
    QuickCheck::new()
        .tests(64)
        .quickcheck(buffer_value_roundtrip as fn(ArbValue, u8) -> bool);
}
