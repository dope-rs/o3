use std::{alloc::Layout, mem, ptr};

pub use error::Error;
pub use gap::Gap;
pub use vacant::Vacant;

use crate::alloc;

pub mod array;
mod error;
mod gap;
pub mod raw;
pub mod tail;
mod vacant;

/// The allocator could not satisfy a validated layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Exhausted;

/// Capacity or layout arithmetic exceeded a representable bound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Overflow;

/// Fallible allocation for one boxed value.
pub trait BoxExt<T>: Sized {
    fn try_new(value: T) -> Result<Self, Exhausted>;
}

impl<T> BoxExt<T> for Box<T> {
    fn try_new(value: T) -> Result<Self, Exhausted> {
        let entry =
            crate::alloc::raw::LayoutExt::try_allocate::<mem::MaybeUninit<T>>(Layout::new::<T>())?;
        // SAFETY: this uniquely owned allocation has the exact layout of
        // MaybeUninit<T>, including its aligned dangling pointer for a ZST.
        let entry = unsafe { Box::from_raw(entry.as_ptr()) };
        Ok(Box::write(entry, value))
    }
}

/// Fallible allocation for an uninitialized boxed slice.
pub trait BoxUninitExt<T>: Sized {
    fn try_uninit(len: usize) -> Result<Self, Error>;
}

impl<T> BoxUninitExt<T> for Box<[mem::MaybeUninit<T>]> {
    fn try_uninit(len: usize) -> Result<Self, Error> {
        let plan = crate::alloc::raw::Layout::<T>::try_layout(len)?;
        let data = crate::alloc::raw::LayoutExt::try_allocate::<mem::MaybeUninit<T>>(plan)?;
        let entries = ptr::slice_from_raw_parts_mut(data.as_ptr(), len);
        // SAFETY: the allocation has this slice's exact layout; MaybeUninit
        // carries the uninitialized state until the caller writes each entry.
        Ok(unsafe { Box::from_raw(entries) })
    }
}

/// Fallible allocation for an uninitialized boxed array of exact length.
pub trait BoxArrayExt<T, const N: usize>: Sized {
    fn try_uninit() -> Result<Self, Exhausted>;
}

impl<T, const N: usize> BoxArrayExt<T, N> for Box<[mem::MaybeUninit<T>; N]> {
    fn try_uninit() -> Result<Self, Exhausted> {
        let data = crate::alloc::raw::LayoutExt::try_allocate(Layout::new::<[T; N]>())?;
        // SAFETY: the allocation has this array's exact layout; MaybeUninit
        // carries the uninitialized state until the caller writes each row.
        Ok(unsafe { Box::from_raw(data.as_ptr()) })
    }
}

/// Fallible allocation and initialization for one boxed slice.
pub trait BoxSliceExt<T>: Sized {
    fn try_copy(source: &[T]) -> Result<Self, Exhausted>
    where
        T: Copy;

    fn try_repeat(len: usize, value: T) -> Result<Self, Error>
    where
        T: Copy;

    /// Allocates first, then constructs each default value.
    fn try_default(len: usize) -> Result<Self, Error>
    where
        T: Default;

    /// Allocates first, then calls the generator once per element.
    fn try_init(len: usize, init: impl FnMut() -> T) -> Result<Self, Error>;

    /// Allocates first, then initializes each index in ascending order.
    fn try_from_fn(len: usize, init: impl FnMut(usize) -> T) -> Result<Self, Error>;
}

impl<T> BoxSliceExt<T> for Box<[T]> {
    fn try_copy(source: &[T]) -> Result<Self, Exhausted>
    where
        T: Copy,
    {
        let mut entries = array::Plan::from_slice(source).try_allocate(())?;
        if !source.is_empty() {
            entries.write_copy_of_slice(source);
        }
        // SAFETY: the complete owned extent was initialized by the exact copy.
        Ok(unsafe { entries.assume_init() }
            .into_vec()
            .into_boxed_slice())
    }

    fn try_repeat(len: usize, value: T) -> Result<Self, Error>
    where
        T: Copy,
    {
        Self::try_init(len, || value)
    }

    fn try_default(len: usize) -> Result<Self, Error>
    where
        T: Default,
    {
        Self::try_init(len, T::default)
    }

    fn try_init(len: usize, mut init: impl FnMut() -> T) -> Result<Self, Error> {
        Self::try_from_fn(len, |_| init())
    }

    fn try_from_fn(len: usize, init: impl FnMut(usize) -> T) -> Result<Self, Error> {
        let mut entries = Box::<[mem::MaybeUninit<T>]>::try_uninit(len)?;
        crate::alloc::raw::init(&mut entries, init);
        // SAFETY: the complete owned slice was initialized in index order.
        Ok(unsafe { entries.assume_init() })
    }
}

/// Fallible allocation and reservation for a vector.
pub trait VecExt<T>: Sized {
    fn try_new(cap: usize) -> Result<Self, Error>;

    fn try_grow(&mut self, extra: usize) -> Result<(), Error>;

    /// Reserves additional elements with geometric capacity growth.
    fn try_reserve(&mut self, extra: usize) -> Result<(), Error>;
}

/// Fallible ownership of an already validated UTF-8 string.
pub trait StrExt {
    fn try_to_owned(&self) -> Result<String, Exhausted>;
}

impl StrExt for str {
    fn try_to_owned(&self) -> Result<String, Exhausted> {
        let bytes = Box::<[u8]>::try_copy(self.as_bytes())?.into_vec();
        // SAFETY: the allocation contains an exact copy of this valid UTF-8 string.
        Ok(unsafe { String::from_utf8_unchecked(bytes) })
    }
}

impl<T> VecExt<T> for Vec<T> {
    fn try_new(cap: usize) -> Result<Self, Error> {
        Ok(array::Plan::try_new(cap)?.try_vec()?)
    }

    fn try_reserve(&mut self, extra: usize) -> Result<(), Error> {
        alloc::raw::Layout::<T>::try_grow::<true>(self, extra)
    }

    fn try_grow(&mut self, extra: usize) -> Result<(), Error> {
        alloc::raw::Layout::<T>::try_grow::<false>(self, extra)
    }
}
