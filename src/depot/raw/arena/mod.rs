use std::{marker, mem};

use crate::{
    alloc,
    depot::{
        arena::{
            linked,
            linked::{fifo, head, slots},
        },
        capacity,
        queue::pool::fifo::Fifo,
    },
};

/// Fixed node arena for compact external list and FIFO owners.
pub struct Arena<T, Tag = (), const TRACK: bool = true> {
    pub(in crate::depot) inner: slots::Store<T>,
    domain: marker::PhantomData<fn(Tag) -> Tag>,
}

impl<T, Tag, const TRACK: bool> Arena<T, Tag, TRACK> {
    pub fn try_new<N: capacity::Extent>(capacity: N) -> Result<Self, alloc::Error> {
        let capacity = slots::Capacity::new(capacity)?;
        let bundle = capacity.bundle();
        let nodes = bundle.try_allocate()?.root();
        Ok(Self {
            inner: unsafe { slots::Store::new(nodes, capacity) },
            domain: marker::PhantomData,
        })
    }
}

impl<T, const TRACK: bool> Arena<T, (), TRACK> {
    /// Borrows a FIFO from this exact region. Untracked regions require external credits.
    pub const fn fifo(&self) -> Fifo<'_, T, TRACK> {
        Fifo::new(self)
    }
}

impl<T, Tag> Arena<T, Tag> {
    pub fn available(&self) -> usize {
        self.inner.available()
    }

    pub fn vacant(&self) -> Option<slots::Vacant<T, &'_ slots::Store<T>, Tag>> {
        self.inner.vacant()
    }

    pub fn permit(&mut self) -> Option<slots::Permit<'_, T, Tag>> {
        self.inner.permit()
    }

    /// # Safety
    /// `back` remains live in this exact node set.
    pub unsafe fn back_mut(&mut self, back: fifo::Back<Tag>) -> &mut T {
        unsafe { self.inner.back_mut(back) }
    }

    /// # Safety
    /// `state` owns one acyclic live partition in this exact node set.
    pub unsafe fn iter<'a>(&'a self, state: &'a head::List<Tag>) -> linked::Iter<'a, T, Tag> {
        unsafe { state.iter(&self.inner) }
    }

    /// # Safety
    /// `state` owns one acyclic live partition in this exact node set.
    pub unsafe fn cursor<'a>(
        &'a self,
        state: &'a mut head::List<Tag>,
    ) -> slots::Cursor<'a, T, Tag> {
        unsafe { state.cursor(&self.inner) }
    }

    /// # Safety
    /// `state` owns one acyclic live partition in this exact node set.
    pub unsafe fn front_entry<'a, 'b>(
        &'a self,
        state: &'b mut head::List<Tag>,
    ) -> Option<slots::Entry<'a, 'b, T, Tag>> {
        unsafe { state.front_entry(&self.inner) }
    }

    /// # Safety
    /// `state` owns one acyclic live partition in this exact node set.
    pub unsafe fn pop_front(&self, state: &mut head::List<Tag>) -> Option<T> {
        Some(unsafe { state.front_entry(&self.inner) }?.pop())
    }

    /// # Safety
    /// `state` owns one acyclic live partition in this exact node set.
    pub unsafe fn queue<'a>(&'a self, state: &'a mut fifo::Fifo<Tag>) -> slots::Bound<'a, T, Tag> {
        unsafe { state.queue(&self.inner) }
    }
}

impl<T, Tag, const TRACK: bool> Drop for Arena<T, Tag, TRACK> {
    fn drop(&mut self) {
        unsafe { self.inner.drop_values() };
        let bundle = self.inner.region.capacity().bundle();
        unsafe { bundle.deallocate(self.inner.region.pointer()) };
    }
}

const _: () = assert!(mem::size_of::<Arena<u64>>() == 3 * mem::size_of::<usize>());

mod pool;
pub(in crate::depot) use pool::{PoolMutable, PoolOwner, PoolPolicy, PoolSharing};

mod queues;
pub(in crate::depot) use queues::{QueueBlock, QueueSlot};

mod slabs;
pub(in crate::depot) use slabs::{SlabCapacity, SlabLinks, SlabSlot};

mod indexed;
pub(in crate::depot) use indexed::{IndexedBlock, IndexedHook, IndexedMeta, IndexedState};

mod staged;
pub(in crate::depot) use staged::{Retired, StagedMove, StagedNode, StagedPhase, StagedState};

mod owners;
pub(crate) use owners::SlotOwner;

mod ends;
pub(crate) use ends::EndsPop;
