use std::num::NonZero;

use crate::{buffer, buffer::segments::Cursor};

// SAFETY: construction borrows the same sequence for topology and byte total;
// every node retains a nonempty Item and each advance consumes an exact prefix.
unsafe impl<'a, T: buffer::raw::Segment> buffer::raw::Scatter<'a> for Cursor<'a, T> {
    fn remaining(&self) -> usize {
        self.remaining()
    }

    unsafe fn advance<const MAX: usize>(
        &mut self,
        cap: NonZero<usize>,
        left: NonZero<usize>,
    ) -> buffer::raw::Chunk<'a, MAX> {
        unsafe { self.next_chunk(cap, left) }
    }
}
