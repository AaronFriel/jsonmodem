mod assembler;
pub mod facet_api;
mod zipper;

use alloc::string::String;

#[cfg(test)]
#[allow(unused_imports)]
pub use assembler::NUMBER_EXPECTATION_UNSIGNED;
pub use assembler::{FacetAssembler, FacetEvent, FacetEventKind};

use crate::path::{Path, PathItem};

/// Behaviour when encountering unknown fields during object updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnknownFieldPolicy {
    Ignore,
    Error,
    InsertDynamic,
}

/// Growth behaviour for collection updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectionGrowthPolicy {
    Auto,
    Error,
}

/// Configuration options accepted by the facet adapter.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct FacetOptions {
    /// Emit string updates as fragments rather than waiting for the final
    /// chunk.
    pub partial_strings: bool,
    /// Permit lossy truncating conversions when assigning numeric values.
    pub allow_coerce_numbers: bool,
    /// Behaviour when encountering object keys that are not present on the
    /// target.
    pub unknown_field: UnknownFieldPolicy,
    /// Strategy used when vectors or maps need to grow to accommodate new
    /// entries.
    pub collection_growth: CollectionGrowthPolicy,
}

impl Default for FacetOptions {
    fn default() -> Self {
        Self {
            partial_strings: true,
            allow_coerce_numbers: false,
            unknown_field: UnknownFieldPolicy::Error,
            collection_growth: CollectionGrowthPolicy::Auto,
        }
    }
}

/// Detailed description of an application failure.
#[derive(Debug, Clone)]
pub struct FacetApplyError {
    /// Path within the root value identifying the failing slot.
    pub path: Path,
    /// Description of the value shape expected by the target slot.
    pub expected: &'static str,
    /// Actual type exposed by the slot during the attempted write.
    pub found: &'static str,
    /// Additional context about the failure.
    pub detail: String,
}

impl FacetApplyError {
    #[must_use]
    /// Constructs a new [`FacetApplyError`] describing a failed write.
    pub fn new(
        path: &[PathItem],
        expected: &'static str,
        found: &'static str,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            path: path.to_vec(),
            expected,
            found,
            detail: detail.into(),
        }
    }
}

pub(crate) use zipper::FacetZipper;
