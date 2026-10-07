//! Borrowed scatter fixtures.

use std::{cell::Cell, mem::MaybeUninit, num::NonZero, slice};

use o3::{buffer, buffer::write::consume};

/// A borrowed list of byte slices with a checked exact length.
#[derive(Debug)]
pub struct Slices<'data> {
    parts: &'data [&'data [u8]],
    len: usize,
}

/// Exact traversal over [`Slices`].
#[derive(Debug)]
pub struct Cursor<'data> {
    parts: slice::Iter<'data, &'data [u8]>,
    current: &'data [u8],
    remaining: usize,
}

impl<'data> Slices<'data> {
    pub fn try_new(parts: &'data [&'data [u8]]) -> Result<Self, consume::Error> {
        let mut len = 0;
        for part in parts {
            consume::Error::check(part.len(), usize::MAX - len)?;
            len += part.len();
        }
        Ok(Self { parts, len })
    }
}

// SAFETY: each cursor retains the exact immutable suffix and its byte total;
// current is nonempty whenever any bytes remain.
unsafe impl<'data> buffer::raw::Scatter<'data> for Cursor<'data> {
    fn remaining(&self) -> usize {
        self.remaining
    }

    unsafe fn advance<const MAX: usize>(
        &mut self,
        cap: NonZero<usize>,
        _left: NonZero<usize>,
    ) -> buffer::raw::Chunk<'data, MAX> {
        let len = self.current.len().min(cap.get());
        let (bytes, suffix) = self.current.split_at(len);
        self.current = suffix;
        self.remaining -= len;
        if suffix.is_empty() {
            self.current = self
                .parts
                .find(|part| !part.is_empty())
                .copied()
                .unwrap_or_default();
        }
        // SAFETY: the nonempty current slice advances by at most the admitted cap.
        unsafe { buffer::raw::Chunk::new(bytes) }
    }
}

// SAFETY: construction checks the byte total and each cursor keeps the source borrow.
unsafe impl buffer::raw::Chunks for Slices<'_> {
    type Cursor<'data>
        = Cursor<'data>
    where
        Self: 'data;

    fn len(&self) -> usize {
        self.len
    }

    fn start(&self) -> Self::Cursor<'_> {
        let mut parts = self.parts.iter();
        let current = parts
            .find(|part| !part.is_empty())
            .copied()
            .unwrap_or_default();
        Cursor {
            parts,
            current,
            remaining: self.len,
        }
    }
}

pub struct Source<'a>(pub &'a [u8], pub &'a Cell<Option<usize>>);

// SAFETY: the source retains the exact immutable bytes it writes, and writes
// the complete prefix without reading or retaining the destination.
unsafe impl buffer::raw::Initializer for Source<'_> {
    fn len(&self) -> usize {
        self.0.len()
    }

    unsafe fn initialize(&self, target: *mut MaybeUninit<u8>) {
        self.1
            .set(self.1.get().and_then(|count| count.checked_add(1)));
        // SAFETY: the caller admits this exact length in a disjoint destination.
        unsafe {
            target
                .cast::<u8>()
                .copy_from_nonoverlapping(self.0.as_ptr(), self.0.len())
        };
    }
}
