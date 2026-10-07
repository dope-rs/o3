use std::{alloc::Layout, marker, mem};

use crate::alloc::{self, array};

/// A validated array layout tied to its exact element type and immutable extent.
#[repr(transparent)]
#[derive(Debug)]
pub struct Plan<T, E: alloc::raw::Extent> {
    extent: E,
    value: marker::PhantomData<fn() -> T>,
}

impl<T, E: alloc::raw::Extent> Plan<T, E> {
    pub fn try_new(extent: E) -> Result<Self, alloc::Overflow> {
        alloc::raw::Layout::<T>::try_check(extent.len())?;
        Ok(Self {
            extent,
            value: marker::PhantomData,
        })
    }

    /// Reuses the layout proof; each new allocation can still exhaust memory.
    pub fn try_allocate<S>(
        self,
        state: S,
    ) -> Result<array::Block<mem::MaybeUninit<T>, E, S>, alloc::Exhausted> {
        Ok(array::Block {
            pointer: alloc::raw::LayoutExt::try_allocate(self.layout())?,
            plan: self.uninit(),
            state,
        })
    }

    pub(in crate::alloc::array) const fn empty(extent: alloc::raw::Empty<E>) -> Self {
        Self {
            extent: extent.into_inner(),
            value: marker::PhantomData,
        }
    }

    pub const fn extent(&self) -> &E {
        &self.extent
    }

    pub(in crate::alloc::array) fn layout(&self) -> Layout {
        // SAFETY: admission proves this exact T and immutable extent fit.
        unsafe { alloc::raw::Layout::<T>::layout(self.extent.len()) }
    }

    pub(in crate::alloc::array) const fn uninit(self) -> Plan<mem::MaybeUninit<T>, E> {
        Plan {
            extent: self.extent,
            value: marker::PhantomData,
        }
    }
}

impl<T, E: alloc::raw::Extent> Plan<mem::MaybeUninit<T>, E> {
    pub(super) const fn initialized(self) -> Plan<T, E> {
        Plan {
            extent: self.extent,
            value: marker::PhantomData,
        }
    }
}

impl<T> Plan<T, usize> {
    /// Retains the valid layout of an existing slice with this exact element type.
    pub const fn from_slice(source: &[T]) -> Self {
        Self {
            extent: source.len(),
            value: marker::PhantomData,
        }
    }

    /// Plans geometric reservation without changing the current allocation.
    /// `required` includes the initialized prefix and all additional elements.
    pub fn try_reserve(capacity: usize, required: usize) -> Result<Self, alloc::Overflow> {
        Self::try_new(alloc::raw::Layout::<T>::reserve(capacity, required))
    }

    pub(crate) fn from(len: u8) -> Self {
        const {
            assert!(alloc::raw::Layout::<T>::fits(u8::MAX as usize), "layout");
        };
        Self {
            extent: usize::from(len),
            value: marker::PhantomData,
        }
    }

    /// Allocates an empty vector while retaining this plan's checked capacity.
    pub fn try_vec(self) -> Result<Vec<T>, alloc::Exhausted> {
        let pointer = alloc::raw::LayoutExt::try_allocate::<T>(self.layout())?;
        // SAFETY: this exact T and extent were validated; Vec receives the
        // sole allocation with no initialized elements, including for a ZST.
        Ok(unsafe { Vec::from_raw_parts(pointer.as_ptr(), 0, self.extent) })
    }
}

impl Plan<u8, usize> {
    /// Allocates an empty UTF-8 string with the already checked byte capacity.
    pub fn try_string(self) -> Result<String, alloc::Exhausted> {
        // SAFETY: try_vec transfers an empty vector, which is valid UTF-8.
        Ok(unsafe { String::from_utf8_unchecked(self.try_vec()?) })
    }
}

impl<T, E: alloc::raw::Extent> Clone for Plan<T, E> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T, E: alloc::raw::Extent> Copy for Plan<T, E> {}

const _: () = assert!(mem::size_of::<Plan<u8, usize>>() == mem::size_of::<usize>());
