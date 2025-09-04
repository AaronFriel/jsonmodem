use alloc::{collections::BTreeMap, string::{String, ToString}, sync::Arc, vec::Vec};
use quickcheck::{Arbitrary, Gen, QuickCheck};
use crate::parser::{ParserOptions, StreamingParserImpl, ParseEvent};
type DefaultStreamingParser = StreamingParserImpl<crate::backend::RustContext>;

#[derive(Clone, Debug, PartialEq)]
enum Value {
    Null,
    Boolean(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Object(BTreeMap<Arc<str>, Value>),
}

impl core::fmt::Display for Value {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Value::Null => f.write_str("null"),
            Value::Boolean(b) => f.write_str(if *b { "true" } else { "false" }),
            Value::Number(n) => f.write_str(&alloc::string::ToString::to_string(&n)),
            Value::String(s) => {
                fn write_escaped<W: core::fmt::Write>(src: &str, f: &mut W) -> core::fmt::Result {
                    for c in src.chars() {
                        match c {
                            '"' => f.write_str("\\\"")?,
                            '\\' => f.write_str("\\\\")?,
                            '\u{2028}' | '\u{2029}' => {
                                write!(f, "\\u{:04X}", c as u32)?;
                            }
                            c if c.is_ascii_control() || (c.is_control() && c as u32 <= 0xFFFF) => {
                                write!(f, "\\u{:04X}", c as u32)?;
                            }
                            _ => f.write_char(c)?,
                        }
                    }
                    Ok(())
                }
                f.write_str("\"")?;
                write_escaped(s, f)?;
                f.write_str("\"")
            }
            Value::Array(arr) => {
                f.write_str("[")?;
                let mut first = true;
                for v in arr {
                    if !first { f.write_str(",")?; }
                    first = false;
                    write!(f, "{v}")?;
                }
                f.write_str("]")
            }
            Value::Object(map) => {
                f.write_str("{")?;
                let mut first = true;
                for (k, v) in map {
                    if !first { f.write_str(",")?; }
                    first = false;
                    // escape key
                    fn escape_key(s: &str) -> String {
                        let mut out = String::new();
                        for c in s.chars() {
                            match c {
                                '"' => out.push_str("\\\""),
                                '\\' => out.push_str("\\\\"),
                                '\u{2028}' | '\u{2029}' => {
                                    use alloc::fmt::Write as _;
                                    let _ = write!(&mut out, "\\u{:04X}", c as u32);
                                }
                                c if c.is_ascii_control() || (c.is_control() && c as u32 <= 0xFFFF) => {
                                    use alloc::fmt::Write as _;
                                    let _ = write!(&mut out, "\\u{:04X}", c as u32);
                                }
                                _ => out.push(c),
                            }
                        }
                        out
                    }
                    write!(f, "\"{}\":{}", escape_key(k), v)?;
                }
                f.write_str("}")
            }
        }
    }
}

impl Arbitrary for Value {
    fn arbitrary(g: &mut Gen) -> Self {
        match usize::arbitrary(g) % 3 {
            0 => Value::Null,
            1 => Value::Boolean(bool::arbitrary(g)),
            _ => Value::String(String::arbitrary(g)),
        }
    }
}

fn insert_at_path(target: &mut Value, path: &[crate::PathItem], val: Value) {
    if path.is_empty() { *target = val; return; }
    let mut cur = target;
    for comp in &path[..path.len()-1] {
        match comp {
            crate::PathItem::Key(k) => {
                if let Value::Object(map) = cur { cur = map.entry(k.clone()).or_insert(Value::Null); }
                else { *cur = Value::Object(BTreeMap::new()); if let Value::Object(map) = cur { cur = map.entry(k.clone()).or_insert(Value::Null); } }
            }
            crate::PathItem::Index(i) => {
                let i = *i; if let Value::Array(vec) = cur { if i >= vec.len() { vec.resize(i+1, Value::Null); } cur = &mut vec[i]; }
                else { *cur = Value::Array(Vec::new()); if let Value::Array(vec) = cur { if i >= vec.len() { vec.resize(i+1, Value::Null); } cur = &mut vec[i]; } }
            }
        }
    }
    match path.last().unwrap() {
        crate::PathItem::Key(k) => { if let Value::Object(map) = cur { map.insert(k.clone(), val); } else { let mut m = BTreeMap::new(); m.insert(k.clone(), val); *cur = Value::Object(m); } }
        crate::PathItem::Index(i) => { let i = *i; if let Value::Array(vec) = cur { if i >= vec.len() { vec.resize(i+1, Value::Null); } vec[i] = val; } else { let mut v = Vec::new(); if i >= v.len() { v.resize(i+1, Value::Null); } v[i] = val; *cur = Value::Array(v); } }
    }
}

