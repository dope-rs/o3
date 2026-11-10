//! Borrowed scatter fixtures.

use std::{cell::Cell, mem::MaybeUninit};

use o3::buffer;

pub struct Source<'a>(pub &'a [u8], pub &'a Cell<Option<usize>>);

// SAFETY: the source retains the exact immutable bytes it writes, and writes
// the complete prefix without reading or retaining the destination.
unsafe impl buffer::raw::Initializer for Source<'_> {
    fn len(&self) -> usize {
        self.0.len()
    }

    unsafe fn initialize(&self, target: *mut MaybeUninit<u8>) {
        crate::common::bump(self.1);
        // SAFETY: the caller admits this exact length in a disjoint destination.
        unsafe {
            target
                .cast::<u8>()
                .copy_from_nonoverlapping(self.0.as_ptr(), self.0.len())
        };
    }
}
