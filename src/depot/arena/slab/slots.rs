use std::{marker, ops};

use cycle::Policy;

use crate::depot::{
    arena::slab::{
        self, entries,
        key::{self, cycle},
        lookup,
        storage::Plane as _,
    },
    capacity,
    capacity::Physical,
    index::Initialized,
};

/// Physical slot access retaining the exact source borrow.
#[repr(transparent)]
pub struct Slots<B> {
    source: B,
}

impl<B> Slots<B> {
    pub fn new(source: B) -> Self {
        Self { source }
    }
}

impl<'a, T, Tag, const MAX: u32, const RECYCLE: bool, const PARTITIONS: usize>
    Slots<&'a slab::Store<T, Tag, MAX, RECYCLE, PARTITIONS>>
where
    cycle::Mode<RECYCLE>: Policy,
{
    /// Recovers the exact current generation-bearing key for one occupied
    /// direct-address slot.
    pub fn key_at(self, index: usize) -> Option<key::Handle<Tag, MAX>> {
        if index > u32::MAX as usize {
            return None;
        }
        let index = index as u32;
        let entry = entries::Occupied::at(&self.source.core, index)?;
        Some(key::Handle::new(index, entry.generation()))
    }

    /// Borrows the current value at a physical slot through the owner's shared loan.
    /// Checks occupancy without matching a generation.
    pub fn value_at(self, index: u32) -> Option<&'a T> {
        Some(entries::Occupied::at(&self.source.core, index)?.read())
    }
}

impl<'a, T, Tag, const MAX: u32> Slots<&'a mut slab::Store<T, Tag, MAX>> {
    /// Borrows each usable physical slot once, retaining disjoint entry proofs.
    pub fn entries(self) -> impl Iterator<Item = slab::Entry<'a, T, Tag, MAX>> {
        let core = &self.source.core;
        core.slots
            .as_ref()
            .iter()
            .enumerate()
            .filter_map(move |(index, slot)| {
                match slot.state.get() {
                    slab::State::Occupied => Some(slab::Entry::Occupied(slab::Occupied {
                        entry: entries::Busy {
                            core,
                            slot,
                            plane: marker::PhantomData,
                        },
                        tag: marker::PhantomData,
                    })),
                    slab::State::Free => {
                        // SAFETY: this unique visit proved the original slot's free membership.
                        let free = unsafe {
                            entries::Free::new(core, slot, entries::SlotIndex(index as u32))
                        };
                        Some(slab::Entry::Vacant(slab::Vacant::from_pending(free.take())))
                    }
                    slab::State::Reserved | slab::State::Retired(()) => None,
                }
            })
    }
}

impl<'a, T, Tag, const MAX: u32, const RECYCLE: bool, const PARTITIONS: usize>
    Slots<&'a mut slab::Store<T, Tag, MAX, RECYCLE, PARTITIONS>>
where
    cycle::Mode<RECYCLE>: Policy,
{
    pub fn vacant_at(
        self,
        index: u32,
    ) -> Option<
        slab::Vacant<entries::Pending<'a, T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS>, Tag>,
    > {
        let pending = self.source.core.at(index)?;
        Some(slab::Vacant::from_pending(pending))
    }

    /// Reserves the first reusable slot in `range` without leaving it.
    /// Retired, occupied, and reserved slots are skipped.
    pub fn vacant_in(
        self,
        range: ops::Range<u32>,
    ) -> Option<
        slab::Vacant<entries::Pending<'a, T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS>, Tag>,
    > {
        let pending = self.source.core.in_range(range)?;
        Some(slab::Vacant::from_pending(pending))
    }

    pub fn entry_at(
        self,
        index: u32,
    ) -> Option<
        slab::Occupied<entries::Busy<'a, T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS>, Tag>,
    > {
        let entry = entries::Occupied::at(&self.source.core, index)?;
        Some(slab::Occupied::new(entry))
    }

    pub fn try_entry_at(self, index: u32) -> Occupied<'a, T, Tag, MAX, RECYCLE, PARTITIONS> {
        let entry = entries::Occupied::try_at(&self.source.core, index)?;
        Ok(slab::Occupied::new(entry))
    }
}

