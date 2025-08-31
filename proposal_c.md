JSON Parser Buffer System Redesign (Proposal C)

Summary
- Unify ring-first and batch-borrow parsing behind a single, small “input session” that the iterator owns for one call to next().
- Make token capture explicit via a Cursor/Mark/Take interface that starts borrow-first and upgrades to owned on demand.
- Move per-token scratch and borrow-or-own decisions out of the parser’s fields into ephemeral session/capture types. Parser state keeps only positions and FSM state.
- Preserve existing external behavior and event lifetimes: fragments from the batch are borrowed; fragments from ring/buffer are owned.

Goals
- Simpler invariants: always consume ring first; otherwise parse from batch; never borrow from ring.
- Decouple parsing logic from input ownership mechanics and drop-time spillover handling.
- Make “borrow while possible, own when required” a first-class, local operation with minimal surface area.
- Keep performance: avoid needless copies, minimize char→byte scans, and keep batch-only paths zero-copy.

Operating Modes (what next() sees)
- A. Ring-first: ring has leftover chars. Parse only from ring until it empties; string/number fragments are owned (buffered).
- B. Borrow-from-batch: ring empty and a new &str batch is provided. Parse directly over batch; emit borrowed fragments when contiguous; upgrade to owned only on escapes or cross-batch tokens.
- C. End-of-input: no ring, no batch, closed; drive to completion or EOI errors.

Proposed Abstractions

1) InputSession
- One-per-iterator pass (one call chain that produces up to one event). Owns the temporary view over the current sources and position bookkeeping.
- Created by next() using core::mem::take(&mut self.source) to take ownership of the ring. Holds a borrowed view of the current batch, if any.

Sketch
  struct InputSession<'src> {
    // Sources
    ring: Buffer,                  // taken from parser via mem::take()
    batch: Option<&'src str>,      // provided by feed(), None for closed
    batch_bytes: usize,            // byte cursor into batch
    batch_chars: usize,            // char cursor into batch

    // Positions and control
    pos: usize, line: usize, col: usize,
    end_of_input: bool,
  }

  impl<'src> InputSession<'src> {
    fn peek(&self) -> PeekedChar;             // ring first, else batch, else EOI/Empty
    fn advance(&mut self) -> Option<char>;    // updates pos/line/col and cursors
    fn read_while<F>(&mut self, F) -> usize where F: FnMut(char)->bool; // advances only
    fn slice_in_batch(&self, start_pos: usize, end_pos: usize) -> Option<&'src str>;
    fn unread_batch_tail_into_ring(&mut self); // append batch[batch_bytes..] into ring
  }

Notes
- InputSession owns ring for the duration and is responsible for returning it to the parser on Drop, after appending any unread batch tail. The parser regains the ring at the end of next().
- pos/line/col live inside the session during the step and are copied back to the parser at the end (or the session holds &mut to the parser fields; either approach works).

2) Capture (Cursor/Mark/Take)
- A type for borrow-first token accumulation. It begins as a borrow candidate and upgrades to owned when ring participation or decoding differences require it.

Sketch
  enum CaptureKind { PropertyName, String { raw: bool }, Number }

  enum CaptureBuf {
    BorrowCandidate { start_pos: usize, had_escape: bool },
    OwnedUtf8(String),
    OwnedRaw(Vec<u8>),
  }

  struct Capture<'s, 'src> {
    sess: &'s mut InputSession<'src>,
    kind: CaptureKind,
    buf: CaptureBuf,
  }

  impl<'s, 'src> Capture<'s, 'src> {
    fn new(sess: &'s mut InputSession<'src>, kind: CaptureKind, start_pos: usize) -> Self;
    fn push_char(&mut self, ch: char);            // ensures upgrade to owned when needed
    fn push_raw_bytes(&mut self, bs: &[u8]);      // for surrogate-preserving mode
    fn mark_escape(&mut self);                    // switch to owned due to decode changes
    fn upgrade_to_owned(&mut self);               // ring involvement or cross-batch
    fn partial<'a>(&'a mut self) -> LexToken<'src>; // emit partial for strings, keep cursor
    fn finish(self) -> LexToken<'src>;            // emit final token for this capture
  }

Borrow vs Owned rules
- Borrowed slice is possible only when:
  - The capture buffer is still BorrowCandidate.
  - start_pos..end_pos lies entirely within the current batch.
  - Strings had no escape (decoded content matches source slice) and raw mode is off.
  - Numbers end within the batch and do not cross ring→batch.
- Owned upgrade triggers when:
  - Reading from ring while a capture is active.
  - Encountering an escape inside strings (decoded content differs from source slice), or raw-surrogate mode is requested.
  - Token spans feed boundaries (either begins in ring or ends after feeding more).
  - Numbers cannot be partially emitted; if depleted mid-number, switch to owned.

