Streaming Parser Scanner Refactor — Standalone Design

This document specifies a concrete, low‑risk refactor of the streaming JSON parser’s internal input/lexing path. It replaces scattered buffer/flag handling with a single per‑feed “Scanner” that owns unread input and token scratch, while keeping all public behavior stable.

Scope
- Internal parser architecture only. Parse events, options, and lifetimes do not change.

Background (problem we’re fixing)
- Today the parser maintains several per‑token buffers (`token_buffer`, `owned_batch_buffer`, `owned_batch_raw`) and flags (`token_is_owned`, `string_had_escape`, etc.). Decisions about “borrow vs own” and “ring vs batch” are spread across the state machine and the iterator `Drop` logic. Borrowed slicing from the current batch typically rescans `char_indices` to find byte ranges.
- Required external behavior is already clear and must be preserved:
  - Ring‑first: always read unread input from the ring before the new batch.
  - Borrow only from the current batch. Never borrow from the ring.
  - Keys and numbers never fragment across feeds. String values may fragment.
  - Decode modes: StrictUnicode (error on invalid), ReplaceInvalid (replace with U+FFFD), SurrogatePreserving (preserve lone surrogates as raw bytes for strings).

Design Goals (practical and testable)
- Make ownership decisions explicit and local: one component decides borrow vs own and when to switch.
- Remove duplicate buffers and late merges: one scratch per token, with a clear “upgrade to owned” moment.
- Keep ring‑first and borrowing rules mechanically enforced by the API surface.
- Preserve exact current behavior for keys, numbers, and values under all decode modes.
- Provide fast paths (ASCII batch scan; constant‑time ring decode) and keep regression/perf checks simple.

Non‑Goals
- No change to ParseEvent, builder adapters, or public options.
- No new decode modes or user‑visible diagnostics.

Design Overview
- Scanner<'src> (per feed): Constructed by the iterator from persisted `Tape` with `Scanner::from_carryover(tape, batch)`. It owns:
  - the unread UTF‑8 ring (a `VecDeque<u8>`),
  - the current batch `&'src str` and its byte cursor,
  - global character/line/column counters,
  - a single token scratch buffer (UTF‑8 `String` or raw `Vec<u8>`), and the anchor for the current token.
- Finalization: The iterator, and only the iterator, calls `scanner.finish()` exactly once per feed. This pushes the unread batch tail into the ring and returns a new `Tape` (ring + positions + scratch) to store on the parser for the next feed.

Unread Ring (what lives there and why)
- Contents: only valid UTF‑8 bytes originating from prior feeds and unread batch tails. We do not store “raw”/WTF‑8 in the unread ring.
- Access: the ring is private to the Scanner; all decoding and pushing goes through the Scanner.

Core Types (public to the parser module)

```rust
enum Source { Ring, Batch }
enum FragmentPolicy { Disallowed, Allowed }
enum TokenBuf<'src> { Borrowed(&'src str), OwnedText(String), Raw(Vec<u8>) }
enum TokenScratch { Text(String), Raw(Vec<u8>) }

struct TokenAnchor {
    source: Source,
    start_char: usize,                   // for diagnostics
    start_byte_in_batch: Option<usize>,  // Some when started in Batch
    owned: bool,
    had_escape: bool,
    is_raw: bool,
    policy: FragmentPolicy,
}

pub(crate) struct Scanner<'src> { /* owns ring, batch, cursors, scratch, anchor */ }

impl<'src> Scanner<'src> {
    // construct/finalize
    pub fn from_carryover(tape: Tape, batch: &'src str) -> Self;
    pub fn finish(self) -> Tape; // single‑shot write‑back

    // read
    pub fn peek(&self) -> Option<Unit>;     // Unit { ch: char, ch_len: u8, source: Source }
    pub fn advance(&mut self) -> Option<Unit>;
    pub fn cur_source(&self) -> Source;

    // token lifecycle
    pub fn begin(&mut self, policy: FragmentPolicy);
    pub fn mark_escape(&mut self);                // first escape → upgrade to owned if needed
    pub fn ensure_raw(&mut self);                 // switch scratch to raw bytes (WTF‑8)
    pub fn switch_to_owned_prefix_if_needed(&mut self);

    // fast paths
    pub fn copy_while_ascii(&mut self, pred: impl Fn(u8)->bool) -> usize;  // batch‑only
    pub fn copy_while_char(&mut self, pred: impl Fn(char)->bool) -> usize; // ring path

    // borrowing (O(1) slice from the batch)
    pub fn try_borrow_slice(&self, end_adjust_bytes: usize) -> Option<&'src str>;

    // emit current fragment (final or partial)
    pub fn emit_fragment(&mut self, is_final: bool, end_adjust_bytes: usize) -> TokenBuf<'src>;
}
```

