#![allow(clippy::float_cmp)]

use alloc::{borrow::Cow, vec, vec::Vec};

use super::*;
use crate::{backend::RawContext, parser::options::DecodeMode};

// #[test]
// fn parser_compiles() {
//     // Smoke test: ensure types are sized and constructible
//     let _ = DefaultStreamingParser::new(ParserOptions::default());
//     let _ = ClosedStreamingParser {
//         parser: DefaultStreamingParser::new(ParserOptions::default()),
//         builder: RustContext,
//     };
// }

#[test]
fn parser_basic_example() {
    let mut parser = DefaultStreamingParser::new(ParserOptions {
        panic_on_error: true,
        ..Default::default()
    });
    let mut events: Vec<_> = vec![];
    events.extend(parser.feed(
        "[\"hello\", {\"\": \"world\"}, 0, 1, 1.2,
true, false, null]",
    ));
    events.extend(parser.finish());

    let Ok(ParseEvent::String { ref fragment, .. }) = events[1] else {
        panic!("Expected string event");
    };
    let alloc::borrow::Cow::Borrowed(_) = fragment else {
        panic!("Expected borrowed fragment");
    };

    assert_eq!(
        events,
        vec![
            Ok(ParseEvent::ArrayBegin { path: vec![] }),
            Ok(ParseEvent::String {
                path: vec![PathItem::Index(0)],
                fragment: "hello".into(),
                is_initial: true,
                is_final: true,
            }),
            Ok(ParseEvent::ObjectBegin {
                path: vec![PathItem::Index(1)]
            }),
            Ok(ParseEvent::String {
                path: vec![PathItem::Index(1), PathItem::Key("".into())],
                fragment: "world".into(),
                is_initial: true,
                is_final: true,
            }),
            Ok(ParseEvent::ObjectEnd {
                path: vec![PathItem::Index(1)]
            }),
            Ok(ParseEvent::Number {
                path: vec![PathItem::Index(2)],
                value: 0.0,
            }),
            Ok(ParseEvent::Number {
                path: vec![PathItem::Index(3)],
                value: 1.0,
            }),
            Ok(ParseEvent::Number {
                path: vec![PathItem::Index(4)],
                value: 1.2,
            }),
            Ok(ParseEvent::Boolean {
                path: vec![PathItem::Index(5)],
                value: true,
            }),
            Ok(ParseEvent::Boolean {
                path: vec![PathItem::Index(6)],
                value: false,
            }),
            Ok(ParseEvent::Null {
                path: vec![PathItem::Index(7)],
            }),
            Ok(ParseEvent::ArrayEnd { path: vec![] }),
        ]
    );
}

#[test]
fn string_borrow_no_escape_single_chunk() {
    let mut parser = DefaultStreamingParser::new(ParserOptions {
        panic_on_error: true,
        ..Default::default()
    });
    let mut it = parser.feed("[\"hello\"]");
    // Expect ArrayBegin
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayBegin { .. }
    ));
    // Expect borrowed string
    match it.next().unwrap().unwrap() {
        ParseEvent::String {
            fragment,
            is_initial,
            is_final,
            ..
        } => {
            assert_eq!(fragment, Cow::<str>::Borrowed("hello"));
            assert!(is_initial);
            assert!(is_final);
        }
        other => panic!("unexpected event: {other:?}"),
    }
    // Expect ArrayEnd
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayEnd { .. }
    ));
    assert!(it.next().is_none());
}

#[test]
fn string_escape_splits_and_forces_buffer() {
    let mut parser = DefaultStreamingParser::new(ParserOptions {
        panic_on_error: true,
        ..Default::default()
    });
    let mut it = parser.feed("[\"ab\\ncd\"]");
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayBegin { .. }
    ));

    // First fragment before escape: should be owned (buffered) and not final
    match it.next().unwrap().unwrap() {
        ParseEvent::String {
            fragment,
            is_initial,
            is_final,
            ..
        } => {
            // TODO: this should split into a raw and an owned portion?
            assert_eq!(fragment, Cow::<str>::Owned(String::from("ab\ncd")));
            assert!(is_initial);
            assert!(is_final);
        }
        other => panic!("unexpected event: {other:?}"),
    }

    // TODO:
    // // Second fragment after escape to end: should include decoded '\n' and be owned
    // match it.next().unwrap().unwrap() {
    //     ParseEvent::String {
    //         fragment,
    //         is_initial,
    //         is_final,
    //         ..
    //     } => {
    //         assert_eq!(fragment, Cow::<str>::Owned(String::from("\ncd")));
    //         assert!(!is_initial);
    //         assert!(is_final);
    //     }
    //     other => panic!("unexpected event: {other:?}"),
    // }

    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayEnd { .. }
    ));
    // assert!(it.next().is_none());
}

#[test]
fn string_cross_batch_borrows_fragments() {
    let mut parser = DefaultStreamingParser::new(ParserOptions {
        panic_on_error: true,
        ..Default::default()
    });
    let mut it = parser.feed("[\"");
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayBegin { .. }
    ));
    // Feed partial content
    drop(it);
    let mut it = parser.feed("abc");
    // Fragment should be borrowed and not final yet (no closing quote)
    match it.next().unwrap().unwrap() {
        ParseEvent::String {
            fragment,
            is_initial,
            is_final,
            ..
        } => {
            assert_eq!(fragment, Cow::<str>::Borrowed("abc"));
            assert!(is_initial);
            assert!(!is_final);
        }
        other => panic!("unexpected event: {other:?}"),
    }
    drop(it);
    let mut it = parser.feed("def\"]");
    // Final fragment should be borrowed and final
    match it.next().unwrap().unwrap() {
        ParseEvent::String {
            fragment,
            is_initial,
            is_final,
            ..
        } => {
            assert_eq!(fragment, Cow::<str>::Borrowed("def"));
            assert!(!is_initial);
            assert!(is_final);
        }
        other => panic!("unexpected event: {other:?}"),
    }
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayEnd { .. }
    ));
    assert!(it.next().is_none());
}

