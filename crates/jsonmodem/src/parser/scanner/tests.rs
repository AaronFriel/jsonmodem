use super::*;

fn carry(ring_s: &str) -> ScannerState {
    let mut c = ScannerState::default();
    c.pending.extend(ring_s.as_bytes().iter().copied());
    c
}

#[test]
fn number_borrowed_when_fully_in_batch() {
    let batch = "12345,";
    let mut s = Scanner::from_state(carry(""), batch);
    // lazy anchor
    // copy ASCII digits fast from batch
    let n = s.consume_while_ascii(|b| (b as char).is_ascii_digit());
    assert_eq!(n, 5);
    match s.emit() {
        Capture::Borrowed(frag) => assert_eq!(frag, "12345"),
        other => panic!("expected borrowed, got {other:?}"),
    }
    // Delimiter still present in batch tail
    assert_eq!(s.peek().unwrap().ch, ',');
}

#[test]
fn number_owned_when_split_ring_then_batch() {
    let mut s = Scanner::from_state(carry("12"), "345,");
    // anchor starts in ring, so owned
    let copied_ring = s.consume_while_char(|c| c.is_ascii_digit());
    assert_eq!(copied_ring, 2);
    let copied_batch = s.consume_while_ascii(|b| (b as char).is_ascii_digit());
    assert_eq!(copied_batch, 3);
    match s.emit() {
        Capture::Owned(sv) => assert_eq!(sv, "12345"),
        other => panic!("expected owned, got {other:?}"),
    }
}

#[test]
fn key_string_borrowed_simple() {
    let batch = "abc\""; // closing quote included
    let mut s = Scanner::from_state(carry(""), batch);
    // lazy anchor
    // Consume three ASCII letters, then emit final before quote
    s.consume_while_ascii(|b| (b as char).is_ascii_alphabetic());
    match s.emit() {
        Capture::Borrowed(f) => assert_eq!(f, "abc"),
        other => panic!("expected borrowed, got {other:?}"),
    }
    assert_eq!(s.peek().unwrap().ch, '"');
    let _ = s.consume();
}

#[test]
fn key_string_switches_to_owned_on_escape_prefix_copy_once() {
    // abc\x (simulate encountering a backslash after reading abc)
    let batch = "abc\\rest";
    let mut s = Scanner::from_state(carry(""), batch);
    // lazy anchor
    s.consume_while_ascii(|b| (b as char).is_ascii_alphabetic()); // abc
    // Encounter escape: copy prefix into scratch and set owned
    s.switch_to_owned_prefix_if_needed();
    // After escape handling the lexer would append decoded unit; simulate pushing 'X'
    s.scratch.as_text_mut().push('X');
    // Skip the backslash (already consumed by lexer in real flow)
    let _ = s.skip();
    // Continue reading remaining letters
    s.consume_while_ascii(|b| (b as char).is_ascii_alphabetic()); // rest
    match s.emit() {
        Capture::Owned(v) => assert_eq!(v, "abcXrest"),
        other => panic!("expected owned text, got {other:?}"),
    }
}

#[test]
fn raw_allowed_for_keys_reported_as_raw() {
    // SurrogatePreserving mode should yield Raw when ensure_raw() engaged.
    let batch = "A\""; // A followed by closing quote
    let mut s = Scanner::from_state(carry(""), batch);
    // lazy anchor
    s.consume_while_ascii(|b| (b as char).is_ascii_alphabetic()); // 'A'
    // Switch to raw (e.g., due to an unpaired surrogate escape)
    s.ensure_raw();
    match s.emit() {
        Capture::Raw(bytes) => {
            assert_eq!(bytes, b"A");
        }
        other => panic!("expected raw, got {other:?}"),
    }
    assert_eq!(s.peek().unwrap().ch, '"');
    let _ = s.consume();
}

