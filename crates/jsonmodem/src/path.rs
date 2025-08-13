use alloc::vec::Vec;

use crate::{Index, JsonPath, Key, PathComponent};

pub type Path = Vec<PathComponent>;

impl JsonPath for Path {
    type Key = Key;
    type Index = Index;

    fn push_key(&mut self, key: Self::Key) {
        self.push(PathComponent::Key(key));
    }

    fn push_index(&mut self, index: Self::Index) {
        self.push(PathComponent::Index(index));
    }

    fn pop(&mut self) {
        self.pop();
    }

    fn len(&self) -> usize {
        self.len()
    }

    fn is_empty(&self) -> bool {
        self.is_empty()
    }

    fn last(&self) -> Option<&PathComponent> {
        if self.is_empty() {
            None
        } else {
            Some(&self[self.len() - 1])
        }
    }

    fn iter(&self) -> impl Iterator<Item = &PathComponent<Self::Key, Self::Index>> {
        self.as_slice().iter()
    }
}

#[cfg(test)]
mod test {
    use std::vec::Vec;

    use super::*;
    use crate::{DefaultStreamingParser, ParseEvent, ParserOptions, path};

    #[test]
    fn custom_path_compiles() {
        let mut parser = DefaultStreamingParser::new(ParserOptions::default());
        parser.feed("{\"a\":[1]}");
        let events: Vec<_> = parser.finish().collect();
        assert!(events.iter().all(Result::is_ok));
    }

    #[test]
    fn primitive_path_formation() {
        let mut parser = DefaultStreamingParser::new(ParserOptions::default());
        parser.feed("{\"a\":[1]}");
        let events: Vec<_> = parser.finish().map(|e| e.unwrap()).collect();

        let paths: Vec<Path> = events
            .iter()
            .map(|ev| match ev {
                ParseEvent::ObjectBegin { path }
                | ParseEvent::ArrayStart { path }
                | ParseEvent::ArrayEnd { path, .. }
                | ParseEvent::ObjectEnd { path, .. }
                | ParseEvent::Number { path, .. } => path.clone(),
                _ => unreachable!(),
            })
            .collect();

        assert_eq!(paths[0], path![]);
        assert_eq!(paths[1], path!["a"]);
        assert_eq!(paths[2], path!["a", 0]);
        assert_eq!(paths[3], path!["a"]);
        assert_eq!(paths[4], path![]);
    }
}
