use std::{cell, hint, marker, mem, ops, ptr};

use crate::collections::slab::{self, recycle};

const NONE: u32 = u32::MAX;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Free,
    Reserved,
    Occupied,
    Dropping,
    Retired,
}

enum Value<T: recycle::Recycle> {
    Seed(T::Seed),
    Item(T),
    Transition,
}

struct Slot<T: recycle::Recycle> {
    owner: cell::Cell<ptr::NonNull<Group<T>>>,
    link: cell::Cell<u32>,
    state: cell::Cell<State>,
    value: cell::UnsafeCell<Value<T>>,
}

impl<T: recycle::Recycle> Slot<T> {
    fn new(index: u32, capacity: u32, seed: T::Seed) -> Self {
        Self {
            owner: cell::Cell::new(ptr::NonNull::dangling()),
            link: cell::Cell::new(if index + 1 == capacity {
                NONE
            } else {
                index + 1
            }),
            state: cell::Cell::new(State::Free),
            value: cell::UnsafeCell::new(Value::Seed(seed)),
        }
    }
}

struct Group<T: recycle::Recycle> {
    slots: Box<[Slot<T>]>,
    free: cell::Cell<u32>,
}

impl<T: recycle::Recycle> Group<T> {
    fn slot(&self, index: u32) -> &Slot<T> {
        debug_assert!((index as usize) < self.slots.len());
        // SAFETY: every live index originates in the bounded free-list.
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

/// A fixed typed slab that retains one recyclable seed per slot.
pub struct Pool<T: recycle::Recycle> {
    group: Group<T>,
}

impl<T: recycle::Recycle> Pool<T> {
    pub fn with_capacity(capacity: slab::Capacity, mut seed: impl FnMut() -> T::Seed) -> Self {
        use crate::collections::slab::Capacity;

        let capacity = capacity.raw();
        let slots =
            Capacity::new(capacity).box_with(|index| Slot::new(index as u32, capacity, seed()));
        Self {
            group: Group {
                slots,
                free: cell::Cell::new(if capacity == 0 { NONE } else { 0 }),
            },
        }
    }

    pub fn vacant_entry(&self) -> Option<VacantEntry<'_, T>> {
        Some(VacantEntry {
            pool: self,
            index: self.group.reserve()?,
            armed: true,
        })
    }
}

/// A reserved recyclable slot that still owns its seed.
#[must_use]
pub struct VacantEntry<'pool, T: recycle::Recycle> {
    pool: &'pool Pool<T>,
    index: u32,
    armed: bool,
}

impl<'pool, T: recycle::Recycle> VacantEntry<'pool, T> {
    /// Borrows the retained seed while this reservation exclusively owns it.
    pub fn seed_mut(&mut self) -> &mut T::Seed {
        let slot = self.pool.group.slot(self.index);
        debug_assert!(slot.state.get() == State::Reserved);
        // SAFETY: an armed reservation is the sole accessor to this seed until
        // it is inserted or released.
        match unsafe { &mut *slot.value.get() } {
            Value::Seed(seed) => seed,
            Value::Item(_) | Value::Transition => {
                // SAFETY: reserved slots always contain their retained seed.
                unsafe { hint::unreachable_unchecked() }
            }
        }
    }

    pub fn insert_with(mut self, build: impl FnOnce(T::Seed) -> T) -> Lease<'pool, T> {
        let slot = self.pool.group.slot(self.index);
        debug_assert!(slot.state.get() == State::Reserved);
        // SAFETY: the reservation uniquely owns this slot until it is either
        // inserted or released by VacantEntry::drop.
        let value = unsafe { &mut *slot.value.get() };
        let Value::Seed(seed) = mem::replace(value, Value::Transition) else {
            // SAFETY: only free seed slots can enter the reservation list.
            unsafe { hint::unreachable_unchecked() }
        };
        *value = Value::Item(build(seed));
        slot.owner.set(ptr::NonNull::from(&self.pool.group));
        slot.link.set(self.index);
        slot.state.set(State::Occupied);
        self.armed = false;
        Lease {
            slot: ptr::NonNull::from(slot),
            owner: marker::PhantomData,
        }
    }

    /// Builds a value transactionally, restoring the returned seed on failure.
    pub fn try_insert_with<E>(
        mut self,
        build: impl FnOnce(T::Seed) -> Result<T, (E, T::Seed)>,
    ) -> Result<Lease<'pool, T>, E> {
        let slot = self.pool.group.slot(self.index);
        debug_assert!(slot.state.get() == State::Reserved);
        // SAFETY: the reservation uniquely owns this slot until it is either
        // inserted or released by VacantEntry::drop.
        let value = unsafe { &mut *slot.value.get() };
        let Value::Seed(seed) = mem::replace(value, Value::Transition) else {
            // SAFETY: only free seed slots can enter the reservation list.
            unsafe { hint::unreachable_unchecked() }
        };
        match build(seed) {
            Ok(item) => {
                *value = Value::Item(item);
                slot.owner.set(ptr::NonNull::from(&self.pool.group));
                slot.link.set(self.index);
                slot.state.set(State::Occupied);
                self.armed = false;
                Ok(Lease {
                    slot: ptr::NonNull::from(slot),
                    owner: marker::PhantomData,
                })
            }
            Err((error, seed)) => {
                *value = Value::Seed(seed);
                self.pool.group.release(self.index);
                self.armed = false;
                Err(error)
            }
        }
    }
}