Borrow/Own/Raw Rules (exact)
- Borrowing is only possible from the current batch, and only when the entire token lies in this batch, no escapes were processed, and the token is not in raw mode.
- Keys (property names): never fragment. Result is Borrowed, OwnedText, or Raw (in SurrogatePreserving when lone surrogates occur). Whether Raw is accepted depends on the backend: raw‑capable backends can propagate it; the standard Rust backend will error in SurrogatePreserving if it must produce a surrogate‑preserving key.
- Numbers: never fragment. Borrow when fully in batch; otherwise OwnedText. Never Raw.
- String values: may fragment across feeds. Borrow when fully in batch and undecoded; otherwise OwnedText or Raw (when SurrogatePreserving is active and unpaired surrogates occur).

Unicode/Decode Modes (what actually happens)
- StrictUnicode: `\uXXXX` escapes must be valid; surrogate pairs must be correct and joined. Any invalid escape or unpaired surrogate is an error.
- ReplaceInvalid: join valid pairs; any invalid escape or unpaired surrogate is replaced with U+FFFD in the produced UTF‑8.
- SurrogatePreserving: applies to all strings (keys and values). Valid pairs are joined. If a lone surrogate appears, the Scanner emits `TokenBuf::Raw` for that string segment. A raw‑capable backend accepts and forwards this. The standard Rust backend, which is UTF‑8 only, returns an error in SurrogatePreserving when it would otherwise need to materialize a surrogate‑preserving string (key or value). Numbers are unaffected.

Iterator Integration
- Both `StreamingParserIteratorWith` and `ClosedStreamingParser` hold a `Scanner<'src>` for the life of the iteration.
- On Drop, the iterator consumes the Scanner and stores its `Tape`: `self.tape = core::mem::take(&mut self.scanner).finish()`.
- During migration, legacy logic continues to run; Scanner runs in shadow for parity until we flip individual surfaces to use Scanner outputs.

Migration Plan (step by step)
1) Land `parser/scanner/mod.rs` with `Scanner`, `Tape`, `TokenScratch`, `TokenBuf`, `TokenAnchor`. Use byte anchors for O(1) slicing.
2) Treat ring/scratch/pos on the parser as opaque carryover owned by `Tape`; do not mutate them elsewhere.
3) Route lexing through the Scanner in phases:
   - strings (keys) without escapes → Scanner (already flipped),
   - value strings without escapes,
   - numbers,
   - literals. Use `begin/mark_escape/ensure_raw/copy_while*/emit_fragment` and `try_borrow_slice` where applicable.
4) Remove `BatchView/BatchCursor` and all `slice_chars` rescans once parity holds; use `start_byte_in_batch..batch_bytes` spans instead.
5) Remove legacy fields (`token_buffer`, `owned_batch_buffer`, `owned_batch_raw`, `token_is_owned`, `token_start_pos`).
6) Multiple‑values mode: validate token state resets across End→Start transitions.
7) Validation: fuzz every split point; assert numbers/keys never fragment; verify decode modes including reversed surrogate order; benchmark ASCII batch and ring paths.

Risks and Mitigations
- Double finalization: avoided because `finish(self)` consumes the Scanner; iterators replace the field via `mem::take` before calling it.
- Borrowing hazards: only `try_borrow_slice` can yield an `&'src str`, and only from the batch; no ring borrows are possible.
- Duplicate fragments: `switch_to_owned_prefix_if_needed()` is idempotent; call at the first transform boundary (escape, raw switch).
- Performance regressions: keep fast paths and run parity perf checks during the bake; delete `slice_chars` rescans after flip.

Definition of Done
- All string/number/literal surfaces read via Scanner; legacy buffers and `BatchView/BatchCursor` removed.
- Fuzz tests pass with arbitrary feed boundaries; property names and numbers never fragment; value strings fragment as before.
- Decode‑mode behavior matches current tests (StrictUnicode/ReplaceInvalid/SurrogatePreserving) for both keys and values.
- Benchmarks show no meaningful regression on ASCII‑heavy streams; ring path matches previous behavior.
- Code comments and docs align with the final names (“Scanner”, not “InputSession”).

Glossary
- Ring: the parser’s unread‑input queue (UTF‑8 bytes) carried across feeds.
- Batch: the `&str` input chunk passed to `feed()` for this iteration.
- Borrowed: an `&str` slice that refers directly to the current batch.
- Owned: a `String` (UTF‑8) or `Vec<u8>` (raw/WTF‑8) held by the parser.
- Shadowing: running the new Scanner path in parallel with the legacy path and asserting parity in debug builds.

Repository Status Snapshot (2025‑09‑01)
- Files present: `crates/jsonmodem/src/parser/scanner/mod.rs` (Scanner/Tape APIs and tests) and `crates/jsonmodem/src/parser/mod.rs` (iterators and shadow integration).
- Iterators own a `Scanner<'src>` and finalize via `finish()` on Drop.
- Shadowing is active: whitespace, literals, numbers, and string fast‑paths are Scanner‑driven with `debug_assert!` parity; positions kept in sync.
- Switched: property names without escapes (started in this batch) emit via Scanner.
- Pending flips: value strings (no escapes), numbers, and literals. Escapes and surrogate‑preserving raw emission already call into Scanner for mode switches but still return legacy tokens until flipped.
