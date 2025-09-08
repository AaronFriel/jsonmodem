use alloc::string::String;

use super::{
    FacetApplyError, FacetOptions, FacetZipper,
    facet_api::{self, ContainerKind, NumberKind},
};
use crate::{
    backend::{StdBackend, StdPath},
    event::ParseEvent,
    path::PathItem,
};

pub const NUMBER_EXPECTATION_SIGNED: &str = "signed number";
pub const NUMBER_EXPECTATION_UNSIGNED: &str = "unsigned number";
pub const NUMBER_EXPECTATION_FLOAT: &str = "floating number";

/// Event surfaced to callers when a path is updated.
pub struct FacetEvent<Root> {
    /// Current parser path that was mutated.
    pub path: StdPath,
    view_ptr: facet_api::ErasedPtr,
    /// Shape classification describing the update that occurred.
    pub kind: FacetEventKind,
    root: *const Root,
}

impl<Root> FacetEvent<Root> {
    #[must_use]
    /// Returns an immutable view of the root value associated with this event.
    pub fn root(&self) -> &Root {
        unsafe { &*self.root }
    }

    #[must_use]
    /// Produces a reflected view into the updated slot.
    pub fn view(&self) -> facet_api::ValueRef<'_> {
        unsafe { facet_api::ValueRef::from_erased(self.view_ptr) }
    }
}

/// Classification for the type of facet change emitted during parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FacetEventKind {
    /// A scalar slot was written.
    Scalar {
        /// Name of the scalar type written to the slot.
        ty_name: &'static str,
    },
    /// A fragment of a string was appended.
    StringFragment {
        /// Whether this fragment represents the start of the string.
        is_initial: bool,
        /// Whether this fragment finishes the string payload.
        is_final: bool,
    },
    /// The beginning of a sequence container was observed.
    SeqBegin,
    /// The end of a sequence container was observed.
    SeqEnd {
        /// Indicates whether the root value finished at this boundary.
        root_completed: bool,
    },
    /// The beginning of a map container was observed.
    MapBegin,
    /// The end of a map container was observed.
    MapEnd {
        /// Indicates whether the root value finished at this boundary.
        root_completed: bool,
    },
}

pub struct FacetAssembler<'root, Root: facet_api::Facet> {
    pub(crate) root: &'root mut Root,
    options: FacetOptions,
    zipper: FacetZipper,
    scratch: String,
}

impl<'root, Root> FacetAssembler<'root, Root>
where
    Root: facet_api::Facet,
{
    pub fn new(root: &'root mut Root, options: FacetOptions) -> Self {
        Self {
            root,
            options,
            zipper: FacetZipper::new(),
            scratch: String::new(),
        }
    }

    pub fn options(&self) -> FacetOptions {
        self.options
    }

    pub fn on_event(
        &mut self,
        event: ParseEvent<'_, StdPath, StdBackend>,
    ) -> Result<Option<FacetEvent<Root>>, FacetApplyError> {
        let root_ptr: *const Root = core::ptr::from_mut(self.root).cast_const();
        match event {
            ParseEvent::ArrayBegin { path } => {
                let view = self
                    .zipper
                    .ensure_container(self.root, &path, ContainerKind::Seq)?;
                Ok(Some(make_event(
                    root_ptr,
                    path,
                    view,
                    FacetEventKind::SeqBegin,
                )))
            }
            ParseEvent::ArrayEnd { path } => {
                let view = self.zipper.value(self.root, &path)?;
                let root_completed = path.is_empty();
                Ok(Some(make_event(
                    root_ptr,
                    path,
                    view,
                    FacetEventKind::SeqEnd { root_completed },
                )))
            }
            ParseEvent::ObjectBegin { path } => {
                let view = self
                    .zipper
                    .ensure_container(self.root, &path, ContainerKind::Map)?;
                Ok(Some(make_event(
                    root_ptr,
                    path,
                    view,
                    FacetEventKind::MapBegin,
                )))
            }
            ParseEvent::ObjectEnd { path } => {
                let view = self.zipper.value(self.root, &path)?;
                let root_completed = path.is_empty();
                Ok(Some(make_event(
                    root_ptr,
                    path,
                    view,
                    FacetEventKind::MapEnd { root_completed },
                )))
            }
            ParseEvent::Null { path } => {
                let mut slot = self.zipper.align(self.root, &path)?;
                let found = slot.ty_name();
                slot.write_null().map_err(|detail| {
                    FacetApplyError::new(path.as_slice(), "null", found, detail)
                })?;
                let view = slot.into_erased();
                Ok(Some(make_event(
                    root_ptr,
                    path,
                    view,
                    FacetEventKind::Scalar { ty_name: "null" },
                )))
            }
            ParseEvent::Boolean { path, value } => {
                let mut slot = self.zipper.align(self.root, &path)?;
                let found = slot.ty_name();
                slot.write_bool(value).map_err(|detail| {
                    FacetApplyError::new(path.as_slice(), "bool", found, detail)
                })?;
                let view = slot.into_erased();
                Ok(Some(make_event(
                    root_ptr,
                    path,
                    view,
                    FacetEventKind::Scalar { ty_name: "bool" },
                )))
            }
            ParseEvent::Number { path, value } => {
                let mut slot = self.zipper.align(self.root, &path)?;
                coerce_number(&mut slot, value, &self.options, path.as_slice())?;
                let view = slot.into_erased();
                Ok(Some(make_event(
                    root_ptr,
                    path,
                    view,
                    FacetEventKind::Scalar { ty_name: "number" },
                )))
            }
            ParseEvent::String {
                path,
                fragment,
                is_initial,
                is_final,
            } => self.handle_string(root_ptr, path, fragment.as_ref(), is_initial, is_final),
        }
    }

    fn handle_string(
        &mut self,
        root_ptr: *const Root,
        path: StdPath,
        fragment: &str,
        is_initial: bool,
        is_final: bool,
    ) -> Result<Option<FacetEvent<Root>>, FacetApplyError> {
        let path_ref = &path;
        if self.options.partial_strings {
            let mut slot = self.zipper.align(self.root, path_ref)?;
            if is_initial {
                slot.write_str_final("").map_err(|detail| {
                    FacetApplyError::new(path_ref.as_slice(), "string", slot.ty_name(), detail)
                })?;
            }
            let ty = slot.ty_name();
            slot.string_push_str(fragment).map_err(|detail| {
                FacetApplyError::new(path_ref.as_slice(), "string", ty, detail)
            })?;
            let view = slot.into_erased();
            Ok(Some(make_event(
                root_ptr,
                path,
                view,
                FacetEventKind::StringFragment {
                    is_initial,
                    is_final,
                },
            )))
        } else {
            if is_initial {
                self.scratch.clear();
            }
            self.scratch.push_str(fragment);
            if !is_final {
                return Ok(None);
            }
            let mut slot = self.zipper.align(self.root, path_ref)?;
            let ty = slot.ty_name();
            slot.write_str_final(&self.scratch).map_err(|detail| {
                FacetApplyError::new(path_ref.as_slice(), "string", ty, detail)
            })?;
            self.scratch.clear();
            let view = slot.into_erased();
            Ok(Some(make_event(
                root_ptr,
                path,
                view,
                FacetEventKind::Scalar { ty_name: "string" },
            )))
        }
    }
}