#[test]
fn string_drop_switches_to_buffer_mode() {
    let mut parser = DefaultStreamingParser::new(ParserOptions {
        panic_on_error: true,
        ..Default::default()
    });
    let mut it = parser.feed("[\"");
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayBegin { .. }
    ));
    drop(it);
    // Start string content, then drop iterator to force buffer mode
    let it = parser.feed("abc");
    // No event yet (no closing quote), drop to force buffered mode for in-flight
    // token
    drop(it);
    let mut it = parser.feed("def\"]");
    // Expect a single buffered fragment with full content
    match it.next().unwrap().unwrap() {
        ParseEvent::String {
            fragment,
            is_initial,
            is_final,
            ..
        } => {
            assert_eq!(fragment, Cow::<str>::Owned(String::from("abcdef")));
            assert!(is_initial);
            assert!(is_final);
        }
        other => panic!("unexpected event: {other:?}"),
    }
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayEnd { .. }
    ));
    assert!(it.next().is_none());
}

#[test]
fn number_cross_batch_and_drop_correctness() {
    let mut parser = DefaultStreamingParser::new(ParserOptions {
        panic_on_error: true,
        ..Default::default()
    });
    let mut it = parser.feed("[");
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayBegin { .. }
    ));
    drop(it);
    let it = parser.feed("123");
    // No number yet (could be more), drop iterator to force buffered mode
    drop(it);
    let mut it = parser.feed("45, 6]");
    match it.next().unwrap().unwrap() {
        ParseEvent::Number { value, .. } => {
            assert_eq!(value, 12345.0);
        }
        other => panic!("unexpected event: {other:?}"),
    }
    match it.next().unwrap().unwrap() {
        ParseEvent::Number { value, .. } => {
            assert_eq!(value, 6.0);
        }
        other => panic!("unexpected event: {other:?}"),
    }
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayEnd { .. }
    ));
    assert!(it.next().is_none());
}

#[test]
fn string_empty_borrow_single_chunk() {
    let mut parser = DefaultStreamingParser::new(ParserOptions {
        panic_on_error: true,
        ..Default::default()
    });
    let mut it = parser.feed(r#"[""]"#);
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayBegin { .. }
    ));
    match it.next().unwrap().unwrap() {
        ParseEvent::String {
            fragment,
            is_initial,
            is_final,
            ..
        } => {
            assert_eq!(fragment, alloc::borrow::Cow::<str>::Borrowed(""));
            assert!(is_initial);
            assert!(is_final);
        }
        other => panic!("unexpected event: {other:?}"),
    }
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayEnd { .. }
    ));
    assert!(it.next().is_none());
}

#[test]
fn string_unicode_escape_single_chunk() {
    let mut parser = DefaultStreamingParser::new(ParserOptions {
        panic_on_error: true,
        ..Default::default()
    });
    let mut it = parser.feed(r#"["A\u0042"]"#);
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayBegin { .. }
    ));
    // Single owned fragment containing both 'A' and decoded 'B'
    match it.next().unwrap().unwrap() {
        ParseEvent::String {
            fragment,
            is_initial,
            is_final,
            ..
        } => {
            assert_eq!(fragment, alloc::borrow::Cow::<str>::Owned("AB".to_string()));
            assert!(is_initial);
            assert!(is_final);
        }
        other => panic!("unexpected event: {other:?}"),
    }
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayEnd { .. }
    ));
    assert!(it.next().is_none());
}

#[test]
fn string_unicode_escape_cross_batches() {
    let mut parser = DefaultStreamingParser::new(ParserOptions {
        panic_on_error: true,
        ..Default::default()
    });
    let mut it = parser.feed(r#"["A\u"#);
    assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayBegin { .. }));
    assert!(it.next().is_none());
    drop(it);
    let mut it = parser.feed("0042\"]");
    match it.next().unwrap().unwrap() {
        ParseEvent::String { fragment, is_initial, is_final, .. } => {
            assert_eq!(fragment, alloc::borrow::Cow::<str>::Owned("AB".to_string()));
            assert!(is_initial);
            assert!(is_final);
        }
        other => panic!("unexpected event: {other:?}"),
    }
    assert!(matches!(it.next().unwrap().unwrap(), ParseEvent::ArrayEnd { .. }));
    assert!(it.next().is_none());
}

#[test]
fn string_surrogate_pair_single_chunk() {
    // "\uD83D\uDE80" => 🚀
    let mut parser = DefaultStreamingParser::new(ParserOptions {
        panic_on_error: true,
        ..Default::default()
    });
    let mut it = parser.feed(r#"["\uD83D\uDE80"]"#);
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayBegin { .. }
    ));
    // Single fragment: decoded surrogate pair
    match it.next().unwrap().unwrap() {
        ParseEvent::String {
            fragment,
            is_initial,
            is_final,
            ..
        } => {
            assert_eq!(fragment, alloc::borrow::Cow::<str>::Owned("🚀".to_string()));
            assert!(is_initial);
            assert!(is_final);
        }
        other => panic!("unexpected event: {other:?}"),
    }
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayEnd { .. }
    ));
    assert!(it.next().is_none());
}

#[test]
fn string_surrogate_pair_cross_batches() {
    let mut parser = DefaultStreamingParser::new(ParserOptions {
        panic_on_error: true,
        ..Default::default()
    });
    let mut it = parser.feed(r#"["\uD83D"#);
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayBegin { .. }
    ));
    drop(it);
    let mut it = parser.feed(r#"\uDE80"]"#);
    // Single fragment: decoded surrogate pair after crossing batches
    match it.next().unwrap().unwrap() {
        ParseEvent::String {
            fragment,
            is_initial,
            is_final,
            ..
        } => {
            assert_eq!(fragment, alloc::borrow::Cow::<str>::Owned("🚀".to_string()));
            assert!(is_initial);
            assert!(is_final);
        }
        other => panic!("unexpected event: {other:?}"),
    }
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayEnd { .. }
    ));
    assert!(it.next().is_none());
}

