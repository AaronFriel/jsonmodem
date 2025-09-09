use alloc::{borrow::ToOwned, collections::BTreeMap, string::String, vec::Vec};
use core::any::Any;

/// Minimal container kinds understood by the facet adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerKind {
    Seq,
    Map,
    Struct,
    Tuple,
}

/// Numeric flavour preferred by the underlying slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumberKind {
    Signed,
    Unsigned,
    Float,
}

/// Erased pointer to a value participating in facet reflection.
#[derive(Clone, Copy)]
pub struct ErasedPtr {
    raw: *mut dyn ValueDyn,
}

impl ErasedPtr {
    /// Erases a mutable pointer into the opaque representation used by the
    /// zipper.
    pub fn new<T: ValueDyn + 'static>(value: &mut T) -> Self {
        let raw = core::ptr::from_mut::<T>(value) as *mut dyn ValueDyn;
        Self { raw }
    }

    /// Reborrow the pointer mutably.
    ///
    /// # Safety
    ///
    /// Callers must ensure the referent outlives the borrow and that no other
    /// mutable references exist while the borrow is alive.
    pub unsafe fn as_mut<'a>(self) -> &'a mut dyn ValueDyn {
        unsafe { &mut *self.raw }
    }

    /// Reborrow the pointer immutably.
    ///
    /// # Safety
    ///
    /// Same preconditions as [`ErasedPtr::as_mut`].
    pub unsafe fn as_ref<'a>(self) -> &'a dyn ValueDyn {
        unsafe { &*self.raw }
    }
}

/// Immutable view of a reflected value.
#[derive(Clone, Copy)]
pub struct ValueRef<'a> {
    inner: &'a dyn ValueDyn,
}

impl<'a> ValueRef<'a> {
    #[must_use]
    pub fn ty_name(&self) -> &'static str {
        self.inner.ty_name()
    }

    #[must_use]
    pub fn downcast_ref<T: 'static>(&self) -> Option<&'a T> {
        self.inner.as_any().downcast_ref::<T>()
    }

    /// Constructs a value reference from an erased pointer.
    ///
    /// # Safety
    ///
    /// The pointer must remain valid for the lifetime `'a`.
    pub unsafe fn from_erased(ptr: ErasedPtr) -> Self {
        Self {
            inner: unsafe { ptr.as_ref() },
        }
    }
}

/// Writable handle to a slot within the reflected value.
pub struct PokeTarget<'a> {
    value: &'a mut dyn ValueDyn,
}

impl<'a> PokeTarget<'a> {
    /// Reconstructs a target from an erased pointer.
    ///
    /// # Safety
    ///
    /// The pointer must reference a live value for the duration of `'a`.
    pub unsafe fn from_erased(ptr: ErasedPtr) -> Self {
        Self {
            value: unsafe { ptr.as_mut() },
        }
    }

    #[must_use]
    pub fn ty_name(&self) -> &'static str {
        self.value.ty_name()
    }

    #[must_use]
    pub fn number_kind(&self) -> Option<NumberKind> {
        self.value.number_kind()
    }

    #[must_use]
    pub fn into_erased(self) -> ErasedPtr {
        let raw = self.value as *mut dyn ValueDyn;
        ErasedPtr { raw }
    }

    pub fn write_null(&mut self) -> Result<(), &'static str> {
        self.value.write_null()
    }

    pub fn write_bool(&mut self, value: bool) -> Result<(), &'static str> {
        self.value.write_bool(value)
    }

    pub fn write_i64(&mut self, value: i64) -> Result<(), &'static str> {
        self.value.write_i64(value)
    }

    pub fn write_u64(&mut self, value: u64) -> Result<(), &'static str> {
        self.value.write_u64(value)
    }

    pub fn write_f64(&mut self, value: f64) -> Result<(), &'static str> {
        self.value.write_f64(value)
    }

    pub fn write_str_final(&mut self, value: &str) -> Result<(), &'static str> {
        self.value.write_str_final(value)
    }

    pub fn string_push_str(&mut self, fragment: &str) -> Result<(), &'static str> {
        self.value.string_push_str(fragment)
    }

    pub fn ensure_container(&mut self, kind: ContainerKind) -> Result<(), &'static str> {
        self.value.ensure_container(kind)
    }

    pub fn key(&mut self, name: &str) -> Result<PokeTarget<'_>, &'static str> {
        let child = self.value.key_erased(name)?;
        Ok(unsafe { PokeTarget::from_erased(child) })
    }

    pub fn index(&mut self, idx: usize) -> Result<PokeTarget<'_>, &'static str> {
        let child = self.value.index_erased(idx)?;
        Ok(unsafe { PokeTarget::from_erased(child) })
    }

    #[allow(dead_code)]
    pub fn to_view(&self) -> ValueRef<'a> {
        let raw = self.value as *const dyn ValueDyn;
        let inner = unsafe { &*raw };
        ValueRef { inner }
    }
}

/// Types that can be used as facet reflection roots.
pub trait Facet {
    fn erased_ptr(&mut self) -> ErasedPtr;
}

