//! Generation-aware ordering with stable per-index residents.

pub use occupied::Occupied;

use crate::depot::index::Position;
mod occupied;
pub use vacant::Vacant;
mod vacant;
pub use conflict::Conflict;
mod conflict;
pub use removed::Removed;
mod removed;

use std::{marker, ptr};

use heap::{Live, keyed::Error, resident};

use crate::{
    alloc,
    depot::{arena::slab::key, capacity, heap},
};

const _: () = assert!(
    std::mem::size_of::<Min<key::Handle<(), 1>, usize, usize>>()
        == 4 * std::mem::size_of::<usize>()
);
const _: () = assert!(
    std::mem::size_of::<
        Result<Entry<'static, key::Handle<(), 1>, usize, usize>, Error<key::Handle<(), 1>>>,
    >() == std::mem::size_of::<Entry<'static, key::Handle<(), 1>, usize, usize>>()
);
const _: () = assert!(
    std::mem::size_of::<heap::Probe<resident::Occupied<'static, key::Handle<(), 1>, usize, usize>>>(
    ) == std::mem::size_of::<Entry<'static, key::Handle<(), 1>, usize, usize>>()
);

/// One-allocation minimum heap with a stable resident for every index.
#[repr(C)]
pub struct Min<K: crate::depot::raw::Address, P: Ord, R: Copy, I: Position = u32> {
    pub(in crate::depot::heap) storage: Storage<K, P, R, I>,
    key: marker::PhantomData<fn(K) -> K>,
}

#[must_use = "an entry state must be committed to change the heap"]
pub enum Entry<'a, K: crate::depot::raw::Address, P: Ord, R: Copy, I: Position = u32> {
    Vacant(resident::Vacant<'a, K, P, R, I>),
    Occupied(resident::Occupied<'a, K, P, R, I>),
    Conflict(resident::Conflict<'a, K, P, R, I>),
}

impl<K: crate::depot::raw::Address, P: Ord, R: Copy, I: Position> Entry<'_, K, P, R, I> {
    /// Initializes a missing generation or updates the current generation in place.
    pub fn update(self, new: impl FnOnce() -> (R, P), current: impl FnOnce(&mut R) -> P) {
        match self {
            Self::Vacant(mut entry) => {
                let (resident, priority) = new();
                *entry.resident_mut() = resident;
                entry.insert(priority);
            }
            Self::Occupied(mut entry) => {
                let priority = current(entry.resident_mut());
                entry.set(priority);
            }
            Self::Conflict(mut entry) => {
                let (resident, priority) = new();
                *entry.resident_mut() = resident;
                entry.set(priority);
            }
        }
    }

    /// Clears this generation while preserving an occupied conflicting generation.
    pub fn clear(self, empty: R) {
        match self {
            Self::Occupied(mut entry) => {
                *entry.resident_mut() = empty;
                entry.retire();
            }
            Self::Vacant(mut entry) => *entry.resident_mut() = empty,
            Self::Conflict(_) => {}
        }
    }
}

impl<K: crate::depot::raw::Address, P: Ord, R: Copy, I: Position> Min<K, P, R, I> {
    pub fn try_repeat(capacity: capacity::Count, value: R) -> Result<Self, heap::Error> {
        Ok(Self {
            storage: heap::Capacity::try_new(capacity)?
                .allocate()?
                .enumerate(|_| value),
            key: marker::PhantomData,
        })
    }

    pub fn entry(&mut self, key: K) -> Result<resident::Entry<'_, K, P, R, I>, Error<K>> {
        let entry = heap::Lookup::try_new(self, key)?;
        Ok(resident::Entry::from_lookup(entry))
    }

    pub fn probe(&mut self, key: K) -> heap::Probe<resident::Occupied<'_, K, P, R, I>> {
        heap::Lookup::probe(self, key, |entry| resident::Occupied { entry })
    }

    pub fn peek(&self) -> Option<(K, &P)> {
        heap::raw::Store::peek(self)
    }

    pub fn first_entry(&mut self) -> Option<resident::Occupied<'_, K, P, R, I>> {
        self.first_entry_if(|_| true)
    }

    pub fn first_entry_if(
        &mut self,
        accept: impl FnOnce(&P) -> bool,
    ) -> Option<resident::Occupied<'_, K, P, R, I>> {
        let entry = heap::Entry::first(self)?;
        if !accept(&entry.value().priority) {
            return None;
        }
        let key = entry.value().identity;
        Some(resident::Occupied {
            entry: entry.with_key(key),
        })
    }

    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub const fn len(&self) -> usize {
        self.storage.state as usize
    }
}

impl<'a, K: crate::depot::raw::Address, P: Ord, R: Copy, I: Position>
    resident::Entry<'a, K, P, R, I>
{
    fn from_lookup(entry: heap::Lookup<'a, Min<K, P, R, I>>) -> Self {
        match entry {
            heap::Lookup::Vacant(entry) => Self::Vacant(resident::Vacant { entry }),
            heap::Lookup::Occupied(entry) => Self::Occupied(resident::Occupied { entry }),
            heap::Lookup::Conflict(entry) => Self::Conflict(resident::Conflict { entry }),
        }
    }
}

impl<K: crate::depot::raw::Address, P: Ord, R: Copy, I: Position> Drop for Min<K, P, R, I> {
    fn drop(&mut self) {
        // SAFETY: Drop retains the terminal owner until its MaybeUninit backing is deallocated.
        unsafe { heap::raw::Store::destroy(self) };
    }
}

impl<K: crate::depot::raw::Address, P: Ord, R: Copy, C, I: Position>
    Live<(&'_ mut Min<K, P, R, I>, C), K, ()>
{
    /// Resolves one resident state before returning its unchanged source owner.
    pub fn with<O>(self, run: impl for<'a> FnOnce(resident::Entry<'a, K, P, R, I>) -> O) -> (C, O) {
        let (entry, context) = heap::Lookup::from_source(self);
        let output = run(resident::Entry::from_lookup(entry));
        (context, output)
    }
}

// SAFETY: the prefix and reverse map stay a bijection within this owner's resident domain.
unsafe impl<K: crate::depot::raw::Address, P: Ord, R: Copy, I: Position> heap::raw::Store
    for Min<K, P, R, I>
{
    type Key = K;
    type Priority = P;
    type Position = I;
    type Storage = heap::Capacity<heap::Stored<K, P>, I, R>;
    type Extra = ptr::NonNull<R>;

    fn precedes(left: &K, right: &K) -> bool {
        left.index() < right.index()
    }

    fn storage(&self) -> &alloc::raw::Block<Self::Storage, u32, Self::Extra> {
        &self.storage
    }

    fn storage_mut(&mut self) -> &mut alloc::raw::Block<Self::Storage, u32, Self::Extra> {
        &mut self.storage
    }
}

/// One allocation retains heap entries and their resident values together.
type Storage<K, P, R, I> =
    alloc::raw::Block<heap::Capacity<heap::Stored<K, P>, I, R>, u32, ptr::NonNull<R>>;
