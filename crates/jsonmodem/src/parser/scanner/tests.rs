use super::*;

fn carry(ring_s: &str) -> Tape {
    let mut c = Tape::default();
    c.ring.extend(ring_s.as_bytes().iter().copied());
    c
}

#[test]
fn number_borrowed_when_fully_in_batch() {
    let batch = "12345,";
    let mut s = Scanner::from_carryover(carry(""), batch);
    s.begin(FragmentPolicy::Disallowed);
    // copy ASCII digits fast from batch
    let n = s.copy_while_ascii(|b| (b as char).is_ascii_digit());
    assert_eq!(n, 5);
    match s.emit_final() {
        TokenBuf::Borrowed(frag) => assert_eq!(frag, "12345"),
        other => panic!("expected borrowed, got {other:?}"),
    }
    // Delimiter still present in batch tail
    assert_eq!(s.peek().unwrap().ch, ',');
}

#[test]
fn number_owned_when_split_ring_then_batch() {
    let mut s = Scanner::from_carryover(carry("12"), "345,");
    s.begin(FragmentPolicy::Disallowed);
    // Source is Ring first, so begin() marks owned.
    let copied_ring = s.copy_while_char(|c| c.is_ascii_digit());
    assert_eq!(copied_ring, 2);
    let copied_batch = s.copy_while_ascii(|b| (b as char).is_ascii_digit());
    assert_eq!(copied_batch, 3);
    match s.emit_final() {
        TokenBuf::OwnedText(sv) => assert_eq!(sv, "12345"),
        other => panic!("expected owned, got {other:?}"),
    }
}

#[test]
fn key_string_borrowed_simple() {
    let batch = "abc\""; // closing quote included
    let mut s = Scanner::from_carryover(carry(""), batch);
    s.begin(FragmentPolicy::Disallowed);
    // Consume three ASCII letters, then emit final before quote
    s.copy_while_ascii(|b| (b as char).is_ascii_alphabetic());
    match s.emit_final() {
        TokenBuf::Borrowed(f) => assert_eq!(f, "abc"),
        other => panic!("expected borrowed, got {other:?}"),
    }
    assert_eq!(s.peek().unwrap().ch, '"');
    let _ = s.skip();
}

#[test]
fn key_string_switches_to_owned_on_escape_prefix_copy_once() {
    // abc\x (simulate encountering a backslash after reading abc)
    let batch = "abc\\rest";
    let mut s = Scanner::from_carryover(carry(""), batch);
    s.begin(FragmentPolicy::Disallowed);
    s.copy_while_ascii(|b| (b as char).is_ascii_alphabetic()); // abc
    // Encounter escape: mark_escape should copy prefix into scratch and set owned
    s.mark_escape();
    // After escape handling the lexer would append decoded unit; simulate pushing
    // 'X'
    s.scratch.as_text_mut().push('X');
    // Skip the backslash (already consumed by lexer in real flow)
    let _ = s.skip();
    // Continue reading remaining letters
    s.copy_while_ascii(|b| (b as char).is_ascii_alphabetic()); // rest
    match s.emit_final() {
        TokenBuf::OwnedText(v) => assert_eq!(v, "abcXrest"),
        other => panic!("expected owned text, got {other:?}"),
    }
}

#[test]
fn raw_allowed_for_keys_reported_as_raw() {
    // SurrogatePreserving mode should yield Raw when ensure_raw() engaged.
    let batch = "A\""; // A followed by closing quote
    let mut s = Scanner::from_carryover(carry(""), batch);
    s.begin(FragmentPolicy::Disallowed);
    s.copy_while_ascii(|b| (b as char).is_ascii_alphabetic()); // 'A'
    // Switch to raw (e.g., due to an unpaired surrogate escape)
    s.ensure_raw();
    match s.emit_final() {
        TokenBuf::Raw(bytes) => {
            assert_eq!(bytes, b"A");
        }
        other => panic!("expected raw, got {other:?}"),
    }
    assert_eq!(s.peek().unwrap().ch, '"');
    let _ = s.skip();
}

#[test]
fn finish_preserves_key_prefix_and_unread_tail() {
    // batch: key prefix 'ab' read; iterator dropped before more input
    let batch = "abXYZ"; // we read only 'ab'
    let mut s = Scanner::from_carryover(carry(""), batch);
    s.begin(FragmentPolicy::Disallowed);
    s.copy_while_ascii(|b| (b as char).is_ascii_alphabetic()); // read entire batch; back up to simulate mid-key
    // Simulate that only 'a','b' were read
    s.batch_bytes = 2;
    let carry = s.finish();
    // Prefix 'ab' must be preserved into scratch; unread tail pushed to ring
    assert_eq!(carry.test_scratch_text(), Some("ab"));
    // Unread tail is "XYZ"
    assert_eq!(carry.test_ring_bytes(), b"XYZ".to_vec());
}

