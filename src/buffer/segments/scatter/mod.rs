//! Allocation-free traversal over immutable scattered bytes.

use std::{marker, num::NonZero};

pub use reader::Reader;

use crate::buffer;

pub mod raw;
mod reader;

/// Safe traversal for a source whose raw implementation proves exactness.
pub trait Source: buffer::raw::Chunks {
    fn chunks(&self) -> Self::Cursor<'_> {
        buffer::raw::Chunks::start(self)
    }

    /// Copies an exact array-sized suffix without changing the retained source.
    fn suffix<const N: usize>(&self) -> Option<[u8; N]> {
        let mut cursor = self.chunks();
        let skipped = buffer::raw::Scatter::remaining(&cursor).checked_sub(N)?;
        Take::new(&mut cursor, skipped).finish();
        let mut output = [0; N];
        let source = Take::new(&mut cursor, usize::MAX);
        // SAFETY: admission and exact traversal leave N bytes in the retained
        // source, disjoint from this exclusive array of the same length.
        unsafe {
            buffer::raw::copy_slices(source.map(|chunk| chunk.bytes()), output.as_mut_ptr());
        }
        Some(output)
    }
}

impl<T: buffer::raw::Chunks + ?Sized> Source for T {}

/// An exclusively borrowed byte-bounded view of an exact cursor.
#[must_use = "a bounded scatter view does nothing until it is iterated"]
pub struct Take<'cursor, 'data, C> {
    cursor: &'cursor mut C,
    remaining: usize,
    data: marker::PhantomData<&'data [u8]>,
}

impl<'cursor, 'data, C> Take<'cursor, 'data, C> {
    pub fn new(cursor: &'cursor mut C, limit: usize) -> Self
    where
        C: buffer::raw::Scatter<'data>,
    {
        let len = limit.min(cursor.remaining());
        Self {
            cursor,
            remaining: len,
            data: marker::PhantomData,
        }
    }

    pub const fn len(&self) -> usize {
        self.remaining
    }

    pub const fn is_empty(&self) -> bool {
        self.remaining == 0
    }
}

impl<'data, C: buffer::raw::Scatter<'data>> Take<'_, 'data, C> {
    /// Returns one nonempty chunk whose length also fits the static byte limit.
    pub fn next_chunk<const MAX: usize>(&mut self) -> Option<buffer::raw::Chunk<'data, MAX>> {
        const { assert!(MAX > 0) };
        let cap = NonZero::new(self.remaining.min(MAX))?;
        Some(self.advance(cap))
    }

    fn advance<const MAX: usize>(&mut self, cap: NonZero<usize>) -> buffer::raw::Chunk<'data, MAX> {
        // SAFETY: the exclusive cursor borrow retains this admitted prefix.
        // Exact advancement consumes at most cap, which fits both remaining counts.
        let left = unsafe { NonZero::new_unchecked(self.cursor.remaining()) };
        let chunk = unsafe { self.cursor.advance(cap, left) };
        self.remaining -= chunk.len().get();
        chunk
    }

    pub(in crate::buffer) fn finish(mut self) {
        while !self.is_empty() {
            let _ = self.next();
        }
    }
}

impl<'data, C: buffer::raw::Scatter<'data>> Iterator for Take<'_, 'data, C> {
    type Item = buffer::raw::Chunk<'data>;

    fn next(&mut self) -> Option<Self::Item> {
        let cap = NonZero::new(self.remaining)?;
        Some(self.advance(cap))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        if self.is_empty() {
            (0, Some(0))
        } else {
            (1, None)
        }
    }
}
