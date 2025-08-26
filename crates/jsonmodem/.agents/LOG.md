# JSONModem Parser Refactor Log

Branch: wip-codex-branch

## Current Task
- Complete the refactor in `src/parser/mod.rs` to support a virtual source that drains the ring first, then reads from the current batch (borrow-first), fixing compile errors and ensuring default-feature tests pass.
- Implement principled buffering vs borrowing:
  - If the ring (`self.source`) is non-empty, drain it completely first and treat tokens as owned (write into `self.buffer`).
  - Once the ring is empty, lex directly from the current `BatchView` and borrow slices where possible.
  - While reading from the batch, never write into `self.buffer`; instead, borrow or, when forced (escapes/cross-batch), accumulate into a temporary `batch_owned_buffer` and emit owned events without touching `self.buffer`.
  - For strings, encountering an escape emits a partial fragment (borrowed if in-batch and no prior buffering), then switches to owned mode for the remainder of that string.
  - For numbers that can’t be borrowed (crossing ring/batch or spanning batches), produce owned numbers by concatenating any ring-built prefix with the batch-owned suffix.
- Ensure iterator `Drop` copies any in-flight fragment into the parser’s buffer (if needed) and appends the unread portion of the batch onto the end of the ring.

## Goals
- Make `cargo test` (default features) pass.
- Add comprehensive tests covering:
  - Borrowed strings within a single chunk
  - Strings with escapes forcing buffered fragments
  - Cross-batch borrowed string fragments
  - Drop semantics for strings and numbers
  - Ring→batch transitions for numbers and strings

## Status
- Implemented a virtual source: lex drains the ring first, then reads from the batch.
- Added `batch_read_chars` and `batch_owned_buffer` to track consumption and to accumulate owned fragments while reading from the batch.
- Introduced `LexToken::*Owned` variants for strings, numbers, and property names and routed parse events accordingly.
- Gated writes so `self.buffer` is only used when consuming from the ring; batch-mode lexing never mutates `self.buffer`.
- Stopped copying into `self.source` in `feed_with`; now `Drop` for the iterator appends any unread batch remainder into the ring.
- Fixed batch fast-path copy loops to avoid skipping/duplicating characters when advancing (`copy_*_while_*` now uses `.chars().skip(self.batch_read_chars)`).
- Ensured property-name escapes buffer the already-read prefix and produce an owned key under batch-mode.
- All default-feature tests pass locally (`cargo test`).

## Next Steps
- Push commits to `wip-codex-branch` (done).
- Monitor for edge cases: extremely long UTF-8 multi-byte chars across ring/batch boundary; verify number lexeme corner cases with leading zeros and exponent splits across batch.

## Update (2025-08-25 16:10:55Z)
- Added tests: empty string, unicode escape (single/cross-batch), exponents/sign numbers.
- Enhanced rustdoc in parser module for buffer roles, borrowing, and drop semantics.
- All default-feature tests: 18 passing.

## Update (2025-08-25 17:03:47Z)
- Attempted fully zero-copy feed path; reverted to ring-backed lexing for correctness and simplicity.
- Clarified docs on feed_str and borrowing guarantees.
- All tests passing (18).

## Update (2025-08-25 18:15:00Z)
- Implemented ring-first, then batch borrow path without feeding ring in `feed_with`.
- Added Owned token paths and ensured `self.buffer` is mutated only when reading from the ring.
- Iterator `Drop` now appends unread batch to the ring and preserves in-flight token prefix.
- Fixed off-by-one and iterator-skip bugs in batch copy loops; all tests (18) pass.

## Update (2025-08-26 00:00:00Z)
- Honored `ParserOptions::allow_unicode_whitespace` in the core lexer: by default only JSON's four whitespace chars are accepted; broader `char::is_whitespace()` is gated by the option.
- Added tests for multibyte strings (single chunk, cross-batch with drop) and multibyte property names to validate UTF-8 handling and borrowed/owned fragment behavior.
- Added tests for Unicode whitespace rejection by default and acceptance when enabled.
- Documented `current_token_buffered` semantics in-code to clarify why it cannot be replaced by `!self.source.is_empty()` (escapes and cross-batch tokens still force owned mode).
- All default-feature tests pass locally (`cargo test`): 29 passing in parser module, overall green.