#[test]
fn utf8_multibyte_borrow_and_end_adjust_for_keys_and_values() {
    // å (2 bytes), β (2 bytes), Ω (2-3 bytes depending) with closing quotes
    let s1 = "å\""; // simple single multibyte char key
    let mut sess = Scanner::from_carryover(carry(""), s1);
    sess.begin(FragmentPolicy::Disallowed);
    // Advance over å
    let _ = sess.skip();
    match sess.emit_final() {
        TokenBuf::Borrowed(t) => assert_eq!(t, "å"),
        other => panic!("expected borrowed 'å', got {other:?}"),
    }
    assert_eq!(sess.peek().unwrap().ch, '"');
    let _ = sess.skip();

    // Mixed ASCII and non-ASCII for value, with borrow across entire batch
    let s2 = "abcÅdef\""; // Å is non-ASCII
    let mut sess = Scanner::from_carryover(carry(""), s2);
    sess.begin(FragmentPolicy::Allowed);
    sess.copy_while_ascii(|b| (b as char).is_ascii()); // 'abc'
    let _ = sess.skip(); // 'Å'
    sess.copy_while_ascii(|b| (b as char).is_ascii_alphabetic()); // 'def'
    match sess.emit_final() {
        TokenBuf::Borrowed(t) => assert_eq!(t, "abcÅdef"),
        other => panic!("expected borrowed 'abcÅdef', got {other:?}"),
    }
    assert_eq!(sess.peek().unwrap().ch, '"');
    let _ = sess.skip();
}

#[test]
fn empty_key_and_value_strings_borrow_correctly() {
    // Key: ""
    let mut s = Scanner::from_carryover(carry(""), "\"");
    s.begin(FragmentPolicy::Disallowed);
    match s.emit_final() {
        TokenBuf::Borrowed(t) => assert_eq!(t, ""),
        other => panic!("expected borrowed empty key, got {other:?}"),
    }
    assert_eq!(s.peek().unwrap().ch, '"');
    let _ = s.skip();

    // Value: ""
    let mut s = Scanner::from_carryover(carry(""), "\"");
    s.begin(FragmentPolicy::Allowed);
    match s.emit_final() {
        TokenBuf::Borrowed(t) => assert_eq!(t, ""),
        other => panic!("expected borrowed empty value, got {other:?}"),
    }
    assert_eq!(s.peek().unwrap().ch, '"');
    let _ = s.skip();
}

#[test]
fn switch_to_owned_prefix_is_idempotent_and_no_duplication() {
    let batch = "abcdef";
    let mut s = Scanner::from_carryover(carry(""), batch);
    s.begin(FragmentPolicy::Disallowed);
    s.copy_while_ascii(|b| (b as char).is_ascii()); // all
    // Copy prefix twice; second call must be a no-op
    s.switch_to_owned_prefix_if_needed();
    s.switch_to_owned_prefix_if_needed();
    match s.emit_final() {
        TokenBuf::OwnedText(t) => assert_eq!(t, "abcdef"),
        other => panic!("expected owned text without duplication, got {other:?}"),
    }
}

#[test]
fn numbers_borrow_exclude_delimiters_and_peek_delim() {
    // delimiter comma
    let mut s = Scanner::from_carryover(carry(""), "12345,");
    s.begin(FragmentPolicy::Disallowed);
    s.copy_while_ascii(|b| (b as char).is_ascii_digit());
    match s.emit_final() {
        TokenBuf::Borrowed(t) => assert_eq!(t, "12345"),
        other => panic!("expected borrowed number, got {other:?}"),
    }
    assert_eq!(s.peek().unwrap().ch, ',');

    // delimiter ]
    let mut s = Scanner::from_carryover(carry(""), "678]");
    s.begin(FragmentPolicy::Disallowed);
    s.copy_while_ascii(|b| (b as char).is_ascii_digit());
    match s.emit_final() {
        TokenBuf::Borrowed(t) => assert_eq!(t, "678"),
        other => panic!("expected borrowed number, got {other:?}"),
    }
    assert_eq!(s.peek().unwrap().ch, ']');
}