impl<T> Facet for T
where
    T: ValueDyn + 'static,
{
    fn erased_ptr(&mut self) -> ErasedPtr {
        ErasedPtr::new(self)
    }
}

/// Core trait implemented by values supporting the minimal facet operations.
pub trait ValueDyn: Any {
    fn ty_name(&self) -> &'static str;

    fn as_any(&self) -> &dyn Any;

    fn as_any_mut(&mut self) -> &mut dyn Any;

    fn write_null(&mut self) -> Result<(), &'static str> {
        let _ = self;
        Err("null not supported")
    }

    fn write_bool(&mut self, _value: bool) -> Result<(), &'static str> {
        Err("bool not supported")
    }

    fn write_i64(&mut self, _value: i64) -> Result<(), &'static str> {
        Err("signed integer not supported")
    }

    fn write_u64(&mut self, _value: u64) -> Result<(), &'static str> {
        Err("unsigned integer not supported")
    }

    fn write_f64(&mut self, _value: f64) -> Result<(), &'static str> {
        Err("float not supported")
    }

    fn write_str_final(&mut self, _value: &str) -> Result<(), &'static str> {
        Err("string assignment not supported")
    }

    fn string_push_str(&mut self, _fragment: &str) -> Result<(), &'static str> {
        Err("string fragments not supported")
    }

    fn ensure_container(&mut self, kind: ContainerKind) -> Result<(), &'static str> {
        let _ = kind;
        Err("not a container")
    }

    fn key_erased(&mut self, _name: &str) -> Result<ErasedPtr, &'static str> {
        Err("not a map or struct")
    }

    fn index_erased(&mut self, _idx: usize) -> Result<ErasedPtr, &'static str> {
        Err("not a sequence")
    }

    fn number_kind(&self) -> Option<NumberKind> {
        None
    }

    fn to_view(&self) -> ValueRef<'_>
    where
        Self: Sized,
    {
        ValueRef { inner: self }
    }
}

macro_rules! impl_numeric_signed {
    ($($ty:ty),* $(,)?) => {
        $(
            #[allow(
                clippy::cast_lossless,
                clippy::cast_possible_truncation,
                clippy::cast_possible_wrap,
                clippy::cast_precision_loss,
                clippy::cast_sign_loss,
                clippy::checked_conversions,
                clippy::float_arithmetic
            )]
            impl ValueDyn for $ty {
                fn ty_name(&self) -> &'static str {
                    core::any::type_name::<$ty>()
                }

                fn as_any(&self) -> &dyn Any {
                    self
                }

                fn as_any_mut(&mut self) -> &mut dyn Any {
                    self
                }

                fn write_i64(&mut self, value: i64) -> Result<(), &'static str> {
                    if value >= <$ty>::MIN as i64 && value <= <$ty>::MAX as i64 {
                        *self = value as $ty;
                        Ok(())
                    } else {
                        Err("signed overflow")
                    }
                }

                fn write_u64(&mut self, value: u64) -> Result<(), &'static str> {
                    if value <= <$ty>::MAX as u64 {
                        *self = value as $ty;
                        Ok(())
                    } else {
                        Err("unsigned overflow")
                    }
                }

                fn write_f64(&mut self, value: f64) -> Result<(), &'static str> {
                    if value.is_finite() && value % 1.0 == 0.0 {
                        self.write_i64(value as i64)
                    } else {
                        Err("float does not fit signed")
                    }
                }

                fn number_kind(&self) -> Option<NumberKind> {
                    Some(NumberKind::Signed)
                }
            }
        )*
    };
}

impl_numeric_signed!(i8, i16, i32, i64, isize);

macro_rules! impl_numeric_unsigned {
    ($($ty:ty),* $(,)?) => {
        $(
            #[allow(
                clippy::cast_lossless,
                clippy::cast_possible_truncation,
                clippy::cast_possible_wrap,
                clippy::cast_precision_loss,
                clippy::cast_sign_loss,
                clippy::checked_conversions,
                clippy::float_arithmetic
            )]
            impl ValueDyn for $ty {
                fn ty_name(&self) -> &'static str {
                    core::any::type_name::<$ty>()
                }

                fn as_any(&self) -> &dyn Any {
                    self
                }

                fn as_any_mut(&mut self) -> &mut dyn Any {
                    self
                }

                fn write_u64(&mut self, value: u64) -> Result<(), &'static str> {
                    if value <= <$ty>::MAX as u64 {
                        *self = value as $ty;
                        Ok(())
                    } else {
                        Err("unsigned overflow")
                    }
                }

                fn write_i64(&mut self, value: i64) -> Result<(), &'static str> {
                    if value >= 0 && value <= <$ty>::MAX as i64 {
                        *self = value as $ty;
                        Ok(())
                    } else {
                        Err("signed overflow")
                    }
                }

                fn write_f64(&mut self, value: f64) -> Result<(), &'static str> {
                    if value.is_finite() && value % 1.0 == 0.0 && value >= 0.0 {
                        self.write_u64(value as u64)
                    } else {
                        Err("float does not fit unsigned")
                    }
                }

                fn number_kind(&self) -> Option<NumberKind> {
                    Some(NumberKind::Unsigned)
                }
            }
        )*
    };
}