#[test]
fn finish_preserves_key_prefix_and_unread_tail() {
    // batch: key prefix 'ab' read; iterator dropped before more input
    let batch = "abXYZ"; // we read only 'ab'
    let mut s = Scanner::from_state(carry(""), batch);
    // lazy anchor
    s.consume_while_ascii(|b| (b as char).is_ascii_alphabetic()); // read entire batch; back up to simulate mid-key
    // Simulate that only 'a','b' were read
    s.byte_idx = 2;
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
    let mut sess = Scanner::from_state(carry(""), s1);
    // lazy anchor
    // Advance over å
    let _ = sess.consume();
    match sess.emit() {
        Capture::Borrowed(t) => assert_eq!(t, "å"),
        other => panic!("expected borrowed 'å', got {other:?}"),
    }
    assert_eq!(sess.peek().unwrap().ch, '"');
    let _ = sess.consume();

    // Mixed ASCII and non-ASCII for value, with borrow across entire batch
    let s2 = "abcÅdef\""; // Å is non-ASCII
    let mut sess = Scanner::from_state(carry(""), s2);
    // lazy anchor
    sess.consume_while_ascii(|b| (b as char).is_ascii()); // 'abc'
    let _ = sess.consume(); // 'Å'
    sess.consume_while_ascii(|b| (b as char).is_ascii_alphabetic()); // 'def'
    match sess.emit() {
        Capture::Borrowed(t) => assert_eq!(t, "abcÅdef"),
        other => panic!("expected borrowed 'abcÅdef', got {other:?}"),
    }
    assert_eq!(sess.peek().unwrap().ch, '"');
    let _ = sess.consume();
}

/// Repro for cross-feed duplication when a partial fragment is emitted
/// without clearing the anchor (simulates an incorrect caller that used
/// `emit_fragment(true)` instead of `emit()`). The final fragment becomes
/// Owned("abcdef") instead of the expected borrowed "def".
#[test]
fn repro_cross_feed_borrow_then_owned_duplication() {
    // Feed 1: read "abc" of a value string (caller keeps anchor alive)
    let mut s = Scanner::from_state(carry(""), "abc");
    // Start token and consume ascii letters from the batch
    // Use per-char consume(), which always appends to scratch when a token is active
    // regardless of ownership. This mirrors the parser’s char-by-char path.
    assert_eq!(s.consume().unwrap().ch, 'a');
    assert_eq!(s.consume().unwrap().ch, 'b');
    assert_eq!(s.consume().unwrap().ch, 'c');
    // Incorrect emission path: emit a final-looking fragment but DO NOT clear anchor
    // (using emit_fragment instead of emit). This returns a borrowed slice.
    match s.emit_fragment(true) {
        Capture::Borrowed(t) => assert_eq!(t, "abc"),
        other => panic!("expected borrowed, got {other:?}"),
    }

    // Drop the session – finish() will coalesce the batch prefix into scratch
    // because the anchor is still present and not owned.
    let carry = s.finish();
    // Validate that the scratch now holds the previously emitted prefix.
    assert_eq!(carry.test_scratch_text(), Some("abc"));

    // Feed 2: continue with "def"] and emit final – the coalesced prefix
    // will be duplicated into the final owned buffer.
    let mut s = Scanner::from_state(carry, "def\"]");
    assert_eq!(s.consume().unwrap().ch, 'd');
    assert_eq!(s.consume().unwrap().ch, 'e');
    assert_eq!(s.consume().unwrap().ch, 'f');
    match s.emit() {
        Capture::Owned(v) => assert_eq!(v, "abcdef"),
        other => panic!("expected owned concatenation, got {other:?}"),
    }
}

