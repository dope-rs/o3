use std::ptr;

use crate::buffer::storage::provided::arena;

/// Affine ownership of one slot in its retained phase.
pub(in crate::buffer::storage::provided) struct Node<'d, const READY: bool> {
    pub(super) entry: ptr::NonNull<arena::Entry<'d>>,
}

pub(in crate::buffer::storage::provided) type Empty<'d> = Node<'d, false>;
pub(in crate::buffer::storage::provided) type Ready<'d> = Node<'d, true>;

impl<'d> Empty<'d> {
    /// # Safety
    /// Own the removed list head. Transfer its successor exactly once before reusing the slot.
    pub(super) unsafe fn next(&self) -> Option<Self> {
        // SAFETY: on the free list the slot word contains its vacant successor.
        unsafe { self.entry.as_ref().data.get().free }.map(|entry| Self { entry })
    }
}

impl crate::mem::Return for Empty<'_> {
    type Item = ();

    fn return_item(self, (): Self::Item) {
        // SAFETY: the affine vacancy retains its original detached slot and domain.
        unsafe { self.entry.as_ref() }.domain.push_free(self);
    }
}