3) Drop Semantics
- InputSession implements Drop:
  - If any characters of the current batch are unread, move that tail into ring.
  - Positions are copied back to the parser.
- If next() exits while a Capture is active and it had already read a portion from the batch in BorrowCandidate state, the caller (parser) chooses one of:
  - For strings: allow partial emission before exit (preferred); otherwise, on exit, the parser requests Capture to upgrade_to_owned() and copy the in-flight prefix into the owned buffer so the token can resume later.
  - For property names and numbers: do not emit partials. On exit, Capture upgrades to owned and preserves the prefix as owned in the parser’s ring-backed scratch for the next call.

Public Event and Lexing Surface
- LexToken stays the same shape (borrowed vs buffered/owned variants) so ParseEvent generation remains stable.
- Parser code path becomes simpler because Capture handles “where the bytes live”. Parser only controls token FSM transitions and requests partial/finish at the right times.

How next() changes
1) Build an InputSession by taking the ring and providing the optional batch.
2) For each token start:
   - Create Capture with kind + current global pos as start.
   - Consume via sess.peek/advance and Capture.push_char(), using sess.read_while for simple runs.
   - On boundary/escape/EOI, ask Capture for partial or finish to get a LexToken.
3) Translate LexToken → ParseEvent using the existing backend.
4) Let InputSession drop, which returns the updated ring and unread batch tail to the parser.

API details to support current behaviors
- Strings
  - Borrow-first; emit partial fragments as borrowed when fully within batch and no escapes.
  - On first escape, call Capture.mark_escape() to upgrade to owned (Utf8 or Raw), optionally first emitting a borrowed prefix as a partial.
  - Surrogate-preserving: set kind = String { raw: true }. Capture stores bytes in OwnedRaw, with a helper to convert char→utf8 bytes when still valid.
- Numbers
  - Begin as BorrowCandidate at first digit/sign.
  - If the number completes within the batch, produce NumberBorrowed.
  - If batch ends mid-number or ring participates, upgrade_to_owned and keep appending until a delimiter, then emit NumberOwned/Buffered.
- Property names
  - Never emit partials. Borrow only when fully within current batch and no escape; otherwise Owned.

What moves out of StreamingParserImpl
- token_buffer, owned_batch_buffer, owned_batch_raw → replaced by CaptureBuf owned storage.
- token_start_pos, string_had_escape, token_is_owned → internal to CaptureBuf.
- batch_cursor, total_chars_pushed → replaced by InputSession’s batch_bytes/batch_chars; total_chars_pushed only needed if externally visible; otherwise drop it.
- next_event_with_and_batch simplifies: it constructs an InputSession, drives lexing, and relies on Capture for emission.

Pseudo-usage inside the lexer
  let mut sess = InputSession::new(mem::take(&mut self.source), batch, self.pos, self.line, self.column, self.end_of_input);
  loop {
    match state {
      Value if sess.peek() == Char('"') => {
        sess.advance();
        let mut cap = Capture::new(&mut sess, CaptureKind::String { raw: false }, sess.pos);
        // scan-until-quote, upgrading on escapes or ring involvement
        while let Char(c) = sess.peek() {
          match c {
            '\\' => { if cap.can_emit_partial_prefix() { emit(cap.partial()); } cap.mark_escape(); sess.advance(); handle_escape(&mut cap, &mut sess)?; },
            '"' => { sess.advance(); emit(cap.finish()); break; },
            _ => { sess.advance(); cap.push_char(c); }
          }
        }
      }
      // ... numbers, literals, punctuators similar
    }
  }

Critique / Trade-offs
- Pros
  - Clear separation: InputSession abstracts “where chars come from”; Capture abstracts “how token bytes are stored”. Parser keeps FSM only.
  - Fewer parser fields and cross-cutting flags; logic localized to Capture.
  - Borrow-first logic is encoded in one place with straightforward upgrade rules.
  - Drop semantics centralized; no ad-hoc copying in iterator Drop paths.

- Cons / Risks
  - Lifetimes: returning borrowed slices ties them to the batch owned by InputSession. Ensure LexToken only contains &'src from the batch, and Session lives at least through token production. Iterator must mediate this lifetime boundary carefully.
  - Performance: slice_in_batch(start,end) needs char→byte mapping. Current code rescans with char_indices. We can micro-opt by caching a small running map (e.g., track byte index alongside char count in batch), which InputSession already maintains (batch_bytes + batch_chars). For arbitrary start_pos, consider remembering the byte offset at capture start to avoid rescans.
  - Owned upgrades allocate once per token; ensure re-use by storing the OwnedUtf8/String inside Capture and reusing capacity across tokens (pool in parser if needed).
  - Buffer type choice: current ring is VecDeque<char> which is convenient for char-at-a-time but not contiguous. If throughput becomes an issue, consider a byte ring with UTF-8 decode at the edges. This is an orthogonal optimization and can be deferred.
  - Refactor size: moving fields and rewriting the iterator path is a significant change; comprehensive tests will be essential.