fn append_string_at_path(target: &mut Value, path: &[crate::PathItem], fragment: &str) {
    if path.is_empty() { if let Value::String(s) = target { s.push_str(fragment); } else { *target = Value::String(fragment.into()); } return; }
    let mut cur = target;
    for comp in &path[..path.len()-1] {
        match comp {
            crate::PathItem::Key(k) => {
                if let Value::Object(map) = cur { cur = map.entry(k.clone()).or_insert(Value::Null); }
                else { *cur = Value::Object(BTreeMap::new()); if let Value::Object(map) = cur { cur = map.entry(k.clone()).or_insert(Value::Null); } }
            }
            crate::PathItem::Index(i) => {
                let i = *i; if let Value::Array(vec) = cur { if i >= vec.len() { vec.resize(i+1, Value::Null); } cur = &mut vec[i]; }
                else { *cur = Value::Array(Vec::new()); if let Value::Array(vec) = cur { if i >= vec.len() { vec.resize(i+1, Value::Null); } cur = &mut vec[i]; } }
            }
        }
    }
    match path.last().unwrap() {
        crate::PathItem::Key(k) => { if let Value::Object(map) = cur { if let Some(Value::String(s)) = map.get_mut(k) { s.push_str(fragment); } else { map.insert(k.clone(), Value::String(fragment.into())); } } else { let mut m = BTreeMap::new(); m.insert(k.clone(), Value::String(fragment.into())); *cur = Value::Object(m); } }
        crate::PathItem::Index(i) => { let i = *i; if let Value::Array(vec) = cur { if i < vec.len() { if let Value::String(s) = &mut vec[i] { s.push_str(fragment); } else { vec[i] = Value::String(fragment.into()); } } else { vec.resize(i+1, Value::Null); vec[i] = Value::String(fragment.into()); } } else { let mut v = Vec::new(); if i >= v.len() { v.resize(i+1, Value::Null); } v[i] = Value::String(fragment.into()); *cur = Value::Array(v); } }
    }
}

struct Rebuilder {
    out: Vec<Value>, cur: Value, building: bool
}
impl Rebuilder {
    fn new() -> Self { Self { out: Vec::new(), cur: Value::Null, building: false } }
    fn apply(&mut self, evt: ParseEvent<'_, crate::backend::RustContext>) {
        match evt {
            ParseEvent::ArrayBegin { path } => { insert_at_path(&mut self.cur, &path, Value::Array(Vec::new())); if path.is_empty() { self.building = true; } }
            ParseEvent::ObjectBegin { path } => { insert_at_path(&mut self.cur, &path, Value::Object(BTreeMap::new())); if path.is_empty() { self.building = true; } }
            ParseEvent::Null { path } => { insert_at_path(&mut self.cur, &path, Value::Null); if path.is_empty() { self.out.push(Value::Null); self.cur = Value::Null; self.building = false; } }
            ParseEvent::Boolean { path, value } => { insert_at_path(&mut self.cur, &path, Value::Boolean(value)); if path.is_empty() { self.out.push(Value::Boolean(value)); self.cur = Value::Null; self.building = false; } }
            ParseEvent::Number { path, value } => { insert_at_path(&mut self.cur, &path, Value::Number(value)); if path.is_empty() { self.out.push(Value::Number(value)); self.cur = Value::Null; self.building = false; } }
            ParseEvent::String { path, fragment, is_final, .. } => { append_string_at_path(&mut self.cur, &path, &fragment); if is_final && path.is_empty() { self.out.push(self.cur.clone()); self.cur = Value::Null; self.building = false; } else if path.is_empty() { self.building = true; } }
            ParseEvent::ArrayEnd { path } | ParseEvent::ObjectEnd { path } => { if path.is_empty() && self.building { self.out.push(self.cur.clone()); self.cur = Value::Null; self.building = false; } }
        }
    }
    fn finish(mut self) -> Vec<Value> { if self.building { self.out.push(self.cur); } self.out }
}

/// Property: Feeding a JSON document in arbitrary chunk sizes must yield the
/// exact same `Value` when reconstructed from the emitted `ParseEvent`s.
#[test]
fn prop_partition_roundtrip() {
    #[expect(clippy::needless_pass_by_value)]
    fn prop(value: Value, splits: Vec<usize>) -> bool {
        let src = {
            let mut s = value.to_string();
            s.push(' '); // ensure delimiter for primitives
            s
        };
        if src.is_empty() {
            return true;
        }

        // Stream parser DefaultStreamingParsere so that structural container events are
        // emitted.
        let mut parser = DefaultStreamingParser::new(ParserOptions {
            allow_multiple_json_values: true,
            ..Default::default()
        });
        let mut reb = Rebuilder::new();
        // Feed the JSON text in arbitrarily sized UTF-8-safe chunks (derived from
        // `splits`).
        let chars: Vec<char> = src.chars().collect();
        let mut idx = 0;
        let mut remaining = chars.len();

        for s in splits {
            if remaining == 0 {
                break;
            }
            let size = 1 + (s % remaining);
            let end = idx + size;
            let chunk: String = chars[idx..end].iter().collect();
            for ev in parser.feed(&chunk) { if let Ok(e) = ev { reb.apply(e) } }
            idx = end;
            remaining -= size;
        }
        if remaining > 0 {
            let chunk: String = chars[idx..].iter().collect();
            for ev in parser.feed(&chunk) { if let Ok(e) = ev { reb.apply(e) } }
        }

        // Flush any pending events.
        for ev in parser.finish() { if let Ok(e) = ev { reb.apply(e) } }

        let reconstructed = reb.finish();
        reconstructed.len() == 1 && reconstructed[0] == value
    }

    let tests = if cfg!(any(miri, feature = "test-fast")) {
        10
    } else if is_ci::cached() {
        10_000
    } else {
        1_000
    };

    QuickCheck::new()
        .tests(tests)
        .quickcheck(prop as fn(Value, Vec<usize>) -> bool);
}
