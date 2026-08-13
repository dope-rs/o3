use std::{array, cell, marker, mem};

use crate::collections::{
    self,
    slab::{self, sealed},
};

mod entries;
mod guards;
mod reservations;

pub(super) use guards::Busy;

pub(super) const NONE: u32 = u32::MAX;

pub(super) trait Mode {
    const REENTRANT: bool;
    const RECYCLE_GENERATIONS: bool;
}

pub(super) struct Exclusive<const RECYCLE: bool>;

impl<const RECYCLE: bool> Mode for Exclusive<RECYCLE> {
    const REENTRANT: bool = false;
    const RECYCLE_GENERATIONS: bool = RECYCLE;
}

pub(super) struct Interior<const RECYCLE: bool>;

impl<const RECYCLE: bool> Mode for Interior<RECYCLE> {
    const REENTRANT: bool = true;
    const RECYCLE_GENERATIONS: bool = RECYCLE;
}

#[derive(Clone, Copy)]
pub(super) struct SlotIndex(u32);

impl SlotIndex {
    fn new(index: u32, capacity: usize) -> Option<Self> {
        ((index as usize) < capacity).then_some(Self(index))
    }

    pub(super) fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Free,
    Reserved,
    Occupied,
    Busy,
    Retired,
}

#[derive(Clone, Copy)]
struct Links {
    next: u32,
    prev: u32,
}

union Data<T> {
    links: Links,
    value: mem::ManuallyDrop<T>,
}

struct Slot<T, G: Copy> {
    state: cell::Cell<State>,
    generation: cell::Cell<G>,
    position: cell::Cell<u32>,
    data: cell::UnsafeCell<Data<T>>,
}

impl<T, G: Copy> Slot<T, G> {
    fn free(generation: G, next: u32, prev: u32) -> Self {
        Self {
            state: cell::Cell::new(State::Free),
            generation: cell::Cell::new(generation),
            position: cell::Cell::new(NONE),
            data: cell::UnsafeCell::new(Data {
                links: Links { next, prev },
            }),
        }
    }

    unsafe fn links(&self) -> Links {
        unsafe { (*self.data.get()).links }
    }

    unsafe fn set_links(&self, links: Links) {
        unsafe { (*self.data.get()).links = links };
    }

    unsafe fn value(&self) -> &T {
        unsafe { &*(&raw const (*self.data.get()).value).cast::<T>() }
    }

    unsafe fn value_ptr(&self) -> *mut T {
        unsafe { (&raw mut (*self.data.get()).value).cast::<T>() }
    }

    unsafe fn write_value(&self, value: T) {
        unsafe { (*self.data.get()).value = mem::ManuallyDrop::new(value) };
    }

    unsafe fn take_value(&self) -> T {
        unsafe { mem::ManuallyDrop::take(&mut (*self.data.get()).value) }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Ticket<G> {
    pub(super) index: SlotIndex,
    pub(super) generation: G,
}

pub(super) enum TryInsertError<T, E> {
    Full(T),
    Rejected(T, E),
}

pub(super) enum TryBuildError<I, T, E> {
    Full(I),
    Rejected(T, E),
}

pub(super) struct Core<T, G: sealed::GenerationState, M: Mode, const PARTITIONS: usize = 1> {
    slots: Box<[Slot<T, G>]>,
    occupied: Box<[cell::Cell<u32>]>,
    free: [cell::Cell<u32>; PARTITIONS],
    len: cell::Cell<u32>,
    _thread: crate::ThreadBound,
    mode: marker::PhantomData<M>,
}

impl<T, G: sealed::GenerationState, M: Mode, const PARTITIONS: usize> Core<T, G, M, PARTITIONS> {
    pub(super) fn try_with_capacity(
        capacity: slab::Capacity,
    ) -> Result<Self, collections::AllocationError> {
        use crate::ThreadBound;
        let () = G::VALID;
        assert!(PARTITIONS == 1 || PARTITIONS == 2);
        let raw_capacity = capacity.get();
        let boundary = if PARTITIONS == 1 {
            raw_capacity
        } else {
            raw_capacity.div_ceil(2)
        };
        let slots = capacity.try_box_with(|index| {
            let (start, end) = if index < boundary {
                (0, boundary)
            } else {
                (boundary, raw_capacity)
            };
            Slot::free(
                G::MIN,
                if index + 1 == end {
                    NONE
                } else {
                    index as u32 + 1
                },
                if index == start {
                    NONE
                } else {
                    index as u32 - 1
                },
            )
        })?;
        let occupied = capacity.try_box_with(|_| cell::Cell::new(NONE))?;
        Ok(Self {
            slots,
            occupied,
            free: array::from_fn(|partition| {
                let index = if partition == 0 { 0 } else { boundary };
                cell::Cell::new(if index < raw_capacity {
                    index as u32
                } else {
                    NONE
                })
            }),
            len: cell::Cell::new(0),
            _thread: ThreadBound::NEW,
            mode: marker::PhantomData,
        })
    }

    pub(super) fn recycle_generations(&self) -> bool {
        M::RECYCLE_GENERATIONS
    }

    pub(super) fn capacity(&self) -> usize {
        self.slots.len()
    }

    pub(super) fn partition(&self, index: SlotIndex) -> usize {
        if PARTITIONS == 1 || (index.get() as usize) < self.slots.len().div_ceil(2) {
            0
        } else {
            1
        }
    }

    pub(super) fn len(&self) -> usize {
        self.len.get() as usize
    }

    pub(super) fn available(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| slot.state.get() == State::Free)
            .count()
    }

    pub(super) fn entries(&self) -> entries::Entries<'_, T, G, M, PARTITIONS> {
        use entries::Entries;
        Entries::new(self)
    }

