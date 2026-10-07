use std::ptr;

use crate::buffer::storage::provided::arena;

/// Affine ownership of one vacant slot and its permanently typed free link.
pub(in crate::buffer::storage::provided) struct Empty<'d> {
    pub(super) entry: ptr::NonNull<arena::Entry<'d>>,
}

impl<'d> Empty<'d> {
    pub(super) fn take_next(&mut self) -> Option<Self> {
        // SAFETY: this detached free node exclusively owns its initialized link.
        unsafe { self.entry.as_ref() }.free.take()
    }
}

impl crate::mem::Return for Empty<'_> {
    type Item = ();

    fn return_item(self, (): Self::Item) {
        // SAFETY: the affine vacancy retains its original detached slot and domain.
        unsafe { self.entry.as_ref() }.domain.push_free(self);
    }
}

/// Affine ownership of one initialized item and its permanently typed return link.
pub(in crate::buffer::storage::provided) struct Ready<'d> {
    pub(super) entry: ptr::NonNull<arena::Entry<'d>>,
}

impl<'d> Ready<'d> {
    pub(super) fn take_next(&mut self) -> Option<Self> {
        // SAFETY: this detached return node exclusively owns its initialized link.
        unsafe { self.entry.as_ref() }.returned.take()
    }
}