/// Expectation: after emitting a borrowed prefix across feeds, the tail should
/// still be borrowable and must not include the already-emitted prefix.
///
/// This mirrors the parser flow for string_cross_batch_borrows_fragments:
/// - feed1: emit() once at string start (empty borrowed), then consume "abc"
///   and emit() a partial fragment (borrowed "abc").
/// - feed2: consume "def" and emit() final; expected Borrowed("def").
/// Current buggy behavior returns Owned("abcdef") due to scratch carryover.
#[test]
fn value_string_cross_feed_should_not_duplicate_prefix() {
    // Feed 1: prefix segment
    let mut s = Scanner::from_state(carry(""), "abc");
    // Parser calls emit at string entry to reset; returns empty borrowed.
    match s.emit() {
        Capture::Borrowed(t) => assert_eq!(t, ""),
        other => panic!("expected empty borrowed at entry, got {other:?}"),
    }
    let n = s.consume_while_ascii(|b| (b as char).is_ascii_lowercase());
    assert_eq!(n, 3);
    match s.emit() {
        Capture::Borrowed(t) => assert_eq!(t, "abc"),
        other => panic!("expected borrowed prefix, got {other:?}"),
    }
    let carry = s.finish();

    // Feed 2: tail segment
    let mut s = Scanner::from_state(carry, "def\"]");
    let n = s.consume_while_ascii(|b| (b as char).is_ascii_lowercase());
    assert_eq!(n, 3);
    match s.emit() {
        Capture::Borrowed(t) => assert_eq!(t, "def"),
        other => panic!("expected final borrowed tail 'def', got {other:?}"),
    }
}

/// End-to-end repro mirroring the parser log sequence the user observed.
/// Steps:
/// 1) feed "[\"" and skip '[' then finish (leaves '"' in ring)
/// 2) next feed "abc": skip the leading '"', emit() => Borrowed("") at entry,
///    then consume 'a','b','c' and emit() => Borrowed("abc")
/// 3) next feed "def\"]": consume 'd','e','f' and emit() => Owned("abcdef")
#[test]
fn repro_from_logs_owned_concat_after_partial_borrow() {
    // Step 1
    let mut s = Scanner::from_state(carry(""), "[\"");
    let _ = s.skip(); // '['
    let carry = s.finish();

    // Step 2
    let mut s = Scanner::from_state(carry, "abc");
    let _ = s.skip(); // '"' from ring
    match s.emit() {
        Capture::Borrowed(t) => assert_eq!(t, ""),
        other => panic!("expected empty borrowed entry, got {other:?}"),
    }
    assert_eq!(s.consume().unwrap().ch, 'a');
    assert_eq!(s.consume().unwrap().ch, 'b');
    assert_eq!(s.consume().unwrap().ch, 'c');
    match s.emit() {
        Capture::Borrowed(t) => assert_eq!(t, "abc"),
        other => panic!("expected borrowed 'abc', got {other:?}"),
    }
    let carry = s.finish();

    // Step 3
    let mut s = Scanner::from_state(carry, "def\"]");
    assert_eq!(s.consume().unwrap().ch, 'd');
    assert_eq!(s.consume().unwrap().ch, 'e');
    assert_eq!(s.consume().unwrap().ch, 'f');
    match s.emit() {
        Capture::Borrowed(t) => assert_eq!(t, "def"),
        other => panic!("expected final borrowed tail 'def', got {other:?}"),
    }
}

#[test]
fn empty_key_and_value_strings_borrow_correctly() {
    // Key: ""
    let mut s = Scanner::from_state(carry(""), "\"");
    // lazy anchor
    match s.emit() {
        Capture::Borrowed(t) => assert_eq!(t, ""),
        other => panic!("expected borrowed empty key, got {other:?}"),
    }
    assert_eq!(s.peek().unwrap().ch, '"');
    let _ = s.consume();

    // Value: ""
    let mut s = Scanner::from_state(carry(""), "\"");
    // lazy anchor
    match s.emit() {
        Capture::Borrowed(t) => assert_eq!(t, ""),
        other => panic!("expected borrowed empty value, got {other:?}"),
    }
    assert_eq!(s.peek().unwrap().ch, '"');
    let _ = s.consume();
}