fn make_event<Root>(
    root_ptr: *const Root,
    path: StdPath,
    view: facet_api::ErasedPtr,
    kind: FacetEventKind,
) -> FacetEvent<Root> {
    FacetEvent {
        path,
        view_ptr: view,
        kind,
        root: root_ptr,
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)]
pub fn coerce_number(
    slot: &mut facet_api::PokeTarget<'_>,
    value: f64,
    options: &FacetOptions,
    path: &[PathItem],
) -> Result<(), FacetApplyError> {
    match slot.number_kind() {
        Some(NumberKind::Float) => write_float(slot, value, path),
        Some(NumberKind::Signed) => write_signed(slot, value, options, path),
        Some(NumberKind::Unsigned) => write_unsigned(slot, value, options, path),
        None => {
            if slot.write_f64(value).is_ok() || write_signed(slot, value, options, path).is_ok() {
                Ok(())
            } else {
                write_unsigned(slot, value, options, path)
            }
        }
    }
}

fn write_float(
    slot: &mut facet_api::PokeTarget<'_>,
    value: f64,
    path: &[PathItem],
) -> Result<(), FacetApplyError> {
    let ty = slot.ty_name();
    slot.write_f64(value)
        .map_err(|detail| FacetApplyError::new(path, NUMBER_EXPECTATION_FLOAT, ty, detail))
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn write_signed(
    slot: &mut facet_api::PokeTarget<'_>,
    value: f64,
    options: &FacetOptions,
    path: &[PathItem],
) -> Result<(), FacetApplyError> {
    let Some(int_value) = convert_signed(value, options.allow_coerce_numbers) else {
        let ty = slot.ty_name();
        return Err(FacetApplyError::new(
            path,
            NUMBER_EXPECTATION_SIGNED,
            ty,
            "value does not fit signed integer",
        ));
    };
    let ty = slot.ty_name();
    slot.write_i64(int_value)
        .map_err(|detail| FacetApplyError::new(path, NUMBER_EXPECTATION_SIGNED, ty, detail))
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn write_unsigned(
    slot: &mut facet_api::PokeTarget<'_>,
    value: f64,
    options: &FacetOptions,
    path: &[PathItem],
) -> Result<(), FacetApplyError> {
    let Some(int_value) = convert_unsigned(value, options.allow_coerce_numbers) else {
        let ty = slot.ty_name();
        return Err(FacetApplyError::new(
            path,
            NUMBER_EXPECTATION_UNSIGNED,
            ty,
            "value does not fit unsigned integer",
        ));
    };
    let ty = slot.ty_name();
    slot.write_u64(int_value)
        .map_err(|detail| FacetApplyError::new(path, NUMBER_EXPECTATION_UNSIGNED, ty, detail))
}

#[allow(
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::checked_conversions
)]
fn convert_signed(value: f64, allow_coerce: bool) -> Option<i64> {
    if value.is_nan() || value.is_infinite() {
        return None;
    }
    if value.fract() == 0.0 {
        let rounded = value as i128;
        if rounded >= i64::MIN as i128 && rounded <= i64::MAX as i128 {
            return Some(rounded as i64);
        }
    }
    if allow_coerce {
        let truncated = value.trunc();
        if truncated.fract() == 0.0 {
            let rounded = truncated as i128;
            if rounded >= i64::MIN as i128 && rounded <= i64::MAX as i128 {
                return Some(rounded as i64);
            }
        }
    }
    None
}

#[allow(
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::checked_conversions
)]
fn convert_unsigned(value: f64, allow_coerce: bool) -> Option<u64> {
    if !(value.is_finite()) || value < 0.0 {
        return None;
    }
    if value.fract() == 0.0 {
        let rounded = value as u128;
        if rounded <= u64::MAX as u128 {
            return Some(rounded as u64);
        }
    }
    if allow_coerce {
        let truncated = value.trunc();
        if truncated >= 0.0 && truncated.fract() == 0.0 {
            let rounded = truncated as u128;
            if rounded <= u64::MAX as u128 {
                return Some(rounded as u64);
            }
        }
    }
    None
}
