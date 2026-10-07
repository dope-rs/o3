use std::{marker, ops};

use cycle::Policy;

use crate::depot::arena::slab::{
    self, entries,
    key::{self, cycle},
    storage::{self, Plane},
};

pub(in crate::depot::arena::slab) struct Free<
    'a,
    T,
    const MAX: u32,
    M: entries::Mode,
    const PARTITIONS: usize,
    S = storage::Heap,
    I = storage::Eager,
> where
    S: storage::Storage,
    I: storage::Init,
{
    core: &'a entries::Core<T, MAX, M, PARTITIONS, S, I>,
    slot: &'a crate::depot::raw::SlabSlot<
        T,
        key::Generation<MAX>,
        M::Busy,
        <M::Cycle as Policy>::Retired,
    >,
    index: entries::SlotIndex,
}

impl<'a, T, const MAX: u32, M: entries::Mode, const PARTITIONS: usize, S, I>
    Free<'a, T, MAX, M, PARTITIONS, S, I>
where
    S: storage::Storage,
    I: storage::Init,
{
    /// # Safety
    /// `slot` is this core's free entry at `index`, with reciprocal free neighbors.
    pub(in crate::depot::arena::slab) unsafe fn new(
        core: &'a entries::Core<T, MAX, M, PARTITIONS, S, I>,
        slot: &'a crate::depot::raw::SlabSlot<
            T,
            key::Generation<MAX>,
            M::Busy,
            <M::Cycle as Policy>::Retired,
        >,
        index: entries::SlotIndex,
    ) -> Self {
        Self { core, slot, index }
    }

    /// Selects an existing free member without reserving it; the exclusive loan
    /// keeps that membership stable until the caller consumes this proof.
    pub(in crate::depot::arena::slab) fn in_range(
        core: &'a mut entries::Core<T, MAX, M, PARTITIONS, S, I>,
        mut range: ops::Range<u32>,
        excluded: u32,
    ) -> Option<Self> {
        let core: &'a entries::Core<T, MAX, M, PARTITIONS, S, I> = core;
        let cap = core.slots.count().raw();
        range.end = range.end.min(cap);
        let split = cap.div_ceil(2);
        let groups = if range == (0..cap) {
            0..PARTITIONS
        } else if PARTITIONS == 2 && range == (0..split) {
            0..1
        } else if PARTITIONS == 2 && range == (split..cap) {
            1..2
        } else {
            let slots = core.slots.as_ref();
            let start = range.start as usize;
            let end = (range.end as usize).min(slots.len());
            if start >= end {
                return None;
            }
            return slots
                .get(start..end)?
                .iter()
                .enumerate()
                .find_map(|(offset, slot)| {
                    let index = (start + offset) as u32;
                    if index == excluded {
                        return None;
                    }
                    // SAFETY: this initialized owner slice and state retain the
                    // free member at this index under the exclusive core loan.
                    (slot.state.get() == slab::State::Free)
                        .then(|| unsafe { Self::new(core, slot, entries::SlotIndex(index)) })
                });
        };
        for head in core.free.iter().take(groups.end).skip(groups.start) {
            let mut index = head.get();
            if index == entries::NONE {
                continue;
            }
            // SAFETY: this nonempty head belongs to the original core's free chain.
            let mut slot = unsafe { core.slots.as_ref().get_unchecked(index as usize) };
            if index == excluded {
                // SAFETY: free membership selects Links and proves its successor.
                index = unsafe { slot.links().next };
                if index == entries::NONE {
                    continue;
                }
                // SAFETY: the same chain retains this distinct free successor.
                slot = unsafe { core.slots.as_ref().get_unchecked(index as usize) };
            }
            // SAFETY: this exclusive loan retains the head or its free successor.
            return Some(unsafe { Self::new(core, slot, entries::SlotIndex(index)) });
        }
        None
    }

    pub(in crate::depot::arena::slab) fn take(
        self,
    ) -> entries::Pending<'a, T, MAX, M, PARTITIONS, (), S, I> {
        // SAFETY: Free retains the active Links member and its original core.
        let crate::depot::raw::SlabLinks { next, prev } = unsafe { self.slot.links() };
        let view = self.core.slots.as_ref();
        if prev == entries::NONE {
            self.core.partition(self.index).set(next);
        } else {
            // SAFETY: this retained free entry proves its predecessor's bounds and state.
            unsafe { view.get_unchecked(prev as usize).set_next(next) };
        }
        if next != entries::NONE {
            // SAFETY: the same free membership retains its reciprocal successor.
            unsafe { view.get_unchecked(next as usize).set_prev(prev) };
        }
        self.slot.state.set(slab::State::Reserved);
        entries::Pending {
            plane: marker::PhantomData,
            core: self.core,
            slot: self.slot,
        }
    }
}
