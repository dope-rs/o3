//! Allocation-free traversal over immutable scattered bytes.

use std::{marker, num::NonZero};

pub use reader::Reader;

use crate::buffer;

pub mod raw;
mod reader;

/// An exclusively borrowed byte-bounded view of an exact cursor.
#[must_use = "a bounded scatter view does nothing until it is iterated"]
pub struct Take<'cursor, 'data, C> {
    cursor: &'cursor mut C,
    remaining: usize,
    data: marker::PhantomData<&'data [u8]>,
}

const _: () = assert!(size_of::<Take<'static, 'static, &'static [u8]>>() == 2 * size_of::<usize>());

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
        // SAFETY: the exclusive cursor borrow retains this admitted prefix.
        // Exact advancement consumes at most cap, which fits both remaining counts.
        let chunk = unsafe { self.cursor.advance(cap) };
        self.remaining -= chunk.len().get();
        Some(chunk)
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
        self.next_chunk::<{ usize::MAX }>()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        if self.is_empty() {
            (0, Some(0))
        } else {
            (1, None)
        }
    }
}