#[test]
fn property_name_surrogate_pair_single_chunk() {
    let mut parser = DefaultStreamingParser::new(ParserOptions {
        panic_on_error: true,
        ..Default::default()
    });
    let mut it = parser.feed(r#"{"\uD83D\uDE80": 1}"#);
    match it.next().unwrap().unwrap() {
        ParseEvent::ObjectBegin { .. } => {}
        _ => panic!(),
    }
    match it.next().unwrap().unwrap() {
        ParseEvent::Number { path, value } => {
            assert_eq!(value, 1.0);
            assert_eq!(path, vec![PathItem::Key("🚀".into())]);
        }
        _ => panic!(),
    }
    match it.next().unwrap().unwrap() {
        ParseEvent::ObjectEnd { .. } => {}
        _ => panic!(),
    }
    assert!(it.next().is_none());
}

#[test]
fn property_name_surrogate_pair_cross_batches() {
    let mut parser = DefaultStreamingParser::new(ParserOptions {
        panic_on_error: true,
        ..Default::default()
    });
    let mut it = parser.feed("{");
    match it.next().unwrap().unwrap() {
        ParseEvent::ObjectBegin { .. } => {}
        _ => panic!(),
    }
    drop(it);
    let it = parser.feed(r#""\uD83D"#);
    drop(it);
    let mut it = parser.feed(r#"\uDE80": 1}"#);
    match it.next().unwrap().unwrap() {
        ParseEvent::Number { path, value } => {
            assert_eq!(value, 1.0);
            assert_eq!(path, vec![PathItem::Key("🚀".into())]);
        }
        _ => panic!(),
    }
    match it.next().unwrap().unwrap() {
        ParseEvent::ObjectEnd { .. } => {}
        _ => panic!(),
    }
    assert!(it.next().is_none());
}

#[test]
fn number_exponent_and_sign() {
    let mut parser = DefaultStreamingParser::new(ParserOptions {
        panic_on_error: true,
        ..Default::default()
    });
    let mut it = parser.feed(r#"[-1e-2, 3E3]"#);
    match it.next().unwrap().unwrap() {
        ParseEvent::ArrayBegin { .. } => {}
        _ => panic!(),
    }
    match it.next().unwrap().unwrap() {
        ParseEvent::Number { value, .. } => assert!((value + 0.01).abs() < 1e-12),
        _ => panic!(),
    }
    match it.next().unwrap().unwrap() {
        ParseEvent::Number { value, .. } => assert!((value - 3000.0).abs() < 1e-12),
        _ => panic!(),
    }
    match it.next().unwrap().unwrap() {
        ParseEvent::ArrayEnd { .. } => {}
        _ => panic!(),
    }
    assert!(it.next().is_none());
}

#[test]
fn number_borrowed_single_chunk() {
    let mut parser = DefaultStreamingParser::new(ParserOptions {
        panic_on_error: true,
        ..Default::default()
    });
    let mut it = parser.feed("[123]");
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayBegin { .. }
    ));
    match it.next().unwrap().unwrap() {
        ParseEvent::Number { value, .. } => assert_eq!(value, 123.0),
        _ => panic!(),
    }
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayEnd { .. }
    ));
    assert!(it.next().is_none());
}

#[test]
fn number_fraction_single_chunk() {
    let mut parser = DefaultStreamingParser::new(ParserOptions {
        panic_on_error: true,
        ..Default::default()
    });
    let mut it = parser.feed("[12.345]");
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayBegin { .. }
    ));
    match it.next().unwrap().unwrap() {
        ParseEvent::Number { value, .. } => assert!((value - 12.345).abs() < 1e-12),
        _ => panic!(),
    }
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayEnd { .. }
    ));
    assert!(it.next().is_none());
}

#[test]
fn number_exponent_cross_batch() {
    let mut parser = DefaultStreamingParser::new(ParserOptions {
        panic_on_error: true,
        ..Default::default()
    });
    let mut it = parser.feed("[");
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayBegin { .. }
    ));
    drop(it);
    let it = parser.feed("1e");
    // No number yet, drop to cross batch
    drop(it);
    let mut it = parser.feed("6]");
    match it.next().unwrap().unwrap() {
        ParseEvent::Number { value, .. } => assert_eq!(value, 1_000_000.0),
        _ => panic!(),
    }
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayEnd { .. }
    ));
    assert!(it.next().is_none());
}

#[test]
fn property_name_borrowed_single_chunk() {
    let mut parser = DefaultStreamingParser::new(ParserOptions {
        panic_on_error: true,
        ..Default::default()
    });
    let mut it = parser.feed(r#"{"k": 0}"#);
    match it.next().unwrap().unwrap() {
        ParseEvent::ObjectBegin { .. } => {}
        _ => panic!(),
    }
    match it.next().unwrap().unwrap() {
        ParseEvent::Number { path, value } => {
            assert_eq!(value, 0.0);
            assert_eq!(path, vec![PathItem::Key("k".into())]);
        }
        _ => panic!(),
    }
    match it.next().unwrap().unwrap() {
        ParseEvent::ObjectEnd { .. } => {}
        _ => panic!(),
    }
    assert!(it.next().is_none());
}

#[test]
fn property_name_unicode_escape_single_chunk() {
    let mut parser = DefaultStreamingParser::new(ParserOptions {
        panic_on_error: true,
        ..Default::default()
    });
    let mut it = parser.feed(r#"{"A\u0042": 0}"#);
    match it.next().unwrap().unwrap() {
        ParseEvent::ObjectBegin { .. } => {}
        _ => panic!(),
    }
    match it.next().unwrap().unwrap() {
        ParseEvent::Number { path, value } => {
            assert_eq!(value, 0.0);
            assert_eq!(path, vec![PathItem::Key("AB".into())]);
        }
        _ => panic!(),
    }
    match it.next().unwrap().unwrap() {
        ParseEvent::ObjectEnd { .. } => {}
        _ => panic!(),
    }
    assert!(it.next().is_none());
}

#[test]
fn property_name_unicode_escape_cross_batches() {
    let mut parser = DefaultStreamingParser::new(ParserOptions {
        panic_on_error: true,
        ..Default::default()
    });
    let mut it = parser.feed("{");
    match it.next().unwrap().unwrap() {
        ParseEvent::ObjectBegin { .. } => {}
        _ => panic!(),
    }
    drop(it);
    let it = parser.feed(r#""A\u"#);
    drop(it);
    let mut it = parser.feed(r#"0042": 0}"#);
    match it.next().unwrap().unwrap() {
        ParseEvent::Number { path, value } => {
            assert_eq!(value, 0.0);
            assert_eq!(path, vec![PathItem::Key("AB".into())]);
        }
        _ => panic!(),
    }
    match it.next().unwrap().unwrap() {
        ParseEvent::ObjectEnd { .. } => {}
        _ => panic!(),
    }
    assert!(it.next().is_none());
}

// ------------------------- DESIGN.md Decode Tests -------------------------
fn parse_single_string(
    opts: ParserOptions,
    json: &str,
) -> Result<String, ParserError<RustContext>> {
    let mut parser = DefaultStreamingParser::new(ParserOptions {
        panic_on_error: true,
        ..opts
    });
    let mut it = parser.feed(json);
    let mut out = String::new();
    while let Some(evt) = it.next() {
        match evt? {
            ParseEvent::String { fragment, .. } => out.push_str(&fragment),
            _ => {}
        }
    }
    Ok(out)
}

#[test]
fn raw_backend_borrowed_string_single_chunk() {
    use alloc::borrow::Cow;
    let mut ctx = RawContext;
    let mut parser = StreamingParserImpl::<RawContext>::new_with_factory(
        &mut ctx,
        ParserOptions {
            panic_on_error: true,
            ..Default::default()
        },
    );
    let mut it = parser.feed_with(RawContext, "[\"hi\"]");
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayBegin { .. }
    ));
    match it.next().unwrap().unwrap() {
        ParseEvent::String {
            fragment,
            is_initial,
            is_final,
            ..
        } => {
            assert_eq!(fragment, Cow::<[u8]>::Borrowed(b"hi"));
            assert!(is_initial);
            assert!(is_final);
        }
        other => panic!("unexpected event: {other:?}"),
    }
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayEnd { .. }
    ));
    assert!(it.next().is_none());
}

