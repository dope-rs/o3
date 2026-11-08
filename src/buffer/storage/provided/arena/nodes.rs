use std::{cell, ptr};

use crate::buffer::storage::provided::arena;

pub(super) type Link<'d> = cell::Cell<Option<ptr::NonNull<arena::Entry<'d>>>>;

/// Affine ownership of one slot in its retained phase.
pub(in crate::buffer::storage::provided) struct Node<'d, const READY: bool> {
    pub(super) entry: ptr::NonNull<arena::Entry<'d>>,
}

pub(in crate::buffer::storage::provided) type Empty<'d> = Node<'d, false>;
pub(in crate::buffer::storage::provided) type Ready<'d> = Node<'d, true>;

impl<'d, const READY: bool> Node<'d, READY> {
    /// # Safety
    /// This node is the removed head of its live phase's list.
    pub(super) unsafe fn take_next(&mut self) -> Option<Self> {
        // SAFETY: this live list's links retain nodes in the same phase and domain.
        unsafe { self.entry.as_ref() }
            .next
            .take()
            .map(|entry| Self { entry })
    }
}

impl crate::mem::Return for Empty<'_> {
    type Item = ();

    fn return_item(self, (): Self::Item) {
        // SAFETY: the affine vacancy retains its original detached slot and domain.
        unsafe { self.entry.as_ref() }.domain.push_free(self);
    }
}
