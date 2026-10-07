use std::cell;

use crate::buffer::pool::owners::Key;

/// The head of one exact pool list, with its slot/account domain kept distinct.
#[repr(transparent)]
pub(super) struct Free<const ACCOUNT: bool = false> {
    head: cell::Cell<u32>,
}

impl<const ACCOUNT: bool> Free<ACCOUNT> {
    pub(super) const fn new(limit: u32) -> Self {
        Self {
            head: cell::Cell::new(Key::<ACCOUNT>::encode(if limit == 0 {
                None
            } else {
                // SAFETY: the enclosing owner initializes this exact nonempty
                // list in index order before publishing its head.
                Some(unsafe { Key::new(0) })
            })),
        }
    }

    pub(super) fn head(&self) -> Option<Key<ACCOUNT>> {
        // SAFETY: the head contains only this exact list's initialized or
        // exclusively returned keys, and preserves its ACCOUNT domain.
        unsafe { Key::decode(self.head.get()) }
    }

    /// # Safety
    /// The caller exclusively claims this head; next is its unchanged successor
    /// from this exact list's vacant state.
    pub(super) unsafe fn pop(&self, next: u32) {
        self.head.set(next);
    }

    /// # Safety
    /// The caller owns this exact retired key. The private callback writes its
    /// vacant successor without reentry, before this head publishes the key.
    pub(super) unsafe fn push(&self, key: Key<ACCOUNT>, write: impl FnOnce(Option<Key<ACCOUNT>>)) {
        write(self.head());
        self.head.set(key.get());
    }
}

const _: () = {
    assert!(size_of::<Free>() == size_of::<u32>());
    assert!(size_of::<Free<true>>() == size_of::<u32>());
};