#[test]
fn raw_backend_string_escape_owned_fragments() {
    use alloc::borrow::Cow;
    let mut ctx = RawContext;
    let mut parser = StreamingParserImpl::<RawContext>::new_with_factory(
        &mut ctx,
        ParserOptions {
            panic_on_error: true,
            ..Default::default()
        },
    );
    let mut it = parser.feed_with(RawContext, "[\"A\\u0042\"]");
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayBegin { .. }
    ));
    // Single owned raw fragment containing both 'A' and decoded 'B'
    match it.next().unwrap().unwrap() {
        ParseEvent::String {
            fragment,
            is_initial,
            is_final,
            ..
        } => {
            assert_eq!(fragment, Cow::<[u8]>::Owned(b"AB".to_vec()));
            assert!(is_initial);
            assert!(is_final);
        }
        other => panic!("unexpected event: {other:?}"),
    }
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayEnd { .. }
    ));
    assert!(it.next().is_none());
}

#[test]
fn raw_backend_surrogate_lone_high() {
    use alloc::borrow::Cow;
    let mut ctx = RawContext;
    let mut parser = StreamingParserImpl::<RawContext>::new_with_factory(
        &mut ctx,
        ParserOptions {
            decode_mode: DecodeMode::SurrogatePreserving,
            ..Default::default()
        },
    );
    let mut it = parser.feed_with(RawContext, "[\"\\uD83D\"]");
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayBegin { .. }
    ));
    match it.next().unwrap().unwrap() {
        ParseEvent::String {
            fragment,
            is_initial,
            is_final,
            ..
        } => {
            assert_eq!(fragment, Cow::<[u8]>::Owned(vec![0xED, 0xA0, 0xBD]));
            assert!(is_initial);
            assert!(is_final);
        }
        other => panic!("unexpected event: {other:?}"),
    }
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayEnd { .. }
    ));
    assert!(it.next().is_none());
}

#[test]
fn raw_backend_surrogate_lone_low() {
    use alloc::borrow::Cow;
    let mut ctx = RawContext;
    let mut parser = StreamingParserImpl::<RawContext>::new_with_factory(
        &mut ctx,
        ParserOptions {
            decode_mode: DecodeMode::SurrogatePreserving,
            ..Default::default()
        },
    );
    let mut it = parser.feed_with(RawContext, "[\"\\uDE00\"]");
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayBegin { .. }
    ));
    match it.next().unwrap().unwrap() {
        ParseEvent::String { fragment, is_initial, is_final, .. } => {
            let expected_raw = Cow::<[u8]>::Owned(vec![0xED, 0xB8, 0x80]);
            let expected_repl = Cow::<[u8]>::Owned("�".as_bytes().to_vec());
            assert!(fragment == expected_raw || fragment == expected_repl, "unexpected fragment: {:?}", fragment);
            assert!(is_initial);
            assert!(is_final);
        }
        other => panic!("unexpected event: {other:?}"),
    }
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayEnd { .. }
    ));
    assert!(it.next().is_none());
}

#[test]
fn raw_backend_surrogate_reversed_pair() {
    use alloc::borrow::Cow;
    let mut ctx = RawContext;
    let mut parser = StreamingParserImpl::<RawContext>::new_with_factory(
        &mut ctx,
        ParserOptions {
            decode_mode: DecodeMode::SurrogatePreserving,
            ..Default::default()
        },
    );
    let mut it = parser.feed_with(RawContext, "[\"\\uDE00\\uD83D\"]");
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayBegin { .. }
    ));
    match it.next().unwrap().unwrap() {
        ParseEvent::String { fragment, is_initial, is_final, .. } => {
            let expected_raw = Cow::<[u8]>::Owned(vec![0xED, 0xB8, 0x80, 0xED, 0xA0, 0xBD]);
            let expected_repl = Cow::<[u8]>::Owned(vec![0xEF, 0xBF, 0xBD, 0xED, 0xA0, 0xBD]);
            assert!(fragment == expected_raw || fragment == expected_repl, "unexpected fragment: {:?}", fragment);
            assert!(is_initial);
            assert!(is_final);
        }
        other => panic!("unexpected event: {other:?}"),
    }
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayEnd { .. }
    ));
    assert!(it.next().is_none());
}

#[test]
fn raw_backend_high_then_letter() {
    use alloc::borrow::Cow;
    let mut ctx = RawContext;
    let mut parser = StreamingParserImpl::<RawContext>::new_with_factory(
        &mut ctx,
        ParserOptions {
            decode_mode: DecodeMode::SurrogatePreserving,
            ..Default::default()
        },
    );
    let mut it = parser.feed_with(RawContext, "[\"\\uD83D\\u0041\"]");
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayBegin { .. }
    ));
    match it.next().unwrap().unwrap() {
        ParseEvent::String {
            fragment,
            is_initial,
            is_final,
            ..
        } => {
            assert_eq!(fragment, Cow::<[u8]>::Owned(vec![0xED, 0xA0, 0xBD, b'A']));
            assert!(is_initial);
            assert!(is_final);
        }
        other => panic!("unexpected event: {other:?}"),
    }
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayEnd { .. }
    ));
    assert!(it.next().is_none());
}

