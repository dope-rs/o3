use std::{cell, ptr};

use crate::depot::index::bitmap::Width;

/// An initialized word whose mutations belong to the bitmap hierarchy.
#[repr(transparent)]
pub struct Word<W = usize> {
    pub(in crate::depot::index::bitmap) value: cell::Cell<W>,
}

impl<W: Width> Word<W> {
    /// Creates an independent empty word.
    pub const fn zero() -> Self {
        Self::new(W::ZERO)
    }

    /// Borrows initialized words from the exact existing storage.
    /// The `Words` implementation retains responsibility for the hierarchy.
    pub fn from_cells(cells: &[cell::Cell<W>]) -> &[Self] {
        // SAFETY: Word transparently wraps Cell<W>; pointer and length are unchanged.
        unsafe { &*(ptr::from_ref(cells) as *const [Self]) }
    }

    pub(in crate::depot::index::bitmap) const fn new(value: W) -> Self {
        Self {
            value: cell::Cell::new(value),
        }
    }

    pub(in crate::depot) fn get(&self) -> usize {
        self.value.get().offset()
    }
}

const _: () = {
    assert!(size_of::<Word>() == size_of::<usize>());
    assert!(align_of::<Word>() == align_of::<usize>());
    assert!(size_of::<Word<u32>>() == size_of::<u32>());
    assert!(align_of::<Word<u32>>() == align_of::<u32>());
};
