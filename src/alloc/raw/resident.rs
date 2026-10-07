use std::{marker, ptr};

use crate::alloc::{self, tail};

/// A pointer-sized resident borrow retaining its complete allocation provenance.
#[repr(transparent)]
pub struct Resident<'a, H, T, N: alloc::raw::TailExtent = usize> {
    pointer: ptr::NonNull<T>,
    borrow: marker::PhantomData<&'a tail::Block<H, T, N>>,
}

impl<'a, H, T, N: alloc::raw::TailExtent> Resident<'a, H, T, N> {
    /// # Safety
    /// `index` is within this exact initialized view's tail.
    pub unsafe fn new(view: tail::View<'a, H, T, N>, index: N::Index) -> Self {
        // SAFETY: the bounded ordinal is derived from the complete allocation pointer.
        let pointer = unsafe { view.data().add(alloc::raw::TailIndex::get(index)) };
        Self {
            pointer,
            borrow: marker::PhantomData,
        }
    }

    pub fn get(self) -> &'a T {
        // SAFETY: construction retains this initialized element and owner's lifetime.
        unsafe { self.pointer.as_ref() }
    }

    /// Recovers the original whole-allocation view from an immutable resident ordinal.
    /// # Safety
    /// `index` is exactly the ordinal passed when this resident was constructed.
    pub unsafe fn view(self, index: N::Index) -> tail::View<'a, H, T, N> {
        // SAFETY: subtracting the original ordinal and admitted tail offset
        // recovers the same retained allocation pointer without leaving its bounds.
        let pointer = unsafe {
            self.pointer
                .sub(alloc::raw::TailIndex::get(index))
                .cast::<u8>()
                .sub(tail::Plan::<H, T, N>::tail_offset())
                .cast()
        };
        // SAFETY: the exact original ordinal recovers this retained allocation's prefix.
        unsafe { tail::View::from_raw(pointer) }
    }
}

impl<H, T, N: alloc::raw::TailExtent> Clone for Resident<'_, H, T, N> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<H, T, N: alloc::raw::TailExtent> Copy for Resident<'_, H, T, N> {}

const _: () = {
    assert!(std::mem::size_of::<Resident<'static, u8, u8>>() == std::mem::size_of::<usize>());
    assert!(
        std::mem::size_of::<Option<Resident<'static, u8, u8>>>() == std::mem::size_of::<usize>()
    );
};