#[test]
fn switch_to_owned_prefix_is_idempotent_and_no_duplication() {
    let batch = "abcdef";
    let mut s = Scanner::from_state(carry(""), batch);
    // lazy anchor
    s.consume_while_ascii(|b| (b as char).is_ascii()); // all
    // Copy prefix twice; second call must be a no-op
    s.switch_to_owned_prefix_if_needed();
    s.switch_to_owned_prefix_if_needed();
    match s.emit() {
        Capture::Owned(t) => assert_eq!(t, "abcdef"),
        other => panic!("expected owned text without duplication, got {other:?}"),
    }
}

#[test]
fn numbers_borrow_exclude_delimiters_and_peek_delim() {
    // delimiter comma
    let mut s = Scanner::from_state(carry(""), "12345,");
    // lazy anchor
    s.consume_while_ascii(|b| (b as char).is_ascii_digit());
    match s.emit() {
        Capture::Borrowed(t) => assert_eq!(t, "12345"),
        other => panic!("expected borrowed number, got {other:?}"),
    }
    assert_eq!(s.peek().unwrap().ch, ',');

    // delimiter ]
    let mut s = Scanner::from_state(carry(""), "678]");
    // lazy anchor
    s.consume_while_ascii(|b| (b as char).is_ascii_digit());
    match s.emit() {
        Capture::Borrowed(t) => assert_eq!(t, "678"),
        other => panic!("expected borrowed number, got {other:?}"),
    }
    assert_eq!(s.peek().unwrap().ch, ']');
}

#[test]
fn skip_and_consume_sequences_match_units_and_positions() {
    let mut s = Scanner::from_state(carry(""), "abc");
    // lazy anchor
    let u1 = s.skip().unwrap();
    assert_eq!(u1.ch, 'a');
    let u2 = s.consume().unwrap();
    assert_eq!(u2.ch, 'b');
    let u3 = s.skip().unwrap();
    assert_eq!(u3.ch, 'c');
    assert!(s.peek().is_none());
    // Positions advanced by 3 characters; line/col updated accordingly
    #[cfg(debug_assertions)]
    {
        let (pos, _line, _col) = s.debug_positions();
        assert_eq!(pos, 3);
    }
}

#[test]
fn skip_works_across_ring_and_batch() {
    // Ring contains 'x', 'y'; batch contains 'z'
    let mut c = ScannerState::default();
    c.pending.extend(b"xy".iter().copied());
    let mut s = Scanner::from_state(c, "z");
    // lazy anchor
    let u1 = s.skip().unwrap();
    assert_eq!(u1.ch, 'x');
    let u2 = s.skip().unwrap();
    assert_eq!(u2.ch, 'y');
    let u3 = s.consume().unwrap();
    assert_eq!(u3.ch, 'z');
    assert!(s.peek().is_none());
}

#[test]
fn skip_returns_none_on_empty() {
    let mut s = Scanner::from_state(carry(""), "");
    assert!(s.skip().is_none());
    assert!(s.consume().is_none());
}

#[test]
fn alternate_consume_and_skip() {
    // Build a batch of 0..9 repeated 10 times
    let mut batch = String::new();
    for _ in 0..10 {
        batch.push_str("0123456789");
    }
    let mut s_by_peek = Scanner::from_state(carry(""), &batch);
    let mut s_by_scanner = Scanner::from_state(carry(""), &batch);

    loop {
        let Some(g) = s_by_peek.peek_guard() else {
            break;
        };
        // Capture the even-positioned digit
        g.consume();
        s_by_scanner.consume();

        let Some(g) = s_by_peek.peek_guard() else {
            break;
        };
        // Skip the odd-positioned digit
        g.skip();
        s_by_scanner.skip();
    }

    let expected = "02468".repeat(10);
    match s_by_peek.emit() {
        Capture::Owned(t) => assert_eq!(t, expected),
        Capture::Borrowed(b) => panic!("expected owned text, got borrowed: {b}"),
        Capture::Raw(b) => panic!("expected owned text, got raw: {:?}", b),
    }
    match s_by_scanner.emit() {
        Capture::Owned(t) => assert_eq!(t, expected),
        Capture::Borrowed(b) => panic!("expected owned text, got borrowed: {b}"),
        Capture::Raw(b) => panic!("expected owned text, got raw: {:?}", b),
    }
}




