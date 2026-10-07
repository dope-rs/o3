use std::{marker, ptr, slice};

use crate::alloc::{self, tail};

/// Pointer-sized borrow of one exact header-and-tail allocation.
#[repr(transparent)]
pub struct View<'a, H, T, N: alloc::raw::TailExtent = usize> {
    pub(in crate::alloc::tail) pointer: ptr::NonNull<tail::Prefix<H, T, N>>,
    pub(in crate::alloc::tail) borrow: marker::PhantomData<&'a tail::Block<H, T, N>>,
}

impl<'a, H, T, N: alloc::raw::TailExtent> View<'a, H, T, N> {
    pub(in crate::alloc) fn data(&self) -> ptr::NonNull<T> {
        let offset = tail::Plan::<H, T, N>::tail_offset();
        // SAFETY: this view retains the complete allocation provenance and the
        // admitted layout proves its aligned tail boundary, including an empty tail.
        unsafe { self.pointer.cast::<u8>().add(offset).cast() }
    }

    /// # Safety
    /// The pointer retains this exact initialized header-and-tail allocation for `'a`.
    pub(in crate::alloc) unsafe fn from_raw(pointer: ptr::NonNull<u8>) -> Self {
        Self {
            pointer: pointer.cast(),
            borrow: marker::PhantomData,
        }
    }

    pub fn new(block: &'a tail::Block<H, T, N>) -> Self {
        Self {
            pointer: block.pointer,
            borrow: marker::PhantomData,
        }
    }

    pub fn header(self) -> &'a H {
        // SAFETY: construction ties the initialized prefix's liveness to `'a`.
        unsafe { &self.pointer.as_ref().header }
    }

    pub fn is_empty(self) -> bool {
        self.len() == 0
    }

    pub fn len(self) -> usize {
        self.len_proof().len()
    }

    pub fn as_slice(&self) -> &'a [T] {
        let len = self.len_proof();
        // SAFETY: `len` proves the complete initialized tail layout and the
        // view borrow keeps that allocation live without mutable access.
        unsafe { slice::from_raw_parts(self.data().as_ptr(), len.len()) }
    }

    pub(in crate::alloc::tail) fn len_proof(&self) -> tail::Plan<H, T, N> {
        // SAFETY: construction ties this initialized immutable prefix to `'a`.
        unsafe { self.pointer.as_ref().len }
    }
}

impl<H, T, N: alloc::raw::TailExtent> Clone for View<'_, H, T, N> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<H, T, N: alloc::raw::TailExtent> Copy for View<'_, H, T, N> {}