Implementation Notes / Migration Plan
- Step 1: Introduce InputSession and Capture types gated behind a feature flag or in a new module; write unit tests for their invariants.
- Step 2: Replace next_event_with_and_batch internals to use InputSession/Capture, keeping LexToken and ParseEvent unchanged.
- Step 3: Delete deprecated fields (token_buffer, owned_batch_buffer, owned_batch_raw, token_is_owned, token_start_pos, batch_cursor, string_had_escape, total_chars_pushed) after green tests.
- Step 4: Micro-optimize batch slicing by tracking capture_start_byte alongside capture_start_pos to avoid char_indices scans.
- Step 5: Optional: evaluate converting the ring to a small-gap String with head/tail indices or a smallvec-backed buffer.

Why this rescues the code
- Today the parser interleaves ownership concerns (where bytes live) with lexical state transitions, leading to flags and duplicated buffers. This proposal concentrates ownership transitions in Capture and source arbitration in InputSession. The iterator’s Drop logic becomes a one-liner: InputSession handles unread tails and any in-flight capture preservation. The result is shorter, easier-to-audit paths for strings and numbers, with a consistent partial/finish protocol.

Open Questions
- Should property names ever emit partial fragments for extremely long inputs? Keeping current behavior (no) simplifies state and path updates.
- Should Capture reuse buffers across tokens (held by parser) to cut allocations, or is per-token allocation acceptable given typical sizes?
- Do we want a trait Input for Ring and Batch and a CompositeInput impl, or is one struct simpler and clearer? The single struct is recommended for now.

Critique: Why This Might Fail
- Iterator-drop spill timing: The current implementation intentionally defers copying the unread batch tail into the ring until StreamingParserIteratorWith::drop. Proposal C incorrectly had InputSession push unread batch tail on Drop, which would duplicate data and can invalidate the borrow-first path by polluting the ring between successive next() calls within the same feed. This breaks the “borrow directly from current batch across multiple events” invariant.
- Ephemeral Capture loses state: Strings can emit multiple fragments per token. When an escape forces an upgrade to owned, subsequent fragments must append to the same owned buffer until the closing quote. A per-event (ephemeral) Capture would drop that buffer between next() calls, losing state. The owned accumulation for strings (and raw bytes when surrogate-preserving) must be persistent across events.
- mem::take() ergonomics: Taking ownership of self.source via mem::take() and returning it on Drop from an InputSession is awkward. It either needs unsafe plumbing to write back into the parser in Drop or a guard pattern. If not done carefully, early returns could leave the ring detached or lead to double moves.
- Borrow slicing costs and correctness: Using global char positions and then calling slice_chars for every borrow introduces repeated char_indices scans and risks mismatch if pos and batch cursors desynchronize. We need precise byte offsets at capture start to slice efficiently and correctly.
- Cross-source tokens: Tokens that begin in the ring and finish in the batch must be forced-owned for the entire token. If the code inadvertently allows a borrowed suffix from the batch, it would hand out invalid composite borrows. Enforcing this invariant requires a clear “started_in_ring” flag that outlives the event.
- Partial emission rules: Property names must not emit partial fragments. The design must ensure that when encountering an escape or batch depletion mid-key, we do not accidentally emit a partial borrowed prefix; instead we must preserve it and continue in owned mode. This constraint interacts with iterator drop and batch spill semantics.
- Lifetime entanglement: Borrowed fragments must be tied strictly to the feed’s batch lifetime, not to the ephemeral session. As long as the iterator lives, borrowed slices remain valid; if the session took on ownership of decisions about spill timing, it could inadvertently invalidate those borrows.
- Performance risks: Char-by-char APIs on VecDeque<char> plus frequent upgrade-to-owned copies could regress throughput if not optimized. The current code already has ring.copy_while and batch copy-while helpers; Proposal C needs equivalent fast-paths otherwise it may regress under digit runs or long unescaped string segments.