impl<'a, T, Tag, const MAX: u32> Slots<&'a slab::Store<T, Tag, MAX>> {
    pub(in crate::depot) fn contains<'h>(proof: PhysicalSlot<'a, 'h, T, Tag, MAX>) -> bool {
        let (source, key) = proof.into_inner();
        let slots = source.core.slots.as_ref();
        // SAFETY: the retained bound selects this eager owner's initialized extent.
        let slot = unsafe { slots.get_unchecked(key.as_inner().index() as usize) };
        slot.state.get() == slab::State::Occupied
            && slot.generation.get() == key.as_inner().generation()
    }
}

impl<'a, T, Tag, const MAX: u32, const RECYCLE: bool> Slots<&'a slab::Cell<T, Tag, MAX, RECYCLE>>
where
    cycle::Mode<RECYCLE>: Policy,
{
    pub(in crate::depot::arena::slab) fn vacancy_at(
        self,
        index: u32,
    ) -> Option<slab::Vacant<entries::Pending<'a, T, MAX, entries::Interior<RECYCLE>>, Tag>> {
        let pending = self.source.core.at(index)?;
        Some(slab::Vacant {
            entry: pending,
            tag: marker::PhantomData,
        })
    }

    pub(in crate::depot::arena::slab) fn reserve(
        live: crate::depot::raw::Proof<
            (&'a slab::Cell<T, Tag, MAX, RECYCLE>, u32),
            Initialized<entries::Allocated>,
        >,
    ) -> Option<slab::Vacant<entries::Pending<'a, T, MAX, entries::Interior<RECYCLE>>, Tag>> {
        // SAFETY: this maps the retained Cell directly to its original core.
        let live = unsafe { live.map(|(owner, index)| (&owner.core, index)) };
        let pending = entries::Core::allocated(live)?;
        Some(slab::Vacant {
            entry: pending,
            tag: marker::PhantomData,
        })
    }

    pub(in crate::depot::arena::slab) fn current(
        live: crate::depot::raw::Proof<
            (&'a slab::Cell<T, Tag, MAX, RECYCLE>, u32),
            Initialized<entries::Allocated>,
        >,
    ) -> Borrowed<'a, T, Tag, MAX, RECYCLE> {
        // SAFETY: this maps the retained Cell directly to its original core.
        let live = unsafe { live.map(|(owner, index)| (&owner.core, index)) };
        Ok(slab::Borrowed::new(entries::Core::try_occupied(live)?))
    }

    pub fn entry_at(
        self,
        index: u32,
    ) -> Option<slab::Borrowed<entries::Busy<'a, T, MAX, entries::Interior<RECYCLE>>, Tag>> {
        let entry = entries::Occupied::at(&self.source.core, index)?;
        Some(slab::Borrowed::new(entry))
    }

    pub(in crate::depot::arena::slab) fn try_entry_at(
        self,
        index: u32,
    ) -> Borrowed<'a, T, Tag, MAX, RECYCLE> {
        Ok(slab::Borrowed::new(entries::Occupied::try_at(
            &self.source.core,
            index,
        )?))
    }
}

type Occupied<'a, T, Tag, const MAX: u32, const RECYCLE: bool, const PARTITIONS: usize> =
    lookup::Result<
        slab::Occupied<entries::Busy<'a, T, MAX, entries::Exclusive<RECYCLE>, PARTITIONS>, Tag>,
        entries::Exclusive<RECYCLE>,
    >;

type PhysicalSlot<'a, 'h, T, Tag, const MAX: u32> = crate::depot::raw::Proof<
    (
        &'a slab::Store<T, Tag, MAX>,
        capacity::Key<'h, key::Handle<Tag, MAX>>,
    ),
    Physical,
>;

type Borrowed<'a, T, Tag, const MAX: u32, const RECYCLE: bool> = lookup::Result<
    slab::Borrowed<entries::Busy<'a, T, MAX, entries::Interior<RECYCLE>>, Tag>,
    entries::Interior<RECYCLE>,
>;
