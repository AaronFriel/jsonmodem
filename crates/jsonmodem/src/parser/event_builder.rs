use alloc::{string::String, sync::Arc, vec::Vec};
use core::fmt::Debug;

use crate::parser::{Path, PathLike};

#[allow(dead_code)]
pub trait EventBuilder: Clone + Debug {
    // Scalar atoms carried by events
    type Str: Clone + Debug;
    type Num: Clone + Debug;
    type Bool: Clone + Debug;
    type Null: Clone + Debug;

    // Path atoms and final public path type
    type Key: Clone + Debug;
    type Index: Clone + Debug;
    type Path: Clone + Debug + PathLike<Self::Key, Self::Index>; // e.g., Vec<PathComponent> or Py<PyTuple>

    /// Creates a new null value.
    fn new_null(&mut self) -> Self::Null;

    /// Creates a new boolean from a value.
    fn new_bool(&mut self, b: bool) -> Self::Bool;

    /// Creates a new number from a lexeme.
    fn new_number(&mut self, lex: String) -> Self::Num;

    /// Creates a new string fragment.
    fn new_string_fragment(&mut self, s: &str) -> Self::Str;

    /// Creates a new path.
    fn new_path(&mut self) -> Self::Path;

    /// Creates a new key from a string.
    fn make_key(&mut self, s: String) -> Self::Key;

    /// Returns the zero index.
    fn index_zero(&mut self) -> Self::Index;

    /// Returns the next index after the given one.
    fn index_next(&mut self, prev: &Self::Index) -> Self::Index;

    /// Clones a path. If the factory provides a faster implementation, this
    /// should be overridden.
    fn clone_path(&mut self, path: &Self::Path) -> Self::Path {
        path.clone()
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub struct StdFactory;

impl EventBuilder for StdFactory {
    type Str = String;
    type Num = f64;
    type Bool = bool;
    type Null = ();

    type Key = Arc<str>;
    type Index = usize;
    type Path = Path;

    fn new_null(&mut self) {}
    fn new_bool(&mut self, b: bool) -> bool {
        b
    }
    fn new_number(&mut self, lex: String) -> f64 {
        // TODO: remove unwrap
        lex.parse().unwrap()
    }
    fn new_string_fragment(&mut self, s: &str) -> String {
        s.into()
    }

    fn new_path(&mut self) -> Self::Path {
        Vec::new()
    }
    fn make_key(&mut self, s: String) -> Arc<str> {
        s.into()
    }
    fn index_zero(&mut self) -> usize {
        0
    }
    fn index_next(&mut self, p: &usize) -> usize {
        *p + 1
    }
}
