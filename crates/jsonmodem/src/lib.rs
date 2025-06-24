//! A faithful 1‑for‑1 Rust port of the incremental / online JSON parser
//! originally written in TypeScript (see source file in the prompt).

#![no_std]
#![allow(missing_docs)]
extern crate alloc;

#[cfg(test)]
extern crate std;

mod buffer;
mod escape_buffer;
mod event;
mod literal_buffer;
mod value;
mod value_zipper;

mod error;
mod event_stack;
mod options;
mod parser;

pub use error::ParserError;
pub use event::{ParseEvent, PathComponent};
pub use options::ParserOptions;
pub use parser::StreamingParser;
pub use value::{Array, Map, Value};

#[cfg(test)]
mod tests;