impl_numeric_unsigned!(u8, u16, u32, u64, usize);

impl ValueDyn for f32 {
    fn ty_name(&self) -> &'static str {
        "f32"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    #[allow(clippy::cast_possible_truncation)]
    fn write_f64(&mut self, value: f64) -> Result<(), &'static str> {
        if value.is_finite() {
            *self = value as f32;
            Ok(())
        } else {
            Err("non-finite float")
        }
    }

    fn number_kind(&self) -> Option<NumberKind> {
        Some(NumberKind::Float)
    }
}

impl ValueDyn for f64 {
    fn ty_name(&self) -> &'static str {
        "f64"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn write_f64(&mut self, value: f64) -> Result<(), &'static str> {
        if value.is_finite() {
            *self = value;
            Ok(())
        } else {
            Err("non-finite float")
        }
    }

    fn number_kind(&self) -> Option<NumberKind> {
        Some(NumberKind::Float)
    }
}

impl ValueDyn for bool {
    fn ty_name(&self) -> &'static str {
        "bool"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn write_bool(&mut self, value: bool) -> Result<(), &'static str> {
        *self = value;
        Ok(())
    }
}

impl ValueDyn for String {
    fn ty_name(&self) -> &'static str {
        "String"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn write_str_final(&mut self, value: &str) -> Result<(), &'static str> {
        self.clear();
        self.push_str(value);
        Ok(())
    }

    fn string_push_str(&mut self, fragment: &str) -> Result<(), &'static str> {
        self.push_str(fragment);
        Ok(())
    }
}

impl<T> ValueDyn for Vec<T>
where
    T: ValueDyn + Default + 'static,
{
    fn ty_name(&self) -> &'static str {
        "Vec"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn ensure_container(&mut self, kind: ContainerKind) -> Result<(), &'static str> {
        match kind {
            ContainerKind::Seq => Ok(()),
            _ => Err("vector expects sequence container"),
        }
    }

    fn index_erased(&mut self, idx: usize) -> Result<ErasedPtr, &'static str> {
        if idx >= self.len() {
            self.resize_with(idx + 1, T::default);
        }
        Ok(ErasedPtr::new(&mut self[idx]))
    }
}

impl<T> ValueDyn for Option<T>
where
    T: ValueDyn + Default + 'static,
{
    fn ty_name(&self) -> &'static str {
        "Option"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn write_null(&mut self) -> Result<(), &'static str> {
        *self = None;
        Ok(())
    }

    fn write_bool(&mut self, value: bool) -> Result<(), &'static str> {
        ensure_option_inner(self).write_bool(value)
    }

    fn write_i64(&mut self, value: i64) -> Result<(), &'static str> {
        ensure_option_inner(self).write_i64(value)
    }

    fn write_u64(&mut self, value: u64) -> Result<(), &'static str> {
        ensure_option_inner(self).write_u64(value)
    }

    fn write_f64(&mut self, value: f64) -> Result<(), &'static str> {
        ensure_option_inner(self).write_f64(value)
    }

    fn write_str_final(&mut self, value: &str) -> Result<(), &'static str> {
        ensure_option_inner(self).write_str_final(value)
    }

    fn string_push_str(&mut self, fragment: &str) -> Result<(), &'static str> {
        ensure_option_inner(self).string_push_str(fragment)
    }

    fn ensure_container(&mut self, kind: ContainerKind) -> Result<(), &'static str> {
        ensure_option_inner(self).ensure_container(kind)
    }

    fn key_erased(&mut self, name: &str) -> Result<ErasedPtr, &'static str> {
        ensure_option_inner(self).key_erased(name)
    }

    fn index_erased(&mut self, idx: usize) -> Result<ErasedPtr, &'static str> {
        ensure_option_inner(self).index_erased(idx)
    }

    fn number_kind(&self) -> Option<NumberKind> {
        self.as_ref().and_then(ValueDyn::number_kind)
    }
}

fn ensure_option_inner<T>(value: &mut Option<T>) -> &mut T
where
    T: ValueDyn + Default,
{
    value.get_or_insert_with(T::default)
}

impl<T> ValueDyn for BTreeMap<String, T>
where
    T: ValueDyn + Default + 'static,
{
    fn ty_name(&self) -> &'static str {
        "BTreeMap"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn ensure_container(&mut self, kind: ContainerKind) -> Result<(), &'static str> {
        match kind {
            ContainerKind::Map | ContainerKind::Struct => Ok(()),
            _ => Err("map expects object container"),
        }
    }

    fn key_erased(&mut self, name: &str) -> Result<ErasedPtr, &'static str> {
        let entry = self.entry(name.to_owned()).or_default();
        Ok(ErasedPtr::new(entry))
    }
}

impl ValueDyn for () {
    fn ty_name(&self) -> &'static str {
        "()"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