#[test]
fn alternate_skip_and_consume() {
    // Build a batch of 0..9 repeated 10 times
    let mut batch = String::new();
    for _ in 0..10 {
        batch.push_str("0123456789");
    }
    let mut s_by_peek = Scanner::from_state(carry(""), &batch);
    let mut s_by_scanner = Scanner::from_state(carry(""), &batch);

    loop {
        let Some(g) = s_by_peek.peek_guard() else {
            break;
        };
        // Skip the even-positioned digit
        g.consume();
        s_by_scanner.consume();

        let Some(g) = s_by_peek.peek_guard() else {
            break;
        };
        // Consume the odd-positioned digit
        g.skip();
        s_by_scanner.skip();
    }

    let expected = "02468".repeat(10);
    match s_by_peek.emit() {
        Capture::Owned(t) => assert_eq!(t, expected),
        Capture::Borrowed(b) => panic!("expected owned text, got borrowed: {b}"),
        Capture::Raw(b) => panic!("expected owned text, got raw: {:?}", b),
    }
    match s_by_scanner.emit() {
        Capture::Owned(t) => assert_eq!(t, expected),
        Capture::Borrowed(b) => panic!("expected owned text, got borrowed: {b}"),
        Capture::Raw(b) => panic!("expected owned text, got raw: {:?}", b),
    }
}



#[test]
fn peek_consume() {
    // Build a batch of 0..9 repeated 10 times
    let mut batch = String::new();
    for _ in 0..10 {
        batch.push_str("0123456789");
    }
    let mut s_by_peek = Scanner::from_state(carry(""), &batch);
    let mut s_by_scanner = Scanner::from_state(carry(""), &batch);

    // lazy anchor

    loop {
        let Some(g) = s_by_peek.peek_guard() else {
            break;
        };
        g.consume();
        s_by_scanner.consume();
    }

    let expected = "0123456789".repeat(10);
    match s_by_peek.emit() {
        Capture::Owned(t) => panic!("expected borrowed text, got owned: {t}"),
        Capture::Borrowed(b) => assert_eq!(b, expected),
        Capture::Raw(b) => panic!("expected owned text, got raw: {:?}", b),
    }
    match s_by_scanner.emit() {
        Capture::Owned(t) => panic!("expected borrowed text, got owned: {t}"),
        Capture::Borrowed(b) => assert_eq!(b, expected),
        Capture::Raw(b) => panic!("expected owned text, got raw: {:?}", b),
    }
}

#[test]
fn raw_hint_matches_decode_mode_for_keys() {
    let mut s = Scanner::from_state(carry(""), "A\"");
    s.consume_while_ascii(|b| (b as char).is_ascii_alphabetic());
    s.ensure_raw();
    match s.emit() {
        Capture::Raw(_) => (),
        other => panic!("expected raw, got {other:?}"),
    }
    assert_eq!(s.peek().unwrap().ch, '"');
    let _ = s.consume();

    let mut s = Scanner::from_state(carry(""), "A\"");
    // lazy anchor
    s.consume_while_ascii(|b| (b as char).is_ascii_alphabetic());
    s.ensure_raw();
    // raw hint removed: backend owns policy
}

#[test]
fn surrogate_flags_round_trip_in_carryover() {
    // Surrogate pairing state is owned by the parser; InputSession/CarryOver no
    // longer track it.
    let s = Scanner::from_state(carry(""), "");
    let _ = s.finish();
}