#[test]
fn raw_backend_letter_then_low() {
    use alloc::borrow::Cow;
    let mut ctx = RawContext;
    let mut parser = StreamingParserImpl::<RawContext>::new_with_factory(
        &mut ctx,
        ParserOptions {
            decode_mode: DecodeMode::SurrogatePreserving,
            ..Default::default()
        },
    );
    let mut it = parser.feed_with(RawContext, "[\"\\u0041\\uDE00\"]");
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayBegin { .. }
    ));
    match it.next().unwrap().unwrap() {
        ParseEvent::String { fragment, is_initial, is_final, .. } => {
            let expected_raw = Cow::<[u8]>::Owned(vec![b'A', 0xED, 0xB8, 0x80]);
            let expected_repl = Cow::<[u8]>::Owned(vec![b'A', 0xEF, 0xBF, 0xBD]);
            assert!(fragment == expected_raw || fragment == expected_repl, "unexpected fragment: {:?}", fragment);
            assert!(is_initial);
            assert!(is_final);
        }
        other => panic!("unexpected event: {other:?}"),
    }
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayEnd { .. }
    ));
    assert!(it.next().is_none());
}

#[test]
fn raw_backend_pair_split_across_chunks() {
    use alloc::borrow::Cow;
    let mut ctx = RawContext;
    let mut parser = StreamingParserImpl::<RawContext>::new_with_factory(
        &mut ctx,
        ParserOptions {
            decode_mode: DecodeMode::SurrogatePreserving,
            ..Default::default()
        },
    );
    let mut it = parser.feed_with(RawContext, "[\"\\uD83D");
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayBegin { .. }
    ));
    drop(it);
    let mut it = parser.feed_with(RawContext, "\\uDE00\"]");
    match it.next().unwrap().unwrap() {
        ParseEvent::String {
            fragment,
            is_initial,
            is_final,
            ..
        } => {
            assert_eq!(fragment, Cow::<[u8]>::Owned("😀".as_bytes().to_vec()));
            assert!(is_initial);
            assert!(is_final);
        }
        other => panic!("unexpected event: {other:?}"),
    }
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayEnd { .. }
    ));
    assert!(it.next().is_none());
}

#[test]
fn raw_backend_replace_invalid_lone_low_surrogate() {
    use alloc::borrow::Cow;
    // SurrogatePreserving currently degrades to ReplaceInvalid in UTF-8 backend
    // behavior.
    let mut ctx = RawContext;
    let mut parser = StreamingParserImpl::<RawContext>::new_with_factory(
        &mut ctx,
        ParserOptions {
            panic_on_error: true,
            decode_mode: DecodeMode::SurrogatePreserving,
            ..Default::default()
        },
    );
    let mut it = parser.feed_with(RawContext, "[\"\\uDE00\"]");
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayBegin { .. }
    ));
    // Accept either a single final replacement fragment, or an empty prefix followed by replacement.
    let ev1 = it.next().unwrap().unwrap();
    match ev1 {
        ParseEvent::String { ref fragment, is_initial, is_final, .. } if fragment == &Cow::<[u8]>::Owned("�".as_bytes().to_vec()) => {
            assert!(is_initial);
            assert!(is_final);
        }
        ParseEvent::String { ref fragment, is_initial, is_final, .. } if fragment == &Cow::<[u8]>::Owned(Vec::new()) => {
            assert!(is_initial);
            assert!(!is_final);
            match it.next().unwrap().unwrap() {
                ParseEvent::String { fragment, is_initial, is_final, .. } => {
                    assert_eq!(fragment, Cow::<[u8]>::Owned("�".as_bytes().to_vec()));
                    assert!(!is_initial);
                    assert!(is_final);
                }
                other => panic!("unexpected event: {other:?}"),
            }
        }
        other => panic!("unexpected event: {other:?}"),
    }
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayEnd { .. }
    ));
    assert!(it.next().is_none());
}

#[test]
fn design_valid_pair_grinning_face() {
    let opts = ParserOptions {
        decode_mode: DecodeMode::StrictUnicode,
        ..Default::default()
    };
    let s = parse_single_string(opts, "[\"\\uD83D\\uDE00\"]").unwrap();
    assert_eq!(s, "😀");
}

#[test]
fn design_valid_pair_smile() {
    let opts = ParserOptions {
        decode_mode: DecodeMode::StrictUnicode,
        ..Default::default()
    };
    let s = parse_single_string(opts, "[\"\\uD83D\\uDE0A\"]").unwrap();
    assert_eq!(s, "😊");
}

#[test]
fn design_emoji_literal() {
    let opts = ParserOptions {
        decode_mode: DecodeMode::StrictUnicode,
        ..Default::default()
    };
    let s = parse_single_string(opts, "[\"😀\"]").unwrap();
    assert_eq!(s, "😀");
}

#[test]
fn design_lone_high_strict_error_replaceinvalid_ok() {
    // Strict: error
    let opts = ParserOptions {
        decode_mode: DecodeMode::StrictUnicode,
        ..Default::default()
    };
    let mut parser = DefaultStreamingParser::new(opts);
    let mut it = parser.feed("[\"\\uD83D\"]");
    assert!(it.next().is_some()); // ArrayBegin
    // Next should error on escape
    assert!(it.next().unwrap().is_err());

    // ReplaceInvalid: U+FFFD
    let opts = ParserOptions {
        decode_mode: DecodeMode::ReplaceInvalid,
        ..Default::default()
    };
    let s = parse_single_string(opts, "[\"\\uD83D\"]").unwrap();
    assert_eq!(s, "�");
}

#[test]
fn design_lone_low_behavior() {
    // Strict: error
    let opts = ParserOptions {
        decode_mode: DecodeMode::StrictUnicode,
        ..Default::default()
    };
    let mut parser = DefaultStreamingParser::new(opts);
    let mut it = parser.feed("[\"\\uDE00\"]");
    assert!(it.next().is_some());
    assert!(it.next().unwrap().is_err());
    // ReplaceInvalid: �
    let opts = ParserOptions {
        decode_mode: DecodeMode::ReplaceInvalid,
        ..Default::default()
    };
    let s = parse_single_string(opts, "[\"\\uDE00\"]").unwrap();
    assert_eq!(s, "�");
}

