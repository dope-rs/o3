use std::{alloc::Layout, mem, num::NonZero};

use crate::alloc;

/// # Safety
/// Immutable members describe one representable allocation of aligned, disjoint Root/Tail/Extra/Last planes in order, starting Root at zero.
/// Offsets stay within `layout()`; every plane has `len()` elements except Tail, which has `tail_len()`.
pub unsafe trait Shape {
    type Root;
    type Tail;
    type Extra;
    type Last;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn len(&self) -> usize;

    fn tail_len(&self) -> usize {
        self.len()
    }

    fn offset(&self) -> usize {
        let size = mem::size_of::<Self::Root>().wrapping_mul(self.len());
        // SAFETY: the retained Shape proves this aligned plane offset fits.
        unsafe { alloc::raw::Layout::<Self::Tail>::offset(size) }
    }

    fn extra(&self) -> usize {
        let size = self
            .offset()
            .wrapping_add(mem::size_of::<Self::Tail>().wrapping_mul(self.tail_len()));
        // SAFETY: the retained Shape proves this aligned plane offset fits.
        unsafe { alloc::raw::Layout::<Self::Extra>::offset(size) }
    }

    fn last(&self) -> usize {
        let size = self
            .extra()
            .wrapping_add(mem::size_of::<Self::Extra>().wrapping_mul(self.len()));
        // SAFETY: the retained Shape proves this aligned plane offset fits.
        unsafe { alloc::raw::Layout::<Self::Last>::offset(size) }
    }

    fn layout(&self) -> Layout {
        let align = mem::align_of::<Self::Root>()
            .max(mem::align_of::<Self::Tail>())
            .max(mem::align_of::<Self::Extra>());
        let align = NonZero::<usize>::MIN.saturating_add(align - 1);
        // SAFETY: the immutable typed extent proves this complete allocation.
        unsafe { alloc::raw::Layout::<Self::Last>::at(self.last(), self.len(), align) }
            .pad_to_align()
    }
}