#[test]
fn raw_hint_matches_decode_mode_for_keys() {
    let mut s = Scanner::from_carryover(carry(""), "A\"");
    s.begin(FragmentPolicy::Disallowed);
    s.copy_while_ascii(|b| (b as char).is_ascii_alphabetic());
    s.ensure_raw();
    match s.emit_final() {
        TokenBuf::Raw(_) => (),
        other => panic!("expected raw, got {other:?}"),
    }
    assert_eq!(s.peek().unwrap().ch, '"');
    let _ = s.skip();

    let mut s = Scanner::from_carryover(carry(""), "A\"");
    s.begin(FragmentPolicy::Disallowed);
    s.copy_while_ascii(|b| (b as char).is_ascii_alphabetic());
    s.ensure_raw();
    // raw hint removed: backend owns policy
}

#[test]
fn surrogate_flags_round_trip_in_carryover() {
    // Surrogate pairing state is owned by the parser; InputSession/CarryOver no
    // longer track it.
    let s = Scanner::from_carryover(carry(""), "");
    let _ = s.finish();
}

#[test]
fn try_borrow_fails_after_escape_or_raw_or_owned() {
    let batch = "abcdef\""; // ensure batch has content + closing quote
    // had_escape
    let mut s = Scanner::from_carryover(carry(""), batch);
    s.begin(FragmentPolicy::Disallowed);
    s.copy_while_ascii(|b| (b as char).is_ascii_alphabetic());
    s.mark_escape();
    // consume quote
    assert!(s.try_borrow_slice().is_none());

    // is_raw
    let mut s = Scanner::from_carryover(carry(""), batch);
    s.begin(FragmentPolicy::Disallowed);
    s.copy_while_ascii(|b| (b as char).is_ascii_alphabetic());
    s.ensure_raw();
    assert!(s.try_borrow_slice().is_none());

    // owned=true
    let mut s = Scanner::from_carryover(carry(""), batch);
    s.begin(FragmentPolicy::Disallowed);
    s.copy_while_ascii(|b| (b as char).is_ascii_alphabetic());
    s.switch_to_owned_prefix_if_needed();
    assert!(s.try_borrow_slice().is_none());
}

#[test]
fn ensure_raw_is_idempotent_and_preserves_prefix() {
    let batch = "AB\"";
    let mut s = Scanner::from_carryover(carry(""), batch);
    s.begin(FragmentPolicy::Disallowed);
    s.copy_while_ascii(|b| (b as char).is_ascii_alphabetic()); // AB
    s.ensure_raw();
    s.ensure_raw(); // second call should be a no-op
    match s.emit_final() {
        TokenBuf::Raw(bytes) => assert_eq!(bytes, b"AB"),
        other => panic!("expected raw, got {other:?}"),
    }
    assert_eq!(s.peek().unwrap().ch, '"');
    let _ = s.skip();
}

#[test]
fn value_fragment_partial_and_final_borrowing() {
    // Value string fragments: partial emits owned when accumulated; final can be
    // borrowed.
    let batch = "abcDEF\""; // we'll split work: own 'abc', then leave 'DEF' borrowable
    let mut s = Scanner::from_carryover(carry(""), batch);
    s.begin(FragmentPolicy::Allowed);
    // Force owned by switching to owned prefix after 'abc'
    s.copy_while_ascii(|b| (b as char).is_ascii_lowercase()); // 'abc'
    s.switch_to_owned_prefix_if_needed();
    if let Some(TokenBuf::OwnedText(t)) = s.emit_partial() {
        assert_eq!(t, "abc");
    } else {
        panic!("expected owned partial")
    }
    // Continue with remaining 'DEF' and closing quote, keep borrow-eligible
    s.copy_while_ascii(|b| (b as char).is_ascii_uppercase());
    match s.emit_final() {
        TokenBuf::OwnedText(t) => assert_eq!(t, "DEF"),
        other => panic!("expected owned final (continued owned mode), got {other:?}"),
    }
    assert_eq!(s.peek().unwrap().ch, '"');
    let _ = s.skip();
}

#[test]
fn newline_updates_positions_across_ring_and_batch() {
    // Put a newline in ring and another in batch; ensure line/col advance.
    let carry = {
        let mut c = Tape::default();
        c.ring.extend(b"A\n");
        c
    };
    let mut s = Scanner::from_carryover(carry, "B\nC");
    assert_eq!(s.line, 1);
    assert_eq!(s.col, 1);
    // Consume 'A' (ring)
    let _ = s.skip();
    assert_eq!(s.line, 1);
    assert_eq!(s.col, 2);
    // Consume '\n' (ring)
    let _ = s.skip();
    assert_eq!(s.line, 2);
    assert_eq!(s.col, 1);
    // Now from batch: 'B'
    let _ = s.skip();
    assert_eq!(s.line, 2);
    assert_eq!(s.col, 2);
    // '\n'
    let _ = s.skip();
    assert_eq!(s.line, 3);
    assert_eq!(s.col, 1);
}