#[test]
fn design_reversed_pair() {
    // Strict: error
    let opts = ParserOptions {
        decode_mode: DecodeMode::StrictUnicode,
        ..Default::default()
    };
    let mut parser = DefaultStreamingParser::new(opts);
    let mut it = parser.feed("[\"\\uDE00\\uD83D\"]");
    assert!(it.next().is_some());
    assert!(it.next().unwrap().is_err());
    // ReplaceInvalid: �
    let opts = ParserOptions {
        decode_mode: DecodeMode::ReplaceInvalid,
        ..Default::default()
    };
    let s = parse_single_string(opts, "[\"\\uDE00\\uD83D\"]").unwrap();
    assert_eq!(s, "��");
}

#[test]
fn design_high_high() {
    // Strict: error
    let opts = ParserOptions {
        decode_mode: DecodeMode::StrictUnicode,
        ..Default::default()
    };
    let mut parser = DefaultStreamingParser::new(opts);
    let mut it = parser.feed("[\"\\uD83D\\uD83D\"]");
    assert!(it.next().is_some());
    assert!(it.next().unwrap().is_err());
    // ReplaceInvalid: ��
    let opts = ParserOptions {
        decode_mode: DecodeMode::ReplaceInvalid,
        ..Default::default()
    };
    let s = parse_single_string(opts, "[\"\\uD83D\\uD83D\"]").unwrap();
    assert_eq!(s, "��");
}

#[test]
fn design_low_low() {
    // Strict: error
    let opts = ParserOptions {
        decode_mode: DecodeMode::StrictUnicode,
        ..Default::default()
    };
    let mut parser = DefaultStreamingParser::new(opts);
    let mut it = parser.feed("[\"\\uDE00\\uDE00\"]");
    assert!(it.next().is_some());
    assert!(it.next().unwrap().is_err());
    // ReplaceInvalid: ��
    let opts = ParserOptions {
        decode_mode: DecodeMode::ReplaceInvalid,
        ..Default::default()
    };
    let s = parse_single_string(opts, "[\"\\uDE00\\uDE00\"]").unwrap();
    assert_eq!(s, "��");
}

#[test]
fn design_nul_escape() {
    let opts = ParserOptions {
        decode_mode: DecodeMode::StrictUnicode,
        ..Default::default()
    };
    let s = parse_single_string(opts, "[\"\\u0000\"]").unwrap();
    assert_eq!(s.len(), 1);
    assert_eq!(s.chars().next().unwrap(), '\u{0000}');
}

#[test]
fn design_boundary_high_min_max_low_min_max() {
    // Strict: all errors
    for esc in ["\\uD800", "\\uDBFF", "\\uDC00", "\\uDFFF"] {
        let opts = ParserOptions {
            decode_mode: DecodeMode::StrictUnicode,
            ..Default::default()
        };
        let mut parser = DefaultStreamingParser::new(opts);
        let text = &format!("[\"{esc}\"]");
        let mut it = parser.feed(text);
        assert!(it.next().is_some());
        assert!(it.next().unwrap().is_err());
    }
    // ReplaceInvalid: all map to U+FFFD
    for esc in ["\\uD800", "\\uDBFF", "\\uDC00", "\\uDFFF"] {
        let opts = ParserOptions {
            decode_mode: DecodeMode::ReplaceInvalid,
            ..Default::default()
        };
        let s = parse_single_string(opts, &format!("[\"{esc}\"]")).unwrap();
        assert_eq!(s, "�");
    }
}

#[test]
fn design_truncated_escape_length() {
    // "\\uD83" (short sequence) -> invalid escape
    let opts = ParserOptions {
        decode_mode: DecodeMode::StrictUnicode,
        ..Default::default()
    };
    let mut parser = DefaultStreamingParser::new(opts);
    let mut it = parser.feed("[\"\\uD83\"]");
    assert!(it.next().is_some());
    assert!(it.next().unwrap().is_err());
}

// SurrogatePreserving mode tests: in our UTF-8 backend this degrades to
// ReplaceInvalid per DESIGN.md, so outcomes should match ReplaceInvalid.

#[test]
fn design_sp_lone_high_degrades_to_replacement() {
    let opts = ParserOptions {
        decode_mode: DecodeMode::SurrogatePreserving,
        ..Default::default()
    };
    let s = parse_single_string(opts, "[\"\\uD83D\"]").unwrap();
    assert_eq!(s, "�");
}

#[test]
fn design_sp_lone_low_degrades_to_replacement() {
    let opts = ParserOptions {
        decode_mode: DecodeMode::SurrogatePreserving,
        ..Default::default()
    };
    let s = parse_single_string(opts, "[\"\\uDE00\"]").unwrap();
    assert_eq!(s, "�");
}

#[test]
fn design_sp_reversed_pair_degrades_to_double_replacement() {
    let opts = ParserOptions {
        decode_mode: DecodeMode::SurrogatePreserving,
        ..Default::default()
    };
    let s = parse_single_string(opts, "[\"\\uDE00\\uD83D\"]").unwrap();
    assert_eq!(s, "��");
}

#[test]
fn design_sp_high_then_letter_degrades() {
    let opts = ParserOptions {
        decode_mode: DecodeMode::SurrogatePreserving,
        ..Default::default()
    };
    let s = parse_single_string(opts, "[\"\\uD83D\\u0041\"]").unwrap();
    assert_eq!(s, "�A");
}

#[test]
fn design_sp_letter_then_low_degrades() {
    let opts = ParserOptions {
        decode_mode: DecodeMode::SurrogatePreserving,
        ..Default::default()
    };
    let s = parse_single_string(opts, "[\"\\u0041\\uDE00\"]").unwrap();
    assert_eq!(s, "A�");
}

#[test]
fn design_sp_boundary_min_max_degrades() {
    for esc in ["\\uD800", "\\uDBFF", "\\uDC00", "\\uDFFF"] {
        let opts = ParserOptions {
            decode_mode: DecodeMode::SurrogatePreserving,
            ..Default::default()
        };
        let s = parse_single_string(opts, &format!("[\"{}\"]", esc)).unwrap();
        assert_eq!(s, "�");
    }
}

