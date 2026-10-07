use std::mem;

use crate::depot::{
    arena::{linked, linked::slots},
    capacity,
    queue::pool,
};

/// Shared access retaining one exact FIFO lane and its original owner borrow.
pub struct View<'a, T, S = (), const TRACK: bool = true> {
    store: &'a slots::Store<T>,
    slot: &'a crate::depot::raw::QueueSlot<S>,
}

impl<'a, T, S, const TRACK: bool> View<'a, T, S, TRACK> {
    pub(in crate::depot) const fn new(
        store: &'a slots::Store<T>,
        slot: &'a crate::depot::raw::QueueSlot<S>,
    ) -> Self {
        Self { store, slot }
    }

    pub const fn len(self) -> usize {
        self.count().get()
    }

    pub const fn count(self) -> capacity::Count {
        self.slot.queue.count()
    }

    pub const fn is_empty(self) -> bool {
        self.slot.queue.is_empty()
    }

    pub fn front(self) -> Option<&'a T> {
        unsafe { self.slot.queue.front(self.store) }
    }

    pub fn iter(self) -> linked::Iter<'a, T> {
        unsafe { self.slot.queue.iter(self.store) }
    }

    pub const fn state(self) -> &'a S {
        &self.slot.state
    }
}

/// Consumes the exclusive capability while retaining its original owner borrow.
impl<'a, T, S, const TRACK: bool> From<pool::Lane<'a, T, S, TRACK>> for View<'a, T, S, TRACK> {
    fn from(lane: pool::Lane<'a, T, S, TRACK>) -> Self {
        Self::new(lane.store, lane.slot)
    }
}

/// Reborrows the retained lane without consuming its exclusive capability.
impl<'a, T, S, const TRACK: bool> From<&'a pool::Lane<'_, T, S, TRACK>> for View<'a, T, S, TRACK> {
    fn from(lane: &'a pool::Lane<'_, T, S, TRACK>) -> Self {
        Self::new(lane.store, lane.slot)
    }
}

impl<T, S, const TRACK: bool> Clone for View<'_, T, S, TRACK> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T, S, const TRACK: bool> Copy for View<'_, T, S, TRACK> {}

const _: () = assert!(mem::size_of::<View<'static, u64>>() == 2 * mem::size_of::<usize>());
