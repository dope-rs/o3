//! Exact-capacity retained segment queues.

use std::{mem, num::NonZero};

pub use bounds::Bounds;
pub use cursor::Cursor;
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
pub(in crate::buffer::segments) use slice::Projected;

mod fifo;

/// An exact array-sized range retaining its immutable queue owner.
pub struct Array<'a, T, const N: usize> {
    source: Iter<'a, T>,
}

impl<'a, T: buffer::raw::Segment, const N: usize> Array<'a, T, N> {
    /// Validates an exact array-sized range against this cursor without advancing it.
    /// The result retains the queue borrow, independently of the cursor borrow.
    pub fn try_new(location: &queue::Cursor<'a, T>, offset: usize) -> Result<Self, queue::Bounds> {
        let end = location.end(offset, N)?;
        Ok(Self {
            source: Iter {
                location: queue::Cursor {
                    queue: location.queue,
                    segment: location.segment,
                    start: location.start,
                },
                start: offset as u32,
                end,
            },
        })
    }

    pub fn contiguous(&mut self) -> Option<&'a [u8; N]> {
        let Some(cap) = NonZero::new(N) else {
            return Some(&[0; N]);
        };
        // SAFETY: array admission retains exactly N bytes without advancing this cursor.
        unsafe { self.source.peek_chunk::<{ usize::MAX }>(cap) }
            .bytes()
            .as_array()
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
    /// The caller proves cap <= self.remaining() and cap <= MAX.
    unsafe fn peek_chunk<const MAX: usize>(
        &mut self,
        cap: NonZero<usize>,
    ) -> buffer::raw::Chunk<'a, MAX> {
        // SAFETY: positive remaining bytes lie in this cursor's admitted queue range.
        let (location, bytes) = unsafe { self.location.locate(self.start) };
        let start = (self.start - location.start) as usize;
        let len = (bytes.len().get() - start).min(cap.get());
        // SAFETY: start lies inside this item and cap bounds the positive suffix.
        let part = unsafe { bytes.bytes().get_unchecked(start..start + len) };
        self.location = location;
        // SAFETY: len is positive, <= cap <= MAX, and retains the queue borrow.
        unsafe { buffer::raw::Chunk::new(part) }
    }

    /// # Safety
    /// The caller proves cap <= self.remaining() and cap <= MAX.
    pub(in crate::buffer) unsafe fn next_chunk<const MAX: usize>(
        &mut self,
        cap: NonZero<usize>,
    ) -> buffer::raw::Chunk<'a, MAX> {
        // SAFETY: the caller supplies this unchanged cursor's exact cap bounds.
        let chunk = unsafe { self.peek_chunk(cap) };
        self.start += chunk.len().get() as u32;
        chunk
    }

}

impl<'a, T: buffer::raw::Segment> Iterator for Iter<'a, T> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        let left = NonZero::new(buffer::raw::Scatter::remaining(self))?;
        // SAFETY: left is this cursor's positive remaining range and fits usize::MAX.
        Some(unsafe { self.next_chunk::<{ usize::MAX }>(left) }.bytes())
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
