use std::mem;

use crate::{buffer, buffer::segments::queue};

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

    pub(in crate::buffer::segments::queue) fn end(
        &self,
        offset: usize,
        len: usize,
    ) -> Result<u32, queue::Bounds> {
        let floor = self.start as usize;
        let ceiling = self.queue.len();
        if offset < floor || offset > ceiling || len > ceiling - offset {
            return Err(queue::Bounds {
                offset,
                amount: len,
                floor,
                ceiling,
            });
        }
        Ok((offset + len) as u32)
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
        self.end(offset, 0)?;
        // SAFETY: admission retains this cursor and its inclusive queue bounds.
        *self = unsafe { self.at(offset as u32) };
        Ok(())
    }

    /// # Safety
    /// `offset` is between this cursor's start and its queue's byte length, inclusive.
    pub(in crate::buffer::segments::queue) unsafe fn at(&self, offset: u32) -> Self {
        if offset as usize == self.len() {
            return Self {
                queue: self.queue,
                segment: self.queue.segments() as u32,
                start: offset,
            };
        }
        // SAFETY: the admitted offset is below the end handled above.
        unsafe { self.locate(offset) }.0
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