impl<T: recycle::Recycle> Drop for VacantEntry<'_, T> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let slot = self.pool.group.slot(self.index);
        // SAFETY: an armed reservation exclusively owns its slot.
        match unsafe { &*slot.value.get() } {
            Value::Seed(_) => self.pool.group.release(self.index),
            Value::Transition => slot.state.set(State::Retired),
            Value::Item(_) => {
                // SAFETY: insertion disarms the reservation before it can drop.
                unsafe { hint::unreachable_unchecked() }
            }
        }
    }
}

/// Exclusive ownership of one active value in a recyclable [`Pool`].
pub struct Lease<'pool, T: recycle::Recycle> {
    slot: ptr::NonNull<Slot<T>>,
    owner: marker::PhantomData<&'pool Pool<T>>,
}

const _: () = assert!(mem::size_of::<Lease<'static, Identity>>() == mem::size_of::<usize>());

struct Identity;

impl recycle::Recycle for Identity {
    type Seed = ();

    fn into_seed(self) {}
}

impl<T: recycle::Recycle> Lease<'_, T> {
    fn slot(&self) -> &Slot<T> {
        // SAFETY: the lease lifetime keeps the owning pool and boxed slot live.
        unsafe { self.slot.as_ref() }
    }
}

impl<T: recycle::Recycle> ops::Deref for Lease<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        let slot = self.slot();
        debug_assert!(slot.state.get() == State::Occupied);
        // SAFETY: only Lease represents an occupied slot and it is unique.
        match unsafe { &*slot.value.get() } {
            Value::Item(value) => value,
            Value::Seed(_) | Value::Transition => {
                // SAFETY: occupied slots always contain an active item.
                unsafe { hint::unreachable_unchecked() }
            }
        }
    }
}

impl<T: recycle::Recycle> ops::DerefMut for Lease<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        let slot = self.slot();
        debug_assert!(slot.state.get() == State::Occupied);
        // SAFETY: Lease is not cloneable, so active mutable access is unique.
        match unsafe { &mut *slot.value.get() } {
            Value::Item(value) => value,
            Value::Seed(_) | Value::Transition => {
                // SAFETY: occupied slots always contain an active item.
                unsafe { hint::unreachable_unchecked() }
            }
        }
    }
}

struct Retire<'slot, T: recycle::Recycle>(&'slot Slot<T>);

impl<T: recycle::Recycle> Drop for Retire<'_, T> {
    fn drop(&mut self) {
        self.0.state.set(State::Retired);
    }
}

impl<T: recycle::Recycle> Drop for Lease<'_, T> {
    fn drop(&mut self) {
        let slot = self.slot();
        debug_assert!(slot.state.get() == State::Occupied);
        slot.state.set(State::Dropping);
        // SAFETY: this lease exclusively owns the active item.
        let value = unsafe { &mut *slot.value.get() };
        let Value::Item(item) = mem::replace(value, Value::Transition) else {
            // SAFETY: occupied slots always contain an active item.
            unsafe { hint::unreachable_unchecked() }
        };
        let retire = Retire(slot);
        *value = Value::Seed(item.into_seed());
        let index = slot.link.get();
        let owner = slot.owner.get();
        // SAFETY: the lease lifetime keeps the owner live through reclamation.
        unsafe { owner.as_ref() }.release(index);
        mem::forget(retire);
    }
}
