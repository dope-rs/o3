use std::{array, cell, convert, marker, ops};

pub use busy::Busy;
use cycle::Policy;
pub(in crate::depot::arena::slab) use free::Free;
pub(in crate::depot) use occupied::Occupied;
pub use pending::Pending;

use crate::{
    alloc,
    base::marker::ThreadBound,
    depot::{
        arena::slab::{
            self, entries,
            key::{self, cycle},
            lookup,
            storage::{self, HeapPlane, InlinePlane, Plane, Storage},
        },
        capacity,
        index::Initialized,
        queue::links,
    },
};

mod busy;
mod free;
mod occupied;
mod pending;

pub(in crate::depot) const NONE: u32 = u32::MAX;

#[derive(Clone, Copy)]
pub(in crate::depot) struct SlotIndex(pub(in crate::depot) u32);

impl SlotIndex {
    pub(in crate::depot) fn get(self) -> u32 {
        self.0
    }
}

pub(in crate::depot) struct Core<
    T,
    const MAX: u32,
    M: Mode,
    const PARTITIONS: usize = 1,
    S = storage::Heap,
    I = storage::Eager,
> where
    S: storage::Storage,
    I: storage::Init,
{
    pub(in crate::depot) slots: <S as storage::Storage>::Plane<T, MAX, M, I>,
    free: [cell::Cell<u32>; PARTITIONS],
    len: cell::Cell<u32>,
    mode: marker::PhantomData<M>,
    _thread: ThreadBound,
}

impl<T, const MAX: u32, M: Mode, const PARTITIONS: usize, S, I> Core<T, MAX, M, PARTITIONS, S, I>
where
    S: storage::Storage,
    I: storage::Init,
{
    const VALID_PARTITIONS: () = assert!(
        PARTITIONS == 1 || PARTITIONS == 2,
        "slab partition count must be one or two"
    );

    fn links(index: u32, count: usize) -> crate::depot::raw::SlabLinks {
        let boundary = count.div_ceil(PARTITIONS);
        let index = index as usize;
        let (start, end) = if index < boundary {
            (0, boundary)
        } else {
            (boundary, count)
        };
        crate::depot::raw::SlabLinks {
            next: if index + 1 == end {
                NONE
            } else {
                index as u32 + 1
            },
            prev: if index == start {
                NONE
            } else {
                index as u32 - 1
            },
        }
    }

    fn from_slots(slots: S::Plane<T, MAX, M, I>) -> Self {
        let count = slots.capacity();
        let boundary = count.div_ceil(PARTITIONS);
        Self {
            slots,
            free: array::from_fn(|group| {
                let index = if group == 0 { 0 } else { boundary };
                cell::Cell::new(if I::LAZY || index >= count {
                    NONE
                } else {
                    index as u32
                })
            }),
            len: cell::Cell::new(0),
            mode: marker::PhantomData,
            _thread: ThreadBound::NEW,
        }
    }

    pub(in crate::depot) fn partition(&self, index: SlotIndex) -> &cell::Cell<u32> {
        if PARTITIONS == 1 || (index.get() as usize) < self.slots.capacity().div_ceil(2) {
            &self.free[0]
        } else {
            &self.free[1]
        }
    }

    pub(in crate::depot) fn len(&self) -> capacity::Count {
        capacity::Count::new(self.len.get())
    }

    pub(in crate::depot) fn clear(&mut self) {
        while let Some(len) = self.len().nonzero() {
            let pos = len.get() - 1;
            // SAFETY: the exclusive owner retains this nonempty initialized dense prefix.
            let index = unsafe {
                self.slots
                    .tail()
                    .get_unchecked(pos as usize)
                    .get()
                    .assume_init()
            };
            // SAFETY: this exclusive owner retains the initialized dense prefix,
            // including an interior guard deliberately forgotten by its caller.
            let slot = unsafe { self.slots.as_ref().get_unchecked(index as usize) };
            Busy::<_, MAX, M, PARTITIONS, (), S, I> {
                core: self,
                slot,
                plane: marker::PhantomData,
            }
            .discard();
        }
    }
}

impl<T, const MAX: u32, M: Mode, const PARTITIONS: usize, I>
    Core<T, MAX, M, PARTITIONS, storage::Heap, I>
where
    I: storage::Init,
{
    /// Each permanent retirement removes one distinct admitted heap slot.
    pub(in crate::depot) fn remaining(&self) -> capacity::Count {
        capacity::Count::new(
            self.slots
                .count()
                .raw()
                .wrapping_sub(self.slots.retired().map_or(0, cell::Cell::get)),
        )
    }

    pub(in crate::depot) fn refusal(&self) -> slab::Vacancy {
        if self.slots.count() != capacity::Count::EMPTY
            && self.remaining() == capacity::Count::EMPTY
        {
            slab::Vacancy::IdentityExhausted
        } else {
            slab::Vacancy::Full
        }
    }

    pub(in crate::depot) fn try_new(cap: capacity::Count) -> Result<Self, alloc::Error> {
        let epoch = key::Generation::<MAX>::MIN;
        let () = Self::VALID_PARTITIONS;
        let slots = <storage::Heap as Storage>::Plane::<T, MAX, M, I>::try_alloc(cap, |index| {
            let links = Self::links(index, cap.get());
            crate::depot::raw::SlabSlot::free(epoch, links.next, links.prev)
        })?;
        Ok(Self::from_slots(slots))
    }
}