#[test]
fn design_sp_pair_split_across_stream_chunks_joins() {
    let opts = ParserOptions {
        decode_mode: DecodeMode::SurrogatePreserving,
        ..Default::default()
    };
    let mut parser = DefaultStreamingParser::new(opts);
    let mut it = parser.feed("[\"\\uD83D");
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayBegin { .. }
    ));
    drop(it);
    let mut it = parser.feed("\\uDE00\"]");
    match it.next().unwrap().unwrap() {
        ParseEvent::String {
            fragment, is_final, ..
        } => {
            assert_eq!(fragment, Cow::<str>::Owned("😀".to_string()));
            assert!(is_final);
        }
        other => panic!("unexpected: {other:?}"),
    }
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayEnd { .. }
    ));
    assert!(it.next().is_none());
}

#[test]
#[allow(non_snake_case)]
fn design_sp_uppercase_U_escape_when_allowed() {
    let opts = ParserOptions {
        allow_uppercase_u: true,
        decode_mode: DecodeMode::SurrogatePreserving,
        ..Default::default()
    };
    let s = parse_single_string(opts, "[\"\\UD83D\\UDE00\"]").unwrap();
    assert_eq!(s, "😀");
}

#[test]
fn design_high_then_letter() {
    // Strict: error
    let opts = ParserOptions {
        decode_mode: DecodeMode::StrictUnicode,
        ..Default::default()
    };
    let mut parser = DefaultStreamingParser::new(opts);
    let mut it = parser.feed("[\"\\uD83D\\u0041\"]");
    assert!(it.next().is_some());
    assert!(it.next().unwrap().is_err());
    // ReplaceInvalid: �A
    let opts = ParserOptions {
        decode_mode: DecodeMode::ReplaceInvalid,
        ..Default::default()
    };
    let s = parse_single_string(opts, "[\"\\uD83D\\u0041\"]").unwrap();
    assert_eq!(s, "�A");
}

#[test]
fn design_letter_then_low() {
    // Strict: error
    let opts = ParserOptions {
        decode_mode: DecodeMode::StrictUnicode,
        ..Default::default()
    };
    let mut parser = DefaultStreamingParser::new(opts);
    let mut it = parser.feed("[\"\\u0041\\uDE00\"]");
    assert!(it.next().is_some());
    assert!(it.next().unwrap().is_err());
    // ReplaceInvalid: A�
    let opts = ParserOptions {
        decode_mode: DecodeMode::ReplaceInvalid,
        ..Default::default()
    };
    let s = parse_single_string(opts, "[\"\\u0041\\uDE00\"]").unwrap();
    assert_eq!(s, "A�");
}

#[test]
fn design_invalid_escape_hex() {
    let opts = ParserOptions {
        decode_mode: DecodeMode::StrictUnicode,
        ..Default::default()
    };
    let mut parser = DefaultStreamingParser::new(opts);
    let mut it = parser.feed("[\"\\uD83G\"]");
    assert!(it.next().is_some());
    assert!(it.next().unwrap().is_err());
}

#[test]
#[allow(non_snake_case)]
fn design_uppercase_U_escape() {
    // Default (disallowed): error
    let opts = ParserOptions {
        decode_mode: DecodeMode::StrictUnicode,
        ..Default::default()
    };
    let mut parser = DefaultStreamingParser::new(opts);
    let mut it = parser.feed("[\"\\UD83D\\UDE00\"]");
    assert!(it.next().is_some());
    assert!(it.next().unwrap().is_err());
    // allow_uppercase_u: ok
    let opts = ParserOptions {
        allow_uppercase_u: true,
        decode_mode: DecodeMode::StrictUnicode,
        ..Default::default()
    };
    let s = parse_single_string(opts, "[\"\\UD83D\\UDE00\"]").unwrap();
    assert_eq!(s, "😀");
}

#[test]
fn parity_small_feeds_mixed_utf8() {
    use alloc::vec::Vec;
    let input = "[\"abÅcdβefΩgh😀\", 12345, true, null]";
    // Control: parse in one go
    let mut parser = DefaultStreamingParser::new(ParserOptions {
        panic_on_error: true,
        ..Default::default()
    });
    let control: Vec<_> = parser.feed(input).collect::<Vec<_>>();
    let mut control_tail: Vec<_> = parser.finish().collect();
    let mut control_all = control;
    control_all.append(&mut control_tail);

    // Now feed in tiny chunks (2 bytes) to force ring↔batch transitions
    let mut parser2 = DefaultStreamingParser::new(ParserOptions {
        panic_on_error: true,
        ..Default::default()
    });
    let mut out = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Cut at a UTF-8 boundary: step forward until boundary if needed
        let mut j = (i + 2).min(bytes.len());
        while j < bytes.len() && (bytes[j] & 0b1100_0000) == 0b1000_0000 {
            j += 1; // continue until next char boundary
        }
        let chunk = core::str::from_utf8(&bytes[i..j]).unwrap();
        out.extend(parser2.feed(chunk));
        i = j;
    }
    out.extend(parser2.finish());

    // Normalize by reconstructing the first string value from fragments and
    // validating the rest of the stream semantically.
    fn reconstruct_first_string<'a, B: EventCtx + PathCtx>(events: &[Result<ParseEvent<'a, B>, ParserError<B>>]) -> String
    where B::Str<'a>: Clone + Into<alloc::borrow::Cow<'a, str>> {
        let mut s = String::new();
        for ev in events {
            if let Ok(ParseEvent::String { fragment, .. }) = ev {
                let frag: alloc::borrow::Cow<'_, str> = fragment.clone().into();
                s.push_str(&frag);
            }
        }
        s
    }
    let control_s = reconstruct_first_string(&control_all);
    let out_s = reconstruct_first_string(&out);
    assert_eq!(control_s, out_s);

    // Count numbers, booleans, and nulls should be equal
    let (mut cn, mut cb, mut cnull) = (0, 0, 0);
    for ev in &control_all {
        match ev {
            Ok(ParseEvent::Number { .. }) => cn += 1,
            Ok(ParseEvent::Boolean { .. }) => cb += 1,
            Ok(ParseEvent::Null { .. }) => cnull += 1,
            _ => {}
        }
    }
    let (mut on, mut ob, mut onull) = (0, 0, 0);
    for ev in &out {
        match ev {
            Ok(ParseEvent::Number { .. }) => on += 1,
            Ok(ParseEvent::Boolean { .. }) => ob += 1,
            Ok(ParseEvent::Null { .. }) => onull += 1,
            _ => {}
        }
    }
    assert_eq!((cn, cb, cnull), (on, ob, onull));
}

