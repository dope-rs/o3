use std::{cell, marker, mem};

use crate::{
    alloc::{self, array},
    depot::{capacity, queue::ring},
};

/// Exact slots partitioned between available capacity, leases, and queued values.
pub struct Pool<T> {
    pub(in crate::depot) block: array::Block<mem::MaybeUninit<T>, capacity::Count, cell::Cell<u32>>,
    pub(in crate::depot) state: cell::Cell<ring::State<u32>>,
    invariant: marker::PhantomData<cell::Cell<T>>,
}

impl<T> Pool<T> {
    pub fn try_new(capacity: capacity::Count) -> Result<Self, alloc::Error> {
        Ok(Self {
            block: array::Block::try_new(capacity, cell::Cell::new(capacity.raw()))?,
            state: cell::Cell::new(ring::State::EMPTY),
            invariant: marker::PhantomData,
        })
    }

    pub fn try_reserve(&self) -> Result<ring::Lease<'_, T>, ring::Full> {
        let free = self.block.state().get();
        if free == 0 {
            return Err(ring::Full {
                used: self.capacity(),
                capacity: self.capacity(),
            });
        }
        self.block.state().set(free - 1);
        Ok(ring::Lease { pool: self })
    }

    /// Transfers the front value and its capacity into a retained reservation.
    pub fn take_front(&self) -> Option<(T, ring::Lease<'_, T>)> {
        let value = crate::depot::raw::RingCell::take_front(self)?;
        Some((value, ring::Lease { pool: self }))
    }

    pub fn len(&self) -> usize {
        self.state.get().len()
    }

    pub fn count(&self) -> capacity::Count {
        self.state.get().count()
    }

    pub fn is_empty(&self) -> bool {
        self.state.get().is_empty()
    }

    pub const fn capacity(&self) -> usize {
        self.block.extent().get()
    }
}

impl<T> Drop for Pool<T> {
    fn drop(&mut self) {
        // SAFETY: no lease or shared operation survives terminal exclusive ownership.
        unsafe { crate::depot::raw::drop_ring(self) };
    }
}

const _: () = assert!(size_of::<Pool<u64>>() == size_of::<ring::Fifo<u64>>());
