use std::{cell::Cell, process::abort};

#[repr(transparent)]
pub(in crate::buffer) struct LocalRefCount(Cell<u32>);

impl LocalRefCount {
    pub(in crate::buffer) const fn one() -> Self {
        Self(Cell::new(1))
    }

    pub(in crate::buffer) const fn empty() -> Self {
        Self(Cell::new(0))
    }

    pub(in crate::buffer) fn is_empty(&self) -> bool {
        self.0.get() == 0
    }

    pub(in crate::buffer) fn is_unique(&self) -> bool {
        self.0.get() == 1
    }

    pub(in crate::buffer) fn activate(&self) {
        debug_assert!(self.is_empty());
        self.0.set(1);
    }

    pub(in crate::buffer) fn deactivate(&self) {
        debug_assert!(self.is_unique());
        self.0.set(0);
    }

    pub(in crate::buffer) fn retain(&self) {
        let refs = self.0.get();
        debug_assert_ne!(refs, 0);
        let refs = refs.wrapping_add(1);
        self.0.set(refs);
        if refs == 0 {
            overflow();
        }
    }

    #[must_use]
    pub(in crate::buffer) fn release(&self) -> bool {
        let refs = self.0.get();
        debug_assert_ne!(refs, 0);
        if refs == 1 {
            true
        } else {
            self.0.set(refs - 1);
            false
        }
    }
}

const _: () = assert!(size_of::<LocalRefCount>() == size_of::<u32>());

#[cold]
fn overflow() -> ! {
    abort()
}
