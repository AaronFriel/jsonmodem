use alloc::{borrow::Cow, string::String, vec::Vec};

use crate::{backend::{EventCtx, PathCtx}, PathItem};

#[derive(Debug, Default, PartialEq, Clone)]
pub struct RawContext;

impl PathCtx for RawContext {
    type Frozen = Vec<PathItem<Vec<u8>, usize>>;
    type Thawed = Vec<PathItem<Vec<u8>, usize>>;

    fn frozen_new(&mut self) -> Self::Frozen {
        Vec::new()
    }
    fn thaw(&mut self, frozen: Self::Frozen) -> Self::Thawed {
        frozen
    }
    fn freeze(&mut self, thawed: Self::Thawed) -> Self::Frozen {
        thawed
    }

    fn push_key_from_str(&mut self, t: &mut Self::Thawed, key: &str) {
        t.push(PathItem::Key(key.into()));
    }
    fn push_index_zero(&mut self, t: &mut Self::Thawed) {
        t.push(PathItem::Index(0));
    }
    fn bump_last_index(&mut self, t: &mut Self::Thawed) -> Result<(), super::PathError> {
        let Some(PathItem::Index(i)) = t.last_mut() else {
            return Err(super::PathError::NotArrayFrame);
        };
        *i += 1;
        Ok(())
    }
    fn pop_kind(&mut self, t: &mut Self::Thawed) -> Option<super::PathKind> {
        t.pop().map(|item| match item {
            PathItem::Key(_) => super::PathKind::Key,
            PathItem::Index(_) => super::PathKind::Index,
        })
    }
    fn last_kind(&self, t: &Self::Thawed) -> Option<super::PathKind> {
        t.last().map(|item| match item {
            PathItem::Key(_) => super::PathKind::Key,
            PathItem::Index(_) => super::PathKind::Index,
        })
    }
}

impl EventCtx for RawContext {
    type Null = ();
    type Bool = bool;
    type Num<'src> = f64;
    type Str<'src> = Cow<'src, [u8]>;
    type Error = core::num::ParseFloatError;

    fn push_key_from_raw_str(&mut self, t: &mut Self::Thawed, key: &[u8]) {
        t.push(PathItem::Key(key.into()));
    }

    fn new_null(&mut self) -> Result<Self::Null, Self::Error> {
        Ok(())
    }
    fn new_bool(&mut self, b: bool) -> Result<Self::Bool, Self::Error> {
        Ok(b)
    }

    fn new_number<'src>(&mut self, n: &'src str) -> Result<Self::Num<'src>, Self::Error> {
        n.parse()
    }
    fn new_number_owned<'a>(&mut self, n: String) -> Result<Self::Num<'a>, Self::Error> {
        n.parse()
    }

    fn new_str<'src>(&mut self, frag: &'src str) -> Result<Self::Str<'src>, Self::Error> {
        Ok(Cow::Borrowed(frag.as_bytes()))
    }
    fn new_str_owned<'a>(&mut self, frag: String) -> Result<Self::Str<'a>, Self::Error> {
        Ok(Cow::Owned(frag.into_bytes()))
    }

    fn new_str_raw_owned<'a>(
        &mut self,
        bytes: Vec<u8>,
    ) -> Result<Self::Str<'a>, Self::Error> {
        Ok(Cow::Owned(bytes))
    }
}
