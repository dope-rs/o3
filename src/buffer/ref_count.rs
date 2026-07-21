use std::cell::Cell;

#[repr(transparent)]
pub(super) struct LocalRefCount(Cell<u32>);

impl LocalRefCount {
    pub(super) const fn one() -> Self {
        Self(Cell::new(1))
    }

    pub(super) const fn empty() -> Self {
        Self(Cell::new(0))
    }

    pub(super) fn is_empty(&self) -> bool {
        self.0.get() == 0
    }

    pub(super) fn is_unique(&self) -> bool {
        self.0.get() == 1
    }

    pub(super) fn activate(&self) {
        debug_assert!(self.is_empty());
        self.0.set(1);
    }

    pub(super) fn deactivate(&self) {
        debug_assert!(self.is_unique());
        self.0.set(0);
    }

    #[inline]
    pub(super) fn retain(&self) {
        let refs = self.0.get();
        debug_assert_ne!(refs, 0);
        let refs = refs.wrapping_add(1);
        self.0.set(refs);
        if refs == 0 {
            overflow();
        }
    }

    #[must_use]
    pub(super) fn release(&self) -> bool {
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
    std::process::abort()
}
