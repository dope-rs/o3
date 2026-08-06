use std::{
    cell::{Cell, UnsafeCell},
    marker::PhantomData,
    mem::{MaybeUninit, size_of},
    ops::{Deref, DerefMut},
    ptr::NonNull,
};

use crate::collections::slab::SlabCapacity;

const NONE: u32 = u32::MAX;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Free,
    Reserved,
    Occupied,
    Dropping,
}

struct Slot<T> {
    owner: Cell<NonNull<Group<T>>>,
    link: Cell<u32>,
    state: Cell<State>,
    value: UnsafeCell<MaybeUninit<T>>,
}

impl<T> Slot<T> {
    fn new(index: u32, capacity: u32) -> Self {
        Self {
            owner: Cell::new(NonNull::dangling()),
            link: Cell::new(if index + 1 == capacity {
                NONE
            } else {
                index + 1
            }),
            state: Cell::new(State::Free),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }
}

struct Group<T> {
    slots: Box<[Slot<T>]>,
    free: Cell<u32>,
}

impl<T> Group<T> {
    fn slot(&self, index: u32) -> &Slot<T> {
        debug_assert!((index as usize) < self.slots.len());
        // SAFETY: every live index originates in the constructor's bounded
        // free-list. `reserve` only follows those links, while `release` only
        // reinserts an index retained by a vacant entry or occupied slot.
        unsafe { self.slots.get_unchecked(index as usize) }
    }

    fn reserve(&self) -> Option<u32> {
        let index = self.free.get();
        if index == NONE {
            return None;
        }
        let slot = self.slot(index);
        debug_assert!(slot.state.get() == State::Free);
        self.free.set(slot.link.get());
        slot.state.set(State::Reserved);
        Some(index)
    }

    fn release(&self, index: u32) {
        let slot = self.slot(index);
        debug_assert!(matches!(
            slot.state.get(),
            State::Reserved | State::Dropping
        ));
        slot.link.set(self.free.replace(index));
        slot.state.set(State::Free);
    }
}

impl<T> Drop for Group<T> {
    fn drop(&mut self) {
        for slot in &mut self.slots {
            if matches!(slot.state.get(), State::Occupied | State::Dropping) {
                // SAFETY: occupied and dropping slots contain initialized
                // values. A forgotten lease cannot run its destructor later.
                unsafe { slot.value.get_mut().assume_init_drop() };
            }
        }
    }
}

/// A fixed typed slab whose occupied slots are owned by leases.
pub struct LeaseSlab<T> {
    group: Group<T>,
}

impl<T> LeaseSlab<T> {
    pub fn with_capacity(capacity: SlabCapacity) -> Self {
        let capacity = capacity.get() as u32;
        let slots = SlabCapacity::new(capacity)
            .collect_box((0..capacity).map(|index| Slot::new(index, capacity)));
        Self {
            group: Group {
                slots,
                free: Cell::new(if capacity == 0 { NONE } else { 0 }),
            },
        }
    }

    pub fn vacant_entry(&self) -> Option<LeaseSlabVacantEntry<'_, T>> {
        Some(LeaseSlabVacantEntry {
            slab: self,
            index: self.group.reserve()?,
            armed: true,
        })
    }
}

#[must_use]
pub struct LeaseSlabVacantEntry<'a, T> {
    slab: &'a LeaseSlab<T>,
    index: u32,
    armed: bool,
}

impl<'a, T> LeaseSlabVacantEntry<'a, T> {
    pub fn insert(mut self, value: T) -> SlabLease<'a, T> {
        let slot = self.slab.group.slot(self.index);
        debug_assert!(slot.state.get() == State::Reserved);
        slot.owner.set(NonNull::from(&self.slab.group));
        // SAFETY: this entry exclusively owns the uninitialized slot, and the
        // owner pointer is derived after the slab reaches its borrowed location.
        unsafe { (*slot.value.get()).write(value) };
        slot.link.set(self.index);
        slot.state.set(State::Occupied);
        self.armed = false;
        SlabLease {
            slot: NonNull::from(slot),
            owner: PhantomData,
        }
    }
}

impl<T> Drop for LeaseSlabVacantEntry<'_, T> {
    fn drop(&mut self) {
        if self.armed {
            self.slab.group.release(self.index);
        }
    }
}

/// Exclusive ownership of one initialized `LeaseSlab` slot.
pub struct SlabLease<'a, T> {
    slot: NonNull<Slot<T>>,
    owner: PhantomData<&'a LeaseSlab<T>>,
}

const _: () = assert!(size_of::<SlabLease<'static, ()>>() == size_of::<usize>());

impl<T> SlabLease<'_, T> {
    fn slot(&self) -> &Slot<T> {
        // SAFETY: the lease lifetime keeps the owning slab and its boxed slot
        // storage alive.
        unsafe { self.slot.as_ref() }
    }
}

impl<T> Deref for SlabLease<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        let slot = self.slot();
        debug_assert!(slot.state.get() == State::Occupied);
        // SAFETY: occupied slots contain one initialized value, and a lease is
        // the only public handle capable of accessing its slot.
        unsafe { (*slot.value.get()).assume_init_ref() }
    }
}

impl<T> DerefMut for SlabLease<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        let slot = self.slot();
        debug_assert!(slot.state.get() == State::Occupied);
        // SAFETY: SlabLease is not cloneable, so mutable lease access is unique.
        unsafe { (*slot.value.get()).assume_init_mut() }
    }
}

struct Reclaim<T> {
    owner: NonNull<Group<T>>,
    index: u32,
}

impl<T> Drop for Reclaim<T> {
    fn drop(&mut self) {
        // SAFETY: the lease lifetime keeps `owner` live through reclamation.
        unsafe { self.owner.as_ref() }.release(self.index);
    }
}

impl<T> Drop for SlabLease<'_, T> {
    fn drop(&mut self) {
        let slot = self.slot();
        debug_assert!(slot.state.get() == State::Occupied);
        slot.state.set(State::Dropping);
        let index = slot.link.get();
        let owner = slot.owner.get();
        let _reclaim = Reclaim { owner, index };
        // SAFETY: the occupied slot is initialized and this lease owns it.
        unsafe { (*slot.value.get()).assume_init_drop() };
    }
}