impl<T, const MAX: u32, M: Mode, const PARTITIONS: usize, const N: usize, I>
    Core<T, MAX, M, PARTITIONS, storage::Inline<N>, I>
where
    I: storage::Init,
{
    pub(in crate::depot) fn new() -> Self {
        let epoch = key::Generation::<MAX>::MIN;
        let () = Self::VALID_PARTITIONS;
        let slots = <storage::Inline<N> as Storage>::Plane::<T, MAX, M, I>::inline(|index| {
            let links = Self::links(index, N);
            crate::depot::raw::SlabSlot::free(epoch, links.next, links.prev)
        });
        Self::from_slots(slots)
    }
}

impl<T, const MAX: u32, M: Mode, const PARTITIONS: usize, S, I> Drop
    for Core<T, MAX, M, PARTITIONS, S, I>
where
    S: storage::Storage,
    I: storage::Init,
{
    fn drop(&mut self) {
        // SAFETY: the consumed core owns this initialized dense prefix and its
        // values, including forgotten interior guards. No slot is reused here.
        let dense = unsafe { self.slots.tail().get_unchecked(..self.len().get()) };
        for index in dense {
            let slot = unsafe {
                self.slots
                    .as_ref()
                    .get_unchecked(index.get().assume_init() as usize)
            };
            unsafe { slot.drop_value() };
        }
    }
}

impl<T, const MAX: u32, const RECYCLE: bool, const PARTITIONS: usize, S, I>
    Core<T, MAX, Exclusive<RECYCLE>, PARTITIONS, S, I>
where
    cycle::Mode<RECYCLE>: Policy,
    S: storage::Storage,
    I: storage::Init,
{
    pub(in crate::depot) fn read<P>(
        live: crate::depot::raw::Proof<(&Self, links::Index), Initialized<P>>,
    ) -> crate::depot::raw::Proof<(&T, links::Index), Initialized<P>> {
        // SAFETY: the live slot remains initialized under this exact shared
        // allocation loan. The native value keeps that lifetime, identity, and plane.
        unsafe {
            live.map(|(core, index)| {
                let slot = core.slots.as_ref().get_unchecked(index.get());
                (slot.value(), index)
            })
        }
    }
}

mod sealed;
pub(in crate::depot) use sealed::Sealed;

impl<'a, T, const MAX: u32, M: Mode, const PARTITIONS: usize, S, I>
    Core<T, MAX, M, PARTITIONS, S, I>
