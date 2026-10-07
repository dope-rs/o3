use std::{mem, ptr, slice};

use crate::alloc::{self, tail};

/// Thin ownership of one exact header-and-tail allocation.
#[repr(transparent)]
pub struct Block<H, T, N: alloc::raw::TailExtent = usize> {
    pub(in crate::alloc::tail) pointer: ptr::NonNull<tail::Prefix<H, T, N>>,
}

impl<H, T, N: alloc::raw::TailExtent> Block<H, mem::MaybeUninit<T>, N> {
    /// # Safety
    /// Every tail slot contains an initialized T.
    pub(in crate::alloc::tail) unsafe fn assume_init(self) -> Block<H, T, N> {
        let block = mem::ManuallyDrop::new(self);
        Block {
            pointer: block.pointer.cast(),
        }
    }
}

impl<H, T, N: alloc::raw::TailExtent> Block<H, T, N> {
    pub(crate) fn extent(&self) -> N {
        tail::View::new(self).len_proof().extent()
    }

    pub fn header_mut(&mut self) -> &mut H {
        // SAFETY: the exclusive block borrow covers its initialized prefix.
        unsafe { &mut self.pointer.as_mut().header }
    }

    pub fn as_mut_slice(&mut self) -> &mut [T] {
        self.parts_mut().1
    }

    pub fn parts_mut(&mut self) -> (&mut H, &mut [T]) {
        let len = tail::View::new(self).len_proof();
        // SAFETY: the prefix header and the tail begin at disjoint regions of
        // the validated allocation under one exclusive block borrow.
        let header = unsafe { &mut self.pointer.as_mut().header };
        // SAFETY: the same exclusive borrow covers the initialized tail.
        let values = unsafe { slice::from_raw_parts_mut(self.data(), len.len()) };
        (header, values)
    }

    pub(crate) fn into_header(self) -> H {
        let owner = mem::ManuallyDrop::new(self);
        let len = tail::View::new(&owner).len_proof();
        let values = ptr::slice_from_raw_parts_mut(owner.data(), len.len());
        // SAFETY: terminal ownership covers every initialized tail element.
        unsafe { ptr::drop_in_place(values) };
        // SAFETY: the tail is gone, so the header can move out exactly once.
        let header = unsafe { ptr::read(&owner.pointer.as_ref().header) };
        // SAFETY: the moved header and dropped tail leave no live value in the
        // exact allocation reconstructed from its inseparable length proof.
        unsafe { alloc::raw::LayoutExt::deallocate(len.layout(), owner.pointer) };
        header
    }

    pub(in crate::alloc::tail) fn data(&self) -> *mut T {
        tail::View::new(self).data().as_ptr()
    }
}

impl<H, T, N: alloc::raw::TailExtent> Drop for Block<H, T, N> {
    fn drop(&mut self) {
        let len = tail::View::new(self).len_proof();
        let values = ptr::slice_from_raw_parts_mut(self.data(), len.len());
        // SAFETY: the complete tail remains initialized and exclusively owned.
        unsafe { ptr::drop_in_place(values) };
        // SAFETY: the tail is gone and the initialized prefix remains live.
        unsafe { ptr::drop_in_place(self.pointer.as_ptr()) };
        // SAFETY: `layout` is reconstructed from the inseparable length proof
        // and `self.pointer` is still this block's sole allocation owner.
        unsafe { alloc::raw::LayoutExt::deallocate(len.layout(), self.pointer) };
    }
}
