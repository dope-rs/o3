use std::cell;

use crate::mem;

#[repr(C, align(8))]
#[derive(Debug)]
pub(in crate::buffer) struct Prefix<T: Copy = u32> {
    refs: mem::raw::Count,
    word: cell::Cell<T>,
}

impl<T: Copy> Prefix<T> {
    pub(in crate::buffer) const fn empty(word: T) -> Self {
        Self {
            refs: mem::raw::Count::empty(),
            word: cell::Cell::new(word),
        }
    }

    pub(in crate::buffer) const fn one(word: T) -> Self {
        Self {
            refs: mem::raw::Count::one(),
            word: cell::Cell::new(word),
        }
    }

    pub(in crate::buffer) fn word(&self) -> T {
        self.word.get()
    }

    /// # Safety
    /// The caller must exclusively own this prefix's configuration state.
    pub(in crate::buffer) unsafe fn write(&self, word: T) {
        self.word.set(word);
    }
}

// SAFETY: each prefix keeps its nominal reference count at one immutable field address.
unsafe impl<T: Copy> mem::raw::Edge for Prefix<T> {
    fn count(&self) -> &mem::raw::Count {
        &self.refs
    }
}