    pub(super) fn reservations(&self) -> reservations::Reservations<'_, T, G, M, PARTITIONS> {
        use reservations::Reservations;
        Reservations::new(self)
    }

    pub(super) fn get_mut(&mut self, index: u32, generation: G) -> Option<&mut T> {
        let slot = self.slots.get_mut(index as usize)?;
        if slot.state.get() == State::Occupied && slot.generation.get() == generation {
            Some(unsafe { &mut *slot.value_ptr() })
        } else {
            None
        }
    }

    pub(super) fn index_mut(&mut self, index: u32) -> Option<(&mut T, G)> {
        let slot = self.slots.get_mut(index as usize)?;
        if slot.state.get() == State::Occupied {
            let generation = slot.generation.get();
            Some((unsafe { &mut *slot.value_ptr() }, generation))
        } else {
            None
        }
    }

    pub(super) fn values_mut(&mut self) -> impl Iterator<Item = &mut T> {
        let len = self.len();
        let slots = self.slots.as_mut_ptr();
        let occupied = &self.occupied;
        (0..len).map(move |position| {
            let index = unsafe { occupied.get_unchecked(position) }.get();
            let slot = unsafe { &mut *slots.add(index as usize) };
            debug_assert!(slot.state.get() == State::Occupied);
            unsafe { &mut *slot.value_ptr() }
        })
    }

    pub(super) fn clear(&mut self) {
        self.entries().clear();
    }
}

impl<T, G: sealed::GenerationState, M: Mode> Core<T, G, M, 1> {
    pub(super) fn grow_to(&mut self, capacity: slab::Capacity) {
        use crate::collections::BoxSliceGrowth;
        let old_capacity = self.capacity();
        let capacity = capacity.get();
        assert!(
            capacity >= old_capacity,
            "cannot shrink slab from {old_capacity} slots to {capacity} slots"
        );
        if capacity == old_capacity {
            return;
        }

        let old_free = self.free[0].get();
        let mut slots = BoxSliceGrowth::take(&mut self.slots);
        let mut occupied = BoxSliceGrowth::take(&mut self.occupied);
        let additional = capacity - old_capacity;
        slots.reserve_exact(additional);
        occupied.reserve_exact(additional);
        occupied.resize_with(capacity, || cell::Cell::new(NONE));

        for index in old_capacity..capacity {
            slots.push(Slot::free(
                G::MIN,
                if index + 1 == capacity {
                    old_free
                } else {
                    index as u32 + 1
                },
                if index == old_capacity {
                    NONE
                } else {
                    index as u32 - 1
                },
            ));
        }

        drop(slots);
        drop(occupied);
        if old_free != NONE {
            self.reservations()
                .set_free_prev(old_free, capacity as u32 - 1);
        }
        self.free[0].set(old_capacity as u32);
    }
}

impl<T, G: sealed::GenerationState, const RECYCLE: bool> Core<T, G, Interior<RECYCLE>> {
    pub(super) fn any_or_busy(&self, mut predicate: impl FnMut(&T) -> bool) -> bool {
        for index in 0..self.slots.len() {
            match self.slots[index].state.get() {
                State::Busy => return true,
                State::Occupied => {
                    let Some(mut busy) = guards::Busy::take(self, index as u32) else {
                        return true;
                    };
                    if predicate(busy.value_mut()) {
                        return true;
                    }
                }
                State::Free | State::Reserved | State::Retired => {}
            }
        }
        false
    }

    pub(super) fn try_insert_with<R, E>(
        &self,
        value: T,
        f: impl FnOnce(Ticket<G>, &mut T) -> Result<R, E>,
    ) -> Result<(Ticket<G>, R), TryInsertError<T, E>> {
        self.try_insert_build(value, |value| value, f)
            .map_err(|error| match error {
                TryBuildError::Full(value) => TryInsertError::Full(value),
                TryBuildError::Rejected(value, error) => TryInsertError::Rejected(value, error),
            })
    }

    pub(super) fn try_insert_build<I, R, E>(
        &self,
        input: I,
        build: impl FnOnce(I) -> T,
        f: impl FnOnce(Ticket<G>, &mut T) -> Result<R, E>,
    ) -> Result<(Ticket<G>, R), TryBuildError<I, T, E>> {
        use crate::collections::slab::sealed::pending::Pending;
        let Some(ticket) = self.reservations().take_free() else {
            return Err(TryBuildError::Full(input));
        };
        let value = build(input);
        Pending::new(self, ticket).commit(value);
        let mut inserted = guards::Inserted::new(self, ticket);
        match f(ticket, inserted.value_mut()) {
            Ok(result) => {
                inserted.commit();
                Ok((ticket, result))
            }
            Err(error) => Err(TryBuildError::Rejected(inserted.rollback(), error)),
        }
    }
}

impl<T, G: sealed::GenerationState, M: Mode, const PARTITIONS: usize> Drop
    for Core<T, G, M, PARTITIONS>
{
    fn drop(&mut self) {
        use crate::collections::ClearGuard;
        ClearGuard::run(self, Self::clear);
    }
}
