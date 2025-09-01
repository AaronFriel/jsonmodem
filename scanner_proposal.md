Scanner: Borrow-First Streaming Scanner (Simplified API)

Purpose
- Replace scattered buffer/flag handling with a single per-feed Scanner that owns unread input and token scratch, while keeping public behavior stable and tests green.
- Make borrow-first decisions local, remove footguns (no delimiter parameters, no end_adjust), and standardize on “emit, then advance”.

Invariants
- Ring contains only valid UTF-8 bytes; it holds unread tails between feeds.
- Borrowed slices come only from the current feed batch; never from the ring.
- Keys and numbers never fragment across feeds; string values may fragment.
- finish(self) is single-shot: it consumes the Scanner, saves positions/scratch/ring, and appends unread batch tail to the ring.
- Emission discipline: emit the fragment first, then advance to consume delimiters.

Core Types
- Source: Ring | Batch
- FragmentPolicy: Disallowed (keys, numbers) | Allowed (string values)
- TokenBuf<'src>: Borrowed(&'src str) | OwnedText(String) | Raw(Vec<u8>)
- Tape: carryover state (ring, pos/line/col, token scratch)
- Unit: { ch: char, ch_len: u8, source: Source }

Scanner API (as-used by the parser)
- Construction/finalization
  - from_carryover(tape: Tape, batch: &'src str) -> Scanner<'src>
  - finish(self) -> Tape
- Reading & position
  - peek() -> Option<Unit>
  - advance() -> Option<Unit>
  - cur_source() -> Source
- Token lifecycle
  - begin(policy: FragmentPolicy)
  - ensure_raw()            // switch scratch to raw (WTF-8) mode, idempotent
  - push_text(&str) | push_char(char) | append_raw_bytes(&[u8]) // for escape decoding
- Fast paths
  - copy_while_ascii(pred: impl Fn(u8) -> bool) -> usize (batch-only)
  - copy_while_char(pred: impl Fn(char) -> bool) -> usize (ring/batch until source changes)
- Borrow/Emit helpers (simplified)
  - emit_partial() -> Option<TokenBuf<'src>>
    - If borrow-eligible and non-empty, returns Borrowed and internally acknowledges prefix (so finish() won’t duplicate it).
    - Else, returns OwnedText/Raw when scratch is non-empty; None if empty.
  - emit_final() -> TokenBuf<'src>
    - Emits final fragment (no delimiter adjustment) and clears the current anchor.
  - yield_prefix() -> Option<TokenBuf<'src>>
    - For Allowed strings at a transform boundary (e.g., escape): returns Some(Borrowed prefix) if still borrow-eligible and non-empty; otherwise switches to owned and returns None. For Disallowed (keys, numbers), switches to owned and returns None.
  - own_prefix() // explicitly switch to owned by copying batch prefix once (idempotent)

Behavior Rules
- Borrowing is allowed only when:
  - Token began in Batch, not Ring; no owned/raw switch yet; no escape; and the byte range is fully within the current batch.
- Owned switch triggers on:
  - Token began in Ring; or a transform occurs (escape, surrogate-preserving raw) while in Batch; or the token spans feeds.
- Raw mode: ensure_raw() migrates accumulated UTF-8 into raw bytes and disables borrowing for this token.
- Fragmentation:
  - FragmentPolicy::Disallowed (keys, numbers): never fragment across feeds; finish() coalesces any batch prefix into scratch if still borrowing when iteration ends.
  - FragmentPolicy::Allowed (string values): may emit partial fragments; emit_partial() acknowledges borrowed prefixes.

Parser Integration (emit-then-advance)
- Token start: scanner.begin(policy).
- Fast path: copy_while_* to consume until a boundary.
- On escape (value strings):
  - if let Some(prefix) = scanner.yield_prefix() { emit partial prefix } else { scanner.own_prefix(); }
  - decode escape units via push_char/push_text or ensure_raw()+append_raw_bytes.
- Feed end mid-string: if let Some(part) = scanner.emit_partial() { emit partial } else { return Eof }.
- Close quote: let frag = scanner.emit_final(); then advance() to eat the quote; map frag into LexToken.
- Numbers/keys: begin(Disallowed); never partial; emit only at completion (no delimiter adjustment), then advance delimiter in the parser.

Footgun Removal
- No delimiter parameter: callers never pass a delimiter to Scanner. The parser follows a fixed pattern: emit, then advance the delimiter.
- No end_adjust: API does not take end_adjust_bytes; correctness relies on call ordering.

Carryover & Drop
- Iterator owns a Scanner for the life of iteration. On iterator drop, it calls finish() once to:
  - coalesce non-fragment tokens (if needed),
  - push unread batch tail to ring,
  - persist positions and scratch in Tape.

Decode Modes
- StrictUnicode: invalid escape or unpaired surrogate is an error (no Raw).
- ReplaceInvalid: invalid escape/unpaired surrogate become U+FFFD in UTF-8 OwnedText.
- SurrogatePreserving: string tokens may switch to Raw(Vec<u8>) when lone surrogates appear; keys remain UTF‑8 in standard backends (degrade or error upstream as needed).

Migration Plan (from current mod.rs)
- Phase 1 (minimal & safe):
  - Add emit_final/emit_partial/yield_prefix/own_prefix to Scanner (done).
  - Close-quote sites: replace emit-with-adjust and position hacks with emit_final(); then advance the quote.
  - Escape boundary: replace mark_escape + manual prefix copying with yield_prefix() and own_prefix().
  - Feed-end partials: replace try_borrow_slice + acknowledge + emit(false, ..) with emit_partial().
- Phase 2 (cleanups): remove end_adjust usage; delete acknowledge_partial_borrow call sites; simplify legacy buffers.
- Phase 3 (numbers/keys): ensure Disallowed path uses Scanner emits and never fragments.
- Phase 4: prune legacy BatchView rescans; rely on Scanner’s byte anchors; remove duplicate per-token buffers.

Testing & Perf
- Keep current tests passing (cargo nextest run). Add focused tests for:
  - emit-then-advance sequencing,
  - partial emission acknowledgment,
  - Disallowed vs Allowed across feed boundaries,
  - Raw transitions.
- Benchmark ASCII fast path and ring-digit runs for regression checks.

Open Questions (post-MVP)
- Optional scratch flushing: maybe_flush_owned(cap) to emit large owned chunks proactively for value strings; not required for correctness.
- Policy hints for Raw emission plumbing to backends (keep in backend layer).

