//! Integration tests for `JsonModemValues`.
//! These snapshot-style tests are disabled under Miri because `insta`
//! tries to interact with the workspace and uses `open(2)` under the hood.

#[cfg(not(miri))]
mod tests;