where
    S: storage::Storage,
    I: storage::Init,
{
    pub(in crate::depot) fn free(&'a self) -> Option<Pending<'a, T, MAX, M, PARTITIONS, (), S, I>> {
        if let Some(pending) = self.head_at(&self.free[0]) {
            return Some(pending);
        }
        if PARTITIONS == 2
            && let Some(pending) = self.head_at(&self.free[1])
        {
            return Some(pending);
        }
        if !I::LAZY {
            return None;
        }
        let slot = crate::depot::raw::SlabSlot::free(key::Generation::<MAX>::MIN, NONE, NONE);
        slot.state.set(slab::State::Reserved);
        let slot = self.slots.push(slot)?;
        Some(Pending {
            plane: marker::PhantomData,
            core: self,
            slot,
        })
    }

    fn head_at(
        &'a self,
        head: &cell::Cell<u32>,
    ) -> Option<Pending<'a, T, MAX, M, PARTITIONS, (), S, I>> {
        let raw = head.get();
        if raw == NONE {
            return None;
        }
        // SAFETY: this retained nonempty head belongs to this core's free chain.
        let slot = unsafe { self.slots.as_ref().get_unchecked(raw as usize) };
        // SAFETY: free membership selects the Links union member.
        let next = unsafe { slot.links() }.next;
        if next != NONE {
            // SAFETY: the same chain owns this free successor; it becomes the head.
            unsafe {
                self.slots
                    .as_ref()
                    .get_unchecked(next as usize)
                    .set_prev(NONE)
            };
        }
        head.set(next);
        slot.state.set(slab::State::Reserved);
        Some(Pending {
            plane: marker::PhantomData,
            core: self,
            slot,
        })
    }

    pub(in crate::depot) fn in_range(
        &'a self,
        mut range: ops::Range<u32>,
    ) -> Option<Pending<'a, T, MAX, M, PARTITIONS, (), S, I>> {
        let cap = self.slots.capacity() as u32;
        if range == (0..cap) {
            return self.free();
        }
        if PARTITIONS == 2 {
            let split = cap.div_ceil(2);
            if range == (0..split) {
                return self.head_at(&self.free[0]);
            }
            if range == (split..cap) {
                return self.head_at(&self.free[1]);
            }
        }
        range.end = range.end.min(cap);
        let slots = self.slots.as_ref();
        let start = range.start as usize;
        let end = (range.end as usize).min(slots.len());
        slots
            .get(start..end)?
            .iter()
            .enumerate()
            .find_map(|(offset, slot)| {
                if slot.state.get() != slab::State::Free {
                    return None;
                }
                let index = SlotIndex((start + offset) as u32);
                // SAFETY: this initialized owner slice retains the slot at this
                // index, and its free membership is consumed immediately.
                let free = unsafe { Free::new(self, slot, index) };
                Some(free.take())
            })
    }

    pub(in crate::depot) fn at(
        &'a self,
        index: u32,
    ) -> Option<Pending<'a, T, MAX, M, PARTITIONS, (), S, I>> {
        if index >= self.slots.built() {
            return None;
        }
        // SAFETY: the exact core lookup admitted this initialized backing slot.
        Self::allocated(unsafe { crate::depot::raw::Proof::new((self, index)) })
    }

    pub(in crate::depot) fn allocated(
        live: crate::depot::raw::Proof<(&'a Self, u32), Initialized<entries::Allocated>>,
    ) -> Option<Pending<'a, T, MAX, M, PARTITIONS, (), S, I>> {
        let (owner, index) = live.into_inner();
        // SAFETY: the consumed proof retains this exact initialized backing slot.
        let slot = unsafe { owner.slots.as_ref().get_unchecked(index as usize) };
        let index = SlotIndex(index);
        if slot.state.get() != slab::State::Free {
            return None;
        }
        // SAFETY: this first lookup proved the physical slot's free membership.
        let free = unsafe { Free::new(owner, slot, index) };
        Some(free.take())
    }

    pub(in crate::depot) fn try_occupied(
        live: crate::depot::raw::Proof<(&'a Self, u32), Initialized<entries::Allocated>>,
    ) -> lookup::Result<Occupied<'a, T, MAX, M, PARTITIONS, S, I>, M> {
        let (core, index) = live.into_inner();
        // SAFETY: the consumed proof retains this exact allocated backing slot.
        let slot = unsafe { core.slots.as_ref().get_unchecked(index as usize) };
        match slot.state.get() {
            slab::State::Occupied => Ok(Occupied { core, slot }),
            slab::State::Free => Err(lookup::Error::Free { index }),
            slab::State::Reserved => Err(lookup::Error::Reserved { index }),
            slab::State::Busy(borrow) => Err(lookup::Error::Busy { index, borrow }),
            slab::State::Retired(retired) => Err(lookup::Error::Retired { index, retired }),
        }
    }
}

impl<'a, T, const MAX: u32, M: Mode, const PARTITIONS: usize, S, I>
    Core<T, MAX, M, PARTITIONS, S, I>
where
    S: storage::Storage,
    I: storage::Init,
{
    pub(in crate::depot) fn is_full(&'a self) -> bool {
        self.slots.built() as usize >= self.slots.capacity()
            && self.free.iter().all(|head| head.get() == NONE)
    }
}

pub trait Mode: Sealed {
    type Busy: Copy + Eq;
    type Cycle: Policy;
    const TAKEN: slab::State<Self::Busy, <Self::Cycle as Policy>::Retired>;
    const REENTRANT: bool;
}

pub struct Exclusive<const RECYCLE: bool = false>;

impl<const RECYCLE: bool> Mode for Exclusive<RECYCLE>
where
    cycle::Mode<RECYCLE>: Policy,
{
    type Busy = convert::Infallible;
    type Cycle = cycle::Mode<RECYCLE>;
    const TAKEN: slab::State<Self::Busy, <Self::Cycle as Policy>::Retired> = slab::State::Occupied;
    const REENTRANT: bool = false;
}

impl<const RECYCLE: bool> Sealed for Exclusive<RECYCLE> {}

pub struct Interior<const RECYCLE: bool = false>;

impl<const RECYCLE: bool> Mode for Interior<RECYCLE>
where
    cycle::Mode<RECYCLE>: Policy,
{
    type Busy = ();
    type Cycle = cycle::Mode<RECYCLE>;
    const TAKEN: slab::State<Self::Busy, <Self::Cycle as Policy>::Retired> = slab::State::Busy(());
    const REENTRANT: bool = true;
}

impl<const RECYCLE: bool> Sealed for Interior<RECYCLE> {}

/// The backing slot is allocated; payload occupancy remains dynamically checked.
pub struct Allocated;
