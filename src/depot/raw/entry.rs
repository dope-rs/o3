use std::{cell, marker, mem, ptr};

/// A structurally pinned intrusive-list node and its caller-owned value.
/// The list never moves or drops the value; remove it before owner movement.
#[repr(C)]
pub struct Entry<T> {
    pub(super) previous: cell::Cell<Option<ptr::NonNull<Entry<T>>>>,
    pub(super) next: cell::Cell<Option<ptr::NonNull<Entry<T>>>>,
    value: T,
    pinned: marker::PhantomPinned,
}

impl<T> Entry<T> {
    pub const fn new(value: T) -> Self {
        Self {
            previous: cell::Cell::new(None),
            next: cell::Cell::new(None),
            value,
            pinned: marker::PhantomPinned,
        }
    }

    pub fn value(&self) -> &T {
        &self.value
    }
}

const _: () = {
    assert!(mem::align_of::<Entry<usize>>() == mem::align_of::<usize>());
    assert!(mem::size_of::<Entry<usize>>() == 3 * mem::size_of::<usize>());
};
