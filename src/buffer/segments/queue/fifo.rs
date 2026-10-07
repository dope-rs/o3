use std::{mem, ops};

use segments::queue;

use crate::{
    buffer,
    buffer::{
        segments,
        write::{consume, consume::Consume},
    },
    depot::queue::ring,
};

/// One exact ring owning its initialized segments and their byte total.
pub struct Fifo<T> {
    pub(in crate::buffer::segments) owners: ring::Fifo<segments::Item<T>>,
    pub(in crate::buffer::segments) len: u32,
    byte_capacity: u32,
}

impl<T> Fifo<T> {
    /// Allocates the complete owner ring. Successful construction makes every
    /// queue operation allocation-free.
    pub fn try_with(capacity: queue::Capacity) -> Result<Self, crate::alloc::Error> {
        Ok(Self {
            owners: ring::Fifo::try_new(capacity.segments())?,
            len: 0,
            byte_capacity: capacity.bytes().raw(),
        })
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub const fn len(&self) -> usize {
        self.len as usize
    }

    #[must_use]
    pub const fn spare_len(&self) -> usize {
        (self.byte_capacity - self.len) as usize
    }

    #[must_use]
    pub fn segments(&self) -> usize {
        self.owners.len()
    }

    #[must_use]
    pub fn get(&self, range: ops::Range<usize>) -> Option<queue::Slice<'_, T>> {
        queue::Slice::new(self, range)
    }

    /// Borrows the largest prefix within `max` from this unchanged queue.
    #[must_use]
    pub fn prefix(&self, max: usize) -> queue::Slice<'_, T> {
        let mut prefix = queue::Slice::all(self);
        prefix.end = self.len().min(max) as u32;
        prefix
    }

    #[must_use]
    pub fn cursor(&self) -> queue::Cursor<'_, T> {
        queue::Cursor::new(self)
    }

    pub(in crate::buffer) const fn fragments(&self) -> queue::Iter<'_, T> {
        queue::Iter::new(queue::Slice::all(self))
    }

    pub(in crate::buffer::segments::queue) fn as_slices(
        &self,
    ) -> (&[segments::Item<T>], &[segments::Item<T>]) {
        self.owners.as_slices()
    }

    pub(in crate::buffer::segments::queue) fn entries(
        &self,
    ) -> impl Iterator<Item = &segments::Item<T>> {
        (&self.owners).into_iter()
    }
}

impl<T: buffer::raw::Segment> Fifo<T> {
    /// Appends without copying. Empty segments are discarded and every failed
    /// admission returns the original owner unchanged.
    pub fn try_push(&mut self, seg: T) -> Result<(), crate::mem::Rejected<T, queue::Error>> {
        let ops::ControlFlow::Continue(seg) = segments::Item::new(seg) else {
            return Ok(());
        };
        let size = seg.len();
        if size.get() > self.spare_len() {
            return Err(crate::mem::Rejected::new(
                seg.into_inner(),
                queue::Error::Bytes {
                    requested: size,
                    available: self.spare_len(),
                },
            ));
        }
        let capacity = self.owners.capacity();
        let Some(vacant) = self.owners.vacant() else {
            return Err(crate::mem::Rejected::new(
                seg.into_inner(),
                queue::Error::Segments { capacity },
            ));
        };
        let len = self.len + size.get() as u32;
        vacant.push_back(seg);
        self.len = len;
        Ok(())
    }
}

impl<T: buffer::raw::Segment> Consume for Fifo<T> {
    fn remaining(&self) -> usize {
        self.len as usize
    }

    fn try_consume(&mut self, amount: usize) -> Result<(), consume::Error> {
        segments::Drain::try_new(self, amount)?.commit();
        Ok(())
    }

    fn consume_max(&mut self, max: usize) -> usize {
        let amount = max.min(self.len as usize);
        segments::Drain {
            target: self,
            amount,
        }
        .commit();
        amount
    }
}

const _: () = {
    assert!(mem::size_of::<Fifo<u64>>() == 4 * mem::size_of::<u64>());
    assert!(mem::size_of::<Option<Fifo<u64>>>() == mem::size_of::<Fifo<u64>>());
};
