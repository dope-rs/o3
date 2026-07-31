use std::cell::{Cell, UnsafeCell};
use std::collections::TryReserveError;
use std::error::Error;
use std::fmt;
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;

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
    available: Cell<u32>,
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
        self.available.set(self.available.get() - 1);
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
        self.available.set(self.available.get() + 1);
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
    group: Box<Group<T>>,
}

const _: () = assert!(size_of::<LeaseSlab<()>>() == size_of::<usize>());

impl<T> LeaseSlab<T> {
    pub fn try_with_capacity(capacity: usize) -> Result<Self, LeaseSlabError> {
        let capacity = u32::try_from(capacity).map_err(|_| LeaseSlabError::Capacity)?;
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(capacity as usize)
            .map_err(LeaseSlabError::Reserve)?;
        slots.extend((0..capacity).map(|index| Slot::new(index, capacity)));
        let group = Box::new(Group {
            slots: slots.into_boxed_slice(),
            free: Cell::new(if capacity == 0 { NONE } else { 0 }),
            available: Cell::new(capacity),
        });
        Ok(Self { group })
    }

    pub fn capacity(&self) -> usize {
        self.group.slots.len()
    }

    pub fn available(&self) -> usize {
        self.group.available.get() as usize
    }

    pub fn len(&self) -> usize {
        self.capacity() - self.available()
    }

    pub fn is_empty(&self) -> bool {
        self.available() == self.capacity()
    }

    pub fn is_full(&self) -> bool {
        self.group.free.get() == NONE
    }

    pub fn vacant_entry(&self) -> Option<LeaseSlabVacantEntry<'_, T>> {
        Some(LeaseSlabVacantEntry {
            slab: self,
            index: self.group.reserve()?,
            armed: true,
        })
    }

    pub fn insert(&self, value: T) -> Result<SlabLease<'_, T>, T> {
        let Some(entry) = self.vacant_entry() else {
            return Err(value);
        };
        Ok(entry.insert(value))
    }
}

#[must_use]
pub struct LeaseSlabVacantEntry<'a, T> {
    slab: &'a LeaseSlab<T>,
    index: u32,
    armed: bool,
}

impl<'a, T> LeaseSlabVacantEntry<'a, T> {
    pub fn index(&self) -> u32 {
        self.index
    }

    pub fn insert(mut self, value: T) -> SlabLease<'a, T> {
        let slot = self.slab.group.slot(self.index);
        debug_assert!(slot.state.get() == State::Reserved);
        // Derive the owner after the slab is in the location borrowed by the
        // lease. A pointer cached before moving the Box into LeaseSlab would
        // lose its provenance under a later unique retag.
        slot.owner.set(NonNull::from(self.slab.group.as_ref()));
        // SAFETY: this vacant entry exclusively owns the uninitialized slot.
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

#[derive(Debug)]
pub enum LeaseSlabError {
    Capacity,
    Reserve(TryReserveError),
}

impl fmt::Display for LeaseSlabError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Capacity => formatter.write_str("lease slab capacity exceeds u32 slots"),
            Self::Reserve(error) => write!(formatter, "failed to reserve lease slab: {error}"),
        }
    }
}

impl Error for LeaseSlabError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Capacity => None,
            Self::Reserve(error) => Some(error),
        }
    }
}
