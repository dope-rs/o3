use std::num::NonZero;

use crate::{
    buffer,
    buffer::segments::{
        queue,
        queue::{Fifo, Iter},
    },
};

// SAFETY: the iterator retains one immutable queue and its exact bounded range.
unsafe impl<'a, T: buffer::raw::Segment> buffer::raw::Scatter<'a> for Iter<'a, T> {
    fn remaining(&self) -> usize {
        self.remaining()
    }

    unsafe fn advance<const MAX: usize>(
        &mut self,
        cap: NonZero<usize>,
        left: NonZero<usize>,
    ) -> buffer::raw::Chunk<'a, MAX> {
        // SAFETY: the raw cursor contract supplies the exact bounds for this
        // borrowed state; Step retains them until the physical read completes.
        unsafe { self.next_chunk(cap, left) }
    }
}

unsafe impl<T: buffer::raw::Segment> buffer::raw::Chunks for queue::Slice<'_, T> {
    type Cursor<'data>
        = queue::Iter<'data, T>
    where
        Self: 'data;

    fn len(&self) -> usize {
        self.len()
    }

    fn start(&self) -> Self::Cursor<'_> {
        self.fragments()
    }

    fn cursor(&self, offset: usize) -> Option<Self::Cursor<'_>> {
        self.get(offset..self.len())
            .map(<queue::Slice<'_, T>>::fragments)
    }
}

unsafe impl<T: buffer::raw::Segment> buffer::raw::Chunks for Fifo<T> {
    type Cursor<'data>
        = queue::Iter<'data, T>
    where
        Self: 'data;

    fn len(&self) -> usize {
        self.len()
    }

    fn start(&self) -> Self::Cursor<'_> {
        self.fragments()
    }

    fn cursor(&self, offset: usize) -> Option<Self::Cursor<'_>> {
        self.get(offset..self.len())
            .map(<queue::Slice<'_, T>>::fragments)
    }
}