#[test]
fn try_borrow_fails_after_escape_or_raw_or_owned() {
    let batch = "abcdef\""; // ensure batch has content + closing quote
    // had_escape
    let mut s = Scanner::from_state(carry(""), batch);
    s.consume_while_ascii(|b| (b as char).is_ascii_alphabetic());
    s.switch_to_owned_prefix_if_needed();
    // consume quote
    assert!(s.try_borrow_slice().is_none());

    // is_raw
    let mut s = Scanner::from_state(carry(""), batch);
    s.consume_while_ascii(|b| (b as char).is_ascii_alphabetic());
    s.ensure_raw();
    assert!(s.try_borrow_slice().is_none());

    // owned=true
    let mut s = Scanner::from_state(carry(""), batch);
    s.consume_while_ascii(|b| (b as char).is_ascii_alphabetic());
    s.switch_to_owned_prefix_if_needed();
    assert!(s.try_borrow_slice().is_none());
}

#[test]
fn ensure_raw_is_idempotent_and_preserves_prefix() {
    let batch = "AB\"";
    let mut s = Scanner::from_state(carry(""), batch);
    // lazy anchor
    s.consume_while_ascii(|b| (b as char).is_ascii_alphabetic()); // AB
    s.ensure_raw();
    s.ensure_raw(); // second call should be a no-op
    match s.emit() {
        Capture::Raw(bytes) => assert_eq!(bytes, b"AB"),
        other => panic!("expected raw, got {other:?}"),
    }
    assert_eq!(s.peek().unwrap().ch, '"');
    let _ = s.consume();
}

#[test]
fn test_scanner_span_owned() {
    let s = Scanner::from_state(carry(""), "12");
    let carry = s.finish(); // Ensure "12" is owned
    let mut s = Scanner::from_state(carry, "345");
    s.consume_while_ascii(|b| b.is_ascii_digit());
    match s.emit() {

        Capture::Owned(t) => assert_eq!(t, "12345"),
        other => panic!("expected owned, got {other:?}"),
    }
}

#[test]
fn test_scanner_span_emoji() {
    let mut s = Scanner::from_state(carry(""), "[\"😀\"]");
    s.skip();
    s.skip();
    s.consume();
    match s.emit() {
        Capture::Borrowed(t) => assert_eq!(t, "😀"),
        other => panic!("expected owned, got {other:?}"),
    }
}

#[test]
fn value_fragment_partial_and_final_borrowing() {
    // Value string fragments: partial emits owned when accumulated; final can be
    // borrowed.
    let batch = "abcDEF\""; // we'll split work: own 'abc', then leave 'DEF' borrowable
    let mut s = Scanner::from_state(carry(""), batch);
    // Force owned by switching to owned prefix after 'abc'
    s.consume_while_ascii(|b| (b as char).is_ascii_lowercase()); // 'abc'
    s.switch_to_owned_prefix_if_needed();
    if let Some(Capture::Owned(t)) = s.emit_partial() {
        assert_eq!(t, "abc");
    } else {
        panic!("expected owned partial")
    }
    // Continue with remaining 'DEF' and closing quote, keep borrow-eligible
    s.consume_while_ascii(|b| (b as char).is_ascii_uppercase());
    match s.emit() {
        Capture::Owned(t) => assert_eq!(t, "DEF"),
        other => panic!("expected owned final (continued owned mode), got {other:?}"),
    }
    assert_eq!(s.peek().unwrap().ch, '"');
    let _ = s.consume();
}

#[test]
fn newline_updates_positions_across_ring_and_batch() {
    // Put a newline in ring and another in batch; ensure line/col advance.
    let carry = {
        let mut c = ScannerState::default();
        c.pending.extend(b"A\n");
        c
    };
    let mut s = Scanner::from_state(carry, "B\nC");
    assert_eq!(s.line, 1);
    assert_eq!(s.col, 1);
    // Consume 'A' (ring)
    let _ = s.consume();
    assert_eq!(s.line, 1);
    assert_eq!(s.col, 2);
    // Consume '\n' (ring)
    let _ = s.consume();
    assert_eq!(s.line, 2);
    assert_eq!(s.col, 1);
    // Now from batch: 'B'
    let _ = s.consume();
    assert_eq!(s.line, 2);
    assert_eq!(s.col, 2);
    // '\n'
    let _ = s.consume();
    assert_eq!(s.line, 3);
    assert_eq!(s.col, 1);
}
