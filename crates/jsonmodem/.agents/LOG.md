# JSONModem Parser Refactor Log

Branch: wip-codex-branch

## Current Task
- Complete the refactor in `src/parser/mod.rs` to support borrow-first parse events via a batch view, fixing compile errors and ensuring default-feature tests pass.
- Implement buffering vs borrowing rules:
  - If `self.buffer` has content, emit owned string/number events.
  - Otherwise, return borrowed fragments from the current feed batch when safe.
  - For strings, encountering an escape emits a partial fragment, then switches to buffered mode for the rest of that string.
  - For numbers that can’t be borrowed (cross-batch), fall back to buffer.
- Ensure iterator `Drop` copies any in-flight batch fragment into the parser’s buffer and switches to buffered mode.

## Goals
- Make `cargo test` (default features) pass.
- Add comprehensive tests covering:
  - Borrowed strings within a single chunk
  - Strings with escapes forcing buffered fragments
  - Cross-batch borrowed string fragments
  - Drop semantics for strings and numbers

## Status
- Implemented `BatchView` borrow slicing and integrated into lexer.
- Fixed overlapping mutable borrow in string escape path.
- Rearm `token_start_pos` after partial string fragments.
- Iterator `Drop` now copies current batch fragment (if any) into buffer and switches to buffered mode.
- Added tests for borrow vs buffer and drop semantics.
- All default-feature tests pass locally (`cargo test`).

## Next Steps
- Push commits to `wip-codex-branch`.
- Continue iterating if additional edge cases arise in CI/remote.

## Update (2025-08-25 16:10:55Z)
- Added tests: empty string, unicode escape (single/cross-batch), exponents/sign numbers.
- Enhanced rustdoc in parser module for buffer roles, borrowing, and drop semantics.
- All default-feature tests: 18 passing.
