use alloc::{borrow::Cow, string::String, vec::Vec};
use core::num::ParseFloatError;

use crate::{
    PathItem,
    backend::{EventCtx, PathCtx, RawStrHint},
};

#[derive(Debug, Default, PartialEq, Clone)]
pub struct RustContext;

impl PathCtx for RustContext {
    type Frozen = Vec<PathItem>;
    type Thawed = Vec<PathItem>;

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

impl EventCtx for RustContext {
    type Null = ();
    type Bool = bool;
    type Num<'src> = f64;
    type Str<'src> = Cow<'src, str>;
    type Error = ParseFloatError;

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
        Ok(Cow::Borrowed(frag))
    }

    fn new_str_owned<'a>(&mut self, frag: String) -> Result<Self::Str<'a>, Self::Error> {
        Ok(Cow::Owned(frag))
    }

    fn new_str_raw_owned<'a>(
        &mut self,
        bytes: Vec<u8>,
        _hint: RawStrHint,
    ) -> Result<Self::Str<'a>, Self::Error> {
        // Default Rust backend is UTF-8-only. Decode raw bytes lossily,
        // replacing invalid sequences. Special-case WTF-8 surrogate 3-byte
        // sequences (ED A0..BF 80..BF) to collapse to a single U+FFFD.
        let mut norm = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            if i + 2 < bytes.len()
                && bytes[i] == 0xED
                && (bytes[i + 1] >= 0xA0 && bytes[i + 1] <= 0xBF)
                && (bytes[i + 2] & 0xC0) == 0x80
            {
                // One surrogate code unit in WTF-8 – normalize to U+FFFD (EF BF BD)
                norm.extend_from_slice(&[0xEF, 0xBF, 0xBD]);
                i += 3;
            } else {
                norm.push(bytes[i]);
                i += 1;
            }
        }
        let owned = match String::from_utf8(norm) {
            Ok(s) => s,
            Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
        };
        Ok(Cow::Owned(owned))
    }
}