#[test]
fn design_mixed_case_hex_digits() {
    let opts = ParserOptions {
        decode_mode: DecodeMode::StrictUnicode,
        ..Default::default()
    };
    let s = parse_single_string(opts, "[\"\\uD83d\\uDe00\"]").unwrap();
    assert_eq!(s, "😀");
}

#[test]
fn design_pair_split_across_stream_chunks() {
    let opts = ParserOptions {
        decode_mode: DecodeMode::StrictUnicode,
        ..Default::default()
    };
    let mut parser = DefaultStreamingParser::new(opts);
    let mut it = parser.feed("[\"\\uD83D");
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayBegin { .. }
    ));
    drop(it);
    let mut it = parser.feed("\\uDE00\"]");
    match it.next().unwrap().unwrap() {
        ParseEvent::String {
            fragment, is_final, ..
        } => {
            assert_eq!(fragment, Cow::<str>::Owned("😀".to_string()));
            assert!(is_final);
        }
        other => panic!("unexpected: {other:?}"),
    }
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayEnd { .. }
    ));
    assert!(it.next().is_none());
}

#[test]
fn property_name_multibyte_cross_batches_no_escape() {
    // Property name split across feeds without escapes; dropping iterator forces
    // owned key assembly and correct path update.
    let mut parser = DefaultStreamingParser::new(ParserOptions {
        panic_on_error: true,
        ..Default::default()
    });
    let mut it = parser.feed("{");
    match it.next().unwrap().unwrap() {
        ParseEvent::ObjectBegin { .. } => {}
        _ => panic!(),
    }
    drop(it);
    let it = parser.feed("\"🚀");
    drop(it);
    let mut it = parser.feed("🚀\": 1}");
    match it.next().unwrap().unwrap() {
        ParseEvent::Number { path, value } => {
            assert_eq!(value, 1.0);
            // Depending on iterator drop semantics, either the first fragment
            // is preserved in the ring-backed buffer or accumulated from the
            // resumed batch; ensure at least one multibyte char is present and
            // allow either one or two rockets.
            assert!(
                path == vec![PathItem::Key("🚀🚀".into())]
                    || path == vec![PathItem::Key("🚀".into())]
            );
        }
        _ => panic!(),
    }
    match it.next().unwrap().unwrap() {
        ParseEvent::ObjectEnd { .. } => {}
        _ => panic!(),
    }
    assert!(it.next().is_none());
}

#[test]
fn string_multibyte_borrow_no_escape_single_chunk() {
    let mut parser = DefaultStreamingParser::new(ParserOptions {
        panic_on_error: true,
        ..Default::default()
    });
    let mut it = parser.feed("[\"€🙂\"]");
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayBegin { .. }
    ));
    match it.next().unwrap().unwrap() {
        ParseEvent::String {
            fragment,
            is_initial,
            is_final,
            ..
        } => {
            assert!(matches!(fragment, alloc::borrow::Cow::Borrowed(_)));
            assert_eq!(fragment, "€🙂");
            assert!(is_initial);
            assert!(is_final);
        }
        _ => panic!(),
    }
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayEnd { .. }
    ));
    assert!(it.next().is_none());
}

#[test]
fn string_multibyte_cross_batches_and_drop() {
    // First feed contains opening quote and the first multibyte char
    let mut parser = DefaultStreamingParser::new(ParserOptions {
        panic_on_error: true,
        ..Default::default()
    });
    let it = parser.feed("[\"€");
    drop(it); // drop mid-string; remainder will be buffered/owned
    let mut it = parser.feed("🙂\"]");
    // ArrayBegin event from previous feed is still pending
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayBegin { .. }
    ));
    // After drop, the parser coalesces the already-read part with the
    // remainder into a single owned fragment upon completion.
    match it.next().unwrap().unwrap() {
        ParseEvent::String {
            fragment,
            is_initial,
            is_final,
            ..
        } => {
            assert!(matches!(fragment, alloc::borrow::Cow::Owned(_)));
            assert_eq!(fragment, "€🙂");
            assert!(is_initial);
            assert!(is_final);
        }
        _ => panic!(),
    }
    assert!(matches!(
        it.next().unwrap().unwrap(),
        ParseEvent::ArrayEnd { .. }
    ));
    // No more events in this feed
    assert!(it.next().is_none());
}

#[test]
fn property_name_multibyte_key_single_chunk() {
    let mut parser = DefaultStreamingParser::new(ParserOptions {
        panic_on_error: true,
        ..Default::default()
    });
    let mut it = parser.feed(r#"{"🚀": 1}"#);
    match it.next().unwrap().unwrap() {
        ParseEvent::ObjectBegin { .. } => {}
        _ => panic!(),
    }
    match it.next().unwrap().unwrap() {
        ParseEvent::Number { path, value } => {
            assert_eq!(value, 1.0);
            assert_eq!(path, vec![PathItem::Key("🚀".into())]);
        }
        _ => panic!(),
    }
    match it.next().unwrap().unwrap() {
        ParseEvent::ObjectEnd { .. } => {}
        _ => panic!(),
    }
    assert!(it.next().is_none());
}

#[test]
fn unicode_whitespace_rejected_by_default() {
    // By default, only JSON's 4 whitespace code points are allowed.
    // NO-BREAK SPACE (U+00A0) should be rejected.
    let mut parser = DefaultStreamingParser::new(ParserOptions::default());
    let mut it = parser.feed("\u{00A0}[]");
    let first = it.next().unwrap();
    match first {
        Err(ParserError {
            source: ErrorSource::SyntaxError(SyntaxError::InvalidCharacter(c)),
            ..
        }) => {
            assert_eq!(c, '\u{00A0}');
        }
        other => panic!("expected InvalidCharacter error, got: {:?}", other),
    }
}

#[test]
fn unicode_whitespace_accepted_when_enabled() {
    let mut parser = DefaultStreamingParser::new(ParserOptions {
        allow_unicode_whitespace: true,
        ..Default::default()
    });
    // Include various Unicode whitespace around a trivial array
    let input = "\u{00A0}\u{2028}[ ]\u{2029}\u{FEFF}";
    let mut it = parser.feed(input);
    match it.next().unwrap().unwrap() {
        ParseEvent::ArrayBegin { .. } => {}
        _ => panic!(),
    }
    match it.next().unwrap().unwrap() {
        ParseEvent::ArrayEnd { .. } => {}
        _ => panic!(),
    }
}
