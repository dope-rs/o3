use std::{mem, ops};

use crate::{
    buffer,
    buffer::{is_within, segments::queue},
};

pub struct Cursor<'a, T> {
    pub(in crate::buffer::segments::queue) queue: &'a queue::Fifo<T>,
    pub(in crate::buffer::segments::queue) segment: u32,
    pub(in crate::buffer::segments::queue) start: u32,
}

impl<'a, T> Cursor<'a, T> {
    pub(in crate::buffer::segments::queue) const fn new(queue: &'a queue::Fifo<T>) -> Self {
        Self {
            queue,
            segment: 0,
            start: 0,
        }
    }
}

/// Monotonic physical position tied to the queue it indexes.
impl<T> Cursor<'_, T> {
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub const fn len(&self) -> usize {
        self.queue.len()
    }
}

impl<'a, T: buffer::raw::Segment> Cursor<'a, T> {
    pub fn seek(&mut self, offset: usize) -> Result<(), queue::Bounds> {
        *self = queue::Position::try_new(self, offset, 0)?.locate();
        Ok(())
    }

    #[must_use]
    pub fn contiguous(&mut self, range: ops::Range<usize>) -> Option<&'a [u8]> {
        if range.start < self.start as usize || !is_within(&range, self.len()) {
            return None;
        }
        if range.is_empty() {
            *self = queue::Position {
                location: self,
                start: range.start as u32,
                end: range.end as u32,
            }
            .locate();
            return Some(&[]);
        }
        // SAFETY: admission proves start is at least this cursor's floor;
        // the nonempty range places it strictly below this queue's byte length.
        let (location, chunk) = unsafe { self.locate(range.start as u32) };
        let start = range.start - location.start as usize;
        let end = range.end - location.start as usize;
        let bytes = chunk.bytes().get(start..end)?;
        *self = location;
        Some(bytes)
    }

    /// # Safety
    /// `offset` is at least this cursor's start and below this queue's byte length.
    pub(in crate::buffer::segments::queue) unsafe fn locate(
        &self,
        offset: u32,
    ) -> (Self, buffer::raw::Chunk<'a>) {
        let (first, second) = self.queue.as_slices();
        let mut segment = self.segment;
        let mut start = self.start;
        loop {
            let index = segment as usize;
            // SAFETY: the unchanged queue's exact totals and positive remaining
            // range prove a live item until the containing segment is reached.
            let owner = unsafe {
                if index < first.len() {
                    first.get_unchecked(index)
                } else {
                    second.get_unchecked(index - first.len())
                }
            };
            let bytes = owner.chunk();
            let end = start + bytes.len().get() as u32;
            if offset < end {
                return (
                    Self {
                        queue: self.queue,
                        segment,
                        start,
                    },
                    bytes,
                );
            }
            segment += 1;
            start = end;
        }
    }
}

const _: () = assert!(mem::size_of::<Cursor<'static, u64>>() == 2 * mem::size_of::<u64>());

pub(in crate::buffer::segments::queue) struct Position<'p, 'a, T> {
    pub(in crate::buffer::segments::queue) location: &'p queue::Cursor<'a, T>,
    pub(in crate::buffer::segments::queue) start: u32,
    pub(in crate::buffer::segments::queue) end: u32,
}

impl<'p, 'a, T> Position<'p, 'a, T> {
    pub(in crate::buffer::segments::queue) fn try_new(
        location: &'p queue::Cursor<'a, T>,
        offset: usize,
        len: usize,
    ) -> Result<Self, queue::Bounds> {
        let floor = location.start as usize;
        let ceiling = location.queue.len();
        if offset < floor || offset > ceiling || len > ceiling - offset {
            return Err(queue::Bounds::new(offset, len, floor, ceiling));
        }
        Ok(Self {
            location,
            start: offset as u32,
            end: (offset + len) as u32,
        })
    }
}

impl<'a, T: buffer::raw::Segment> Position<'_, 'a, T> {
    pub(in crate::buffer::segments::queue) fn locate(&self) -> queue::Cursor<'a, T> {
        if self.start as usize == self.location.len() {
            return queue::Cursor {
                queue: self.location.queue,
                segment: self.location.queue.segments() as u32,
                start: self.start,
            };
        }
        // SAFETY: Position retains the validated range and its exact queue;
        // the end position was handled above.
        unsafe { self.location.locate(self.start) }.0
    }
}