Revisions Based on Critique
- Keep spill-on-iterator-drop: Remove InputSession’s responsibility to push unread batch tail into the ring. Continue to rely on StreamingParserIteratorWith::drop to spill the batch tail exactly once, preserving borrow-first across multiple next() calls in one feed.
- Persist token accumulation as parser state: Replace ephemeral Capture with a parser-held TokenCapture that lives across events until a token completes.
  - Shape: enum TokenCapture { None, StringUtf8 { buf: String, had_escape: bool }, StringRaw { buf: Vec<u8> }, Number { buf: String }, PropertyName { buf: String, had_escape: bool } }
  - Flags: started_in_ring: bool, started_in_batch_byte: Option<usize> for borrow slicing, and token_kind metadata. This mirrors existing fields but consolidates them in one struct with clear methods.
  - Methods: start(kind, started_in_ring, start_batch_byte), push_char, push_raw, mark_escape (switch to owned), can_borrow_slice(end_batch_byte), emit_partial_or_finish(batch: &str) -> LexToken.
- Use a guard for mem::take() of ring: Implement a RingGuard that holds the taken Buffer and writes it back to self.source in Drop. The guard lives entirely inside next_event_internal, ensuring ring is always restored on all exit paths without unsafe. This guard does not manage batch spill.
- Track batch byte offsets: On token start in batch mode, record capture_start_batch_byte in TokenCapture; also maintain current batch byte cursor. Convert borrow slices with &batch[capture_start_batch_byte..current_batch_byte] without rescanning.
- Enforce source-origin invariant: If TokenCapture.started_in_ring is true, can_borrow_slice returns false for the entire token, forcing owned emission. This guarantees no mixed-source borrows.
- Clarify partial emission policy: Property names never partial. Strings may partial when: (a) an escape is encountered (emit prefix as borrowed if available), (b) batch depletes mid-string (emit borrowed prefix if started_in_batch and no escape yet). Numbers never partial.
- Re-scope InputSession: Narrow it to a thin facade over peek/advance with access to ring, batch, and cursors. It does not own ring nor move batch tails. Its only job is unified char reading and position updates, and exposing current batch byte cursor for slicing.
- Fast paths retained: Provide read_while variants:
  - ring.copy_while(dst: &mut String, pred)
  - batch.advance_while(pred) just advances counters (for borrowable path)
  - batch.copy_while_to_owned(dst: &mut String, pred) for owned path in batch mode

Adjusted Sketches
  struct TokenCapture {
    kind: CaptureKind,
    started_in_ring: bool,
    start_batch_byte: Option<usize>,
    had_escape: bool,
    utf8: String,
    raw: Vec<u8>,
  }

  impl TokenCapture {
    fn start_in_ring(&mut self, kind: CaptureKind) { self.started_in_ring = true; self.start_batch_byte = None; }
    fn start_in_batch(&mut self, kind: CaptureKind, start_byte: usize) { self.started_in_ring = false; self.start_batch_byte = Some(start_byte); }
    fn mark_escape(&mut self) { self.had_escape = true; }
    fn must_own(&self) -> bool { self.started_in_ring || self.had_escape || matches!(self.kind, CaptureKind::String { raw: true }) }
    fn can_borrow_slice(&self, end_byte: usize) -> bool { !self.must_own() && self.start_batch_byte.is_some() && self.start_batch_byte.unwrap() <= end_byte }
  }

  // Borrow slice when possible
  if cap.can_borrow_slice(sess.batch_byte()) {
     let s = &batch[cap.start_batch_byte.unwrap()..sess.batch_byte()];
     emit(LexToken::StringBorrowed(s));
  } else {
     // use cap.utf8 or cap.raw, appended via ring/batch copy_while paths
  }

Impact on Migration Plan
- Step 1 becomes: introduce TokenCapture and RingGuard; keep existing batch spill behavior. Write targeted tests for: ring→batch transition forces owned; partial string emission continues to append to the same owned buffer across events; property-name no-partial behavior holds.
- Step 2: refactor next_event_internal to use TokenCapture methods and RingGuard, but leave StreamingParserIteratorWith::drop unchanged.
- Step 3: remove scattered fields (token_buffer, owned_batch_buffer, owned_batch_raw, token_is_owned, token_start_pos, string_had_escape, batch_cursor) only after tests confirm TokenCapture covers them. Keep initialized_string and last_was_lone_low in parser.
- Step 4: optimize slicing using recorded start/end byte offsets; avoid char_indices rescans entirely on the hot path.

Residual Risks After Revision
- Complexity remains around surrogate-preserving raw accumulation across partials; ensure TokenCapture.raw persists and that ensure_raw_mode moves any existing utf8 into raw only once.
- Ring VecDeque<char> performance may still be a bottleneck for very large numbers or long whitespace runs; consider a byte-ring in a follow-up.
- Careful with early errors: guard must restore ring even when returning Err; write property tests that intentionally error mid-token.
