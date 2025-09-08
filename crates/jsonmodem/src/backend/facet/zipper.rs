use alloc::vec::Vec;

use super::{
    FacetApplyError,
    facet_api::{self, ContainerKind},
};
use crate::{backend::StdPath, path::PathItem};

/// Maintains a stack of pointers aligned with the parser path.
#[derive(Default)]
pub struct FacetZipper {
    root: Option<facet_api::ErasedPtr>,
    nodes: Vec<facet_api::ErasedPtr>,
    components: Vec<PathItem>,
}

impl FacetZipper {
    pub fn new() -> Self {
        Self::default()
    }

    fn ensure_root<Root: facet_api::Facet>(&mut self, root: &mut Root) -> facet_api::ErasedPtr {
        if let Some(ptr) = self.root {
            ptr
        } else {
            let ptr = root.erased_ptr();
            self.root = Some(ptr);
            ptr
        }
    }

    fn current_ptr(&self, root: facet_api::ErasedPtr) -> facet_api::ErasedPtr {
        self.nodes.last().copied().unwrap_or(root)
    }

    fn descend_into(
        parent: facet_api::ErasedPtr,
        component: &PathItem,
        full_path: &StdPath,
    ) -> Result<facet_api::ErasedPtr, FacetApplyError> {
        let parent_ty = unsafe { parent.as_ref().ty_name() };
        let mut target = unsafe { facet_api::PokeTarget::from_erased(parent) };
        let result = match component {
            PathItem::Key(key) => target.key(key.as_ref()),
            PathItem::Index(idx) => target.index(*idx),
        };
        match result {
            Ok(child) => Ok(child.into_erased()),
            Err(detail) => Err(FacetApplyError::new(
                full_path.as_slice(),
                match component {
                    PathItem::Key(_) => "object",
                    PathItem::Index(_) => "array",
                },
                parent_ty,
                detail,
            )),
        }
    }

    fn align_internal<Root: facet_api::Facet>(
        &mut self,
        root_ref: &mut Root,
        path: &StdPath,
    ) -> Result<facet_api::ErasedPtr, FacetApplyError> {
        let root_ptr = self.ensure_root(root_ref);
        let current_depth = self.components.len();
        let target_depth = path.len();

        match target_depth.cmp(&current_depth) {
            core::cmp::Ordering::Greater => {
                debug_assert_eq!(target_depth, current_depth + 1);
                let component = path
                    .last()
                    .expect("path depth greater than current depth implies element");
                let parent_ptr = self.current_ptr(root_ptr);
                let child_ptr = Self::descend_into(parent_ptr, component, path)?;
                self.nodes.push(child_ptr);
                self.components.push(component.clone());
            }
            core::cmp::Ordering::Less => {
                debug_assert_eq!(current_depth, target_depth + 1);
                self.nodes.truncate(target_depth);
                self.components.truncate(target_depth);
            }
            core::cmp::Ordering::Equal => {
                if target_depth == 0 {
                    // Already aligned at root.
                } else if let Some(last) = path.last() {
                    let matches = self.components.last() == Some(last);
                    if !matches {
                        self.nodes.pop();
                        self.components.pop();
                        let parent_ptr = self.current_ptr(root_ptr);
                        let child_ptr = Self::descend_into(parent_ptr, last, path)?;
                        self.nodes.push(child_ptr);
                        self.components.push(last.clone());
                    }
                }
            }
        }

        let slot_ptr = if path.is_empty() {
            root_ptr
        } else {
            *self.nodes.last().expect("non-empty path has node entry")
        };

        Ok(slot_ptr)
    }

    pub fn align<'root, Root: facet_api::Facet>(
        &mut self,
        root: &'root mut Root,
        path: &StdPath,
    ) -> Result<facet_api::PokeTarget<'root>, FacetApplyError> {
        let ptr = self.align_internal(root, path)?;
        // SAFETY: the pointer originates from the root borrow, which lives for `'root`.
        Ok(unsafe { facet_api::PokeTarget::from_erased(ptr) })
    }

    pub fn ensure_container<Root: facet_api::Facet>(
        &mut self,
        root: &mut Root,
        path: &StdPath,
        kind: ContainerKind,
    ) -> Result<facet_api::ErasedPtr, FacetApplyError> {
        let mut target = self.align(root, path)?;
        let ty = target.ty_name();
        target
            .ensure_container(kind)
            .map_err(|detail| FacetApplyError::new(path.as_slice(), "container", ty, detail))?;
        Ok(target.into_erased())
    }

    pub fn value<Root: facet_api::Facet>(
        &mut self,
        root: &mut Root,
        path: &StdPath,
    ) -> Result<facet_api::ErasedPtr, FacetApplyError> {
        let target = self.align(root, path)?;
        Ok(target.into_erased())
    }
}
