//! Exact-capacity retained segment queues.

use std::{mem, num::NonZero};

pub use bounds::Bounds;
pub use cursor::Cursor;
pub(in crate::buffer::segments::queue) use cursor::Position;
pub use error::Error;
pub use fifo::Fifo;
pub use slice::Slice;

use crate::{
    buffer,
    buffer::{hold, segments, segments::queue},
    depot::capacity,
};

mod bounds;
mod cursor;
mod error;
mod slice;
pub(in crate::buffer::segments) use slice::Build;

mod fifo;

/// An exact array-sized range retaining its immutable queue owner.
pub struct Array<'a, T, const N: usize> {
    source: Iter<'a, T>,
}

impl<'a, T: buffer::raw::Segment, const N: usize> Array<'a, T, N> {
    /// Validates an exact array-sized range against this cursor without advancing it.
    /// The result retains the queue borrow, independently of the cursor borrow.
    pub fn try_new(location: &queue::Cursor<'a, T>, offset: usize) -> Result<Self, queue::Bounds> {
        let position = queue::Position::try_new(location, offset, N)?;
        Ok(Self {
            source: Iter::at(position),
        })
    }

    pub fn contiguous(&mut self) -> Option<&'a [u8; N]> {
        let Some(mut step) = Step::array(self) else {
            return Some(&[0; N]);
        };
        step.peek().bytes().as_array()
    }

    /// Copies into the array whose size was proved at construction.
    pub fn copy(self, target: &mut [u8; N]) {
        // SAFETY: admission retains exactly N immutable source bytes,
        // disjoint from this exclusive destination of the same length.
        unsafe { buffer::raw::copy_slices(self.source, target.as_mut_ptr()) };
    }
}

const _: () = assert!(size_of::<Array<'static, (), 9>>() == 3 * size_of::<usize>());

/// Exact metadata or payload-copy failure while retaining bounded owners.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetainError {
    Segments(segments::Error),
    Copy(hold::CopyError),
}

impl From<segments::Error> for RetainError {
    fn from(cause: segments::Error) -> Self {
        Self::Segments(cause)
    }
}

/// Allocation-free iterator over the physical fragments of one queue slice.
pub struct Iter<'a, T> {
    location: queue::Cursor<'a, T>,
    start: u32,
    end: u32,
}

/// A positive bounded read retaining its exact immutable cursor state.
struct Step<'cursor, 'data, T, const MAX: usize> {
    source: &'cursor mut Iter<'data, T>,
    cap: NonZero<usize>,
}

impl<'cursor, 'data, T> Step<'cursor, 'data, T, { usize::MAX }> {
    fn array<const N: usize>(source: &'cursor mut Array<'data, T, N>) -> Option<Self> {
        let cap = NonZero::new(N)?;
        Some(Self {
            source: &mut source.source,
            cap,
        })
    }
}

impl<'data, T: buffer::raw::Segment, const MAX: usize> Step<'_, 'data, T, MAX> {
    fn peek(&mut self) -> buffer::raw::Chunk<'data, MAX> {
        let source = &self.source;
        // SAFETY: Step retains positive remaining bytes in this cursor's
        // validated range and the unchanged queue that proves its totals.
        let (location, bytes) = unsafe { source.location.locate(source.start) };
        let start = (source.start - location.start) as usize;
        let len = (bytes.len().get() - start).min(self.cap.get());
        // SAFETY: start lies inside this nonempty item; cap is positive
        // and bounded by the same cursor's remaining logical range.
        let part = unsafe { bytes.bytes().get_unchecked(start..start + len) };
        self.source.location = location;
        // SAFETY: len is positive and <= cap <= MAX (Scatter's contract;
        // array/iterator steps use usize::MAX). The slice retains its source borrow.
        unsafe { buffer::raw::Chunk::new(part) }
    }

    fn advance(mut self) -> buffer::raw::Chunk<'data, MAX> {
        let chunk = self.peek();
        self.source.start += chunk.len().get() as u32;
        chunk
    }
}

impl<'a, T> Iter<'a, T> {
    pub(in crate::buffer::segments::queue) const fn new(slice: queue::Slice<'a, T>) -> Self {
        Self {
            location: queue::Cursor::new(slice.queue),
            start: slice.start,
            end: slice.end,
        }
    }
}

impl<'a, T: buffer::raw::Segment> Iter<'a, T> {
    pub(in crate::buffer) fn remaining(&self) -> usize {
        (self.end - self.start) as usize
    }

    /// # Safety
    /// Scatter's caller retains positive progress and the exact cap/left bounds.
    pub(in crate::buffer) unsafe fn next_chunk<const MAX: usize>(
        &mut self,
        cap: NonZero<usize>,
        _left: NonZero<usize>,
    ) -> buffer::raw::Chunk<'a, MAX> {
        Step { source: self, cap }.advance()
    }

    fn at(position: queue::Position<'_, 'a, T>) -> Self {
        Self {
            location: queue::Cursor {
                queue: position.location.queue,
                segment: position.location.segment,
                start: position.location.start,
            },
            start: position.start,
            end: position.end,
        }
    }
}

impl<'a, T: buffer::raw::Segment> Iterator for Iter<'a, T> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        let left = NonZero::new(buffer::raw::Scatter::remaining(self))?;
        Some(
            Step::<_, { usize::MAX }> {
                source: self,
                cap: left,
            }
            .advance()
            .bytes(),
        )
    }
}

const _: () = assert!(mem::size_of::<Iter<'static, u64>>() == 3 * mem::size_of::<u64>());

/// Exact byte and segment bounds for a fixed segment queue.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Capacity {
    bytes: capacity::Count,
    segments: capacity::Count,
}

impl Capacity {
    #[must_use]
    pub const fn new(bytes: capacity::Count, segments: capacity::Count) -> Self {
        Self { bytes, segments }
    }

    #[must_use]
    pub const fn bytes(self) -> capacity::Count {
        self.bytes
    }

    #[must_use]
    pub const fn segments(self) -> capacity::Count {
        self.segments
    }
}

const _: () = assert!(mem::size_of::<Capacity>() == mem::size_of::<u64>());
