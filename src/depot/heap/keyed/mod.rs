//! Generation-aware indexed minimum heaps.

pub use occupied::Occupied;

use crate::depot::index::Position;
mod occupied;
pub use vacant::Vacant;
mod vacant;

use std::{marker, mem};

use heap::{Live, keyed};

use crate::{
    alloc,
    depot::{arena::slab::key, capacity, heap},
};

#[must_use = "an entry state must be committed to change the heap"]
pub enum Entry<'a, K: crate::depot::raw::Address, P: Ord, I: Position = u32> {
    Vacant(keyed::Vacant<'a, K, P, I>),
    Occupied(keyed::Occupied<'a, K, P, I>),
    Conflict(Conflict<K>),
}

/// A different generation currently owns the requested index.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]

pub struct Conflict<K: crate::depot::raw::Address> {
    requested: K,

    current: K,
}

/// A key outside the heap's fixed domain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Error<K: crate::depot::raw::Address> {
    pub(in crate::depot::heap) key: K,
    pub(in crate::depot::heap) capacity: usize,
}

impl<K: crate::depot::raw::Address> Conflict<K> {
    pub fn requested(&self) -> K {
        self.requested
    }
    pub fn current(&self) -> K {
        self.current
    }
}

impl<K: crate::depot::raw::Address> Error<K> {
    pub fn key(&self) -> K {
        self.key
    }
    pub const fn capacity(&self) -> usize {
        self.capacity
    }
}

const _: () = assert!(
    std::mem::size_of::<Min<key::Handle<(), 1>, usize>>() == 3 * std::mem::size_of::<usize>()
);
const _: () = assert!(
    std::mem::size_of::<Result<Entry<'static, key::Handle<(), 1>, usize>, Error<key::Handle<(), 1>>>>(
    ) == std::mem::size_of::<Entry<'static, key::Handle<(), 1>, usize>>()
);
const _: () = assert!(
    std::mem::size_of::<heap::Probe<keyed::Occupied<'static, key::Handle<(), 1>, usize>>>()
        == std::mem::size_of::<Entry<'static, key::Handle<(), 1>, usize>>()
);

/// A fixed-domain minimum heap addressed by complete generation keys.
#[repr(C)]
pub struct Min<K: crate::depot::raw::Address, P: Ord, I: Position = u32, B = ()> {
    pub(in crate::depot::heap) storage:
        alloc::raw::Block<heap::Capacity<heap::Stored<K, P>, I, ()>, u32>,
    key: Domain<K, B>,
}

impl<K: crate::depot::raw::Address, P: Ord, I: Position, B> Min<K, P, I, B> {
    pub fn first_entry(&mut self) -> Option<keyed::Occupied<'_, K, P, I, B>> {
        let entry = heap::Entry::first(self)?;
        let key = entry.value().identity;
        Some(keyed::Occupied {
            entry: entry.with_key(key),
        })
    }

    pub const fn len(&self) -> usize {
        self.storage.state as usize
    }
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<K: crate::depot::raw::Address, P: Ord, I: Position, B> Drop for Min<K, P, I, B> {
    fn drop(&mut self) {
        // SAFETY: Drop retains the terminal owner until its MaybeUninit backing is deallocated.
        unsafe { heap::raw::Store::destroy(self) };
    }
}

impl<K: crate::depot::raw::Address, P: Ord, I: Position> Min<K, P, I> {
    pub fn entry(&mut self, key: K) -> Result<keyed::Entry<'_, K, P, I>, keyed::Error<K>> {
        let entry = heap::Lookup::try_new(self, key)?;
        Ok(match entry {
            heap::Lookup::Vacant(entry) => keyed::Entry::Vacant(keyed::Vacant { entry }),
            heap::Lookup::Occupied(entry) => keyed::Entry::Occupied(keyed::Occupied { entry }),
            heap::Lookup::Conflict(entry) => keyed::Entry::Conflict(keyed::Conflict {
                requested: entry.next,
                current: entry.value().identity,
            }),
        })
    }

    pub fn probe(&mut self, key: K) -> heap::Probe<keyed::Occupied<'_, K, P, I>> {
        heap::Lookup::probe(self, key, |entry| keyed::Occupied { entry })
    }

    pub fn try_new(capacity: capacity::Count) -> Result<Self, heap::Error> {
        Ok(Self {
            storage: heap::Capacity::try_new(capacity)?.allocate()?,
            key: marker::PhantomData,
        })
    }

    pub fn try_with<R>(
        capacity: capacity::Count,
        run: impl for<'a> FnOnce(Min<K, P, I, capacity::Domain<'a>>, capacity::Domain<'a>) -> R,
    ) -> Result<R, heap::Error> {
        let storage = heap::Capacity::try_new(capacity)?.allocate()?;
        Ok(capacity::Domain::with(capacity, |domain| {
            run(
                Min {
                    storage,
                    key: marker::PhantomData,
                },
                domain,
            )
        }))
    }

    pub fn retire(&mut self, key: K) {
        if let Some(slot) = heap::Slot::new(self, key) {
            slot.retire();
        }
    }

    pub fn peek(&self) -> Option<(K, &P)> {
        heap::raw::Store::peek(self)
    }
}

impl<'a, K: crate::depot::raw::Address, P: Ord, I: Position> Min<K, P, I, capacity::Domain<'a>> {
    /// Replaces this bounded index with the supplied source identity and priority.
    pub fn replace(&mut self, key: capacity::Key<'a, K>, priority: P) {
        self.slot(key).replace(priority);
    }

    pub fn retire(&mut self, key: capacity::Key<'a, K>) {
        self.slot(key).retire();
    }

    fn slot(&mut self, key: capacity::Key<'a, K>) -> heap::Slot<'_, Self> {
        // SAFETY: this key and heap retain the same invariant capacity domain.
        let live = unsafe { crate::depot::raw::Proof::new(((self, ()), key.into_inner())) };
        heap::Slot::from_source(live).0
    }

    pub fn peek(&self) -> Option<(capacity::Key<'a, K>, &P)> {
        let entry = heap::raw::Store::first(self)?;
        // SAFETY: every stored identity entered through this exact domain.
        Some((
            capacity::Key::new(unsafe { crate::depot::raw::Proof::new(entry.identity) }),
            &entry.priority,
        ))
    }
}

impl<K: crate::depot::raw::Address, P: Ord, C, I: Position> Live<(&'_ mut Min<K, P, I>, C), K> {
    /// Replaces the admitted physical position and returns its original authority.
    pub fn replace(self, priority: P) -> C {
        let (entry, source) = heap::Slot::from_source(self);
        entry.replace(priority);
        source
    }

    /// Clears the admitted physical position and returns its original authority.
    pub fn clear(self) -> C {
        let (entry, source) = heap::Slot::from_source(self);
        entry.clear();
        source
    }
}

const _: () = {
    assert!(
        mem::size_of::<keyed::Occupied<'static, key::Handle<(), 1>, usize>>()
            == 3 * mem::size_of::<usize>()
    );
    assert!(
        mem::size_of::<Option<keyed::Occupied<'static, key::Handle<(), 1>, usize>>>()
            == mem::size_of::<keyed::Occupied<'static, key::Handle<(), 1>, usize>>()
    );
};

unsafe impl<K: crate::depot::raw::Address, P: Ord, I: Position, B> heap::raw::Store
    for Min<K, P, I, B>
{
    type Key = K;
    type Priority = P;
    type Position = I;
    type Storage = heap::Capacity<heap::Stored<K, P>, I, ()>;
    type Extra = ();

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

type Domain<K, B> = marker::PhantomData<(fn(K) -> K, B)>;

/// Heap owner and identity share one admitted capacity domain.
type Bound<'h, K, P, I> = keyed::Min<K, P, I, capacity::Domain<'h>>;

/// Admission couples the heap loan, complete bounded identity, and caller context.
type Admission<'a, 'h, K, P, I, C, F> =
    Live<(&'a mut Bound<'h, K, P, I>, C), capacity::Key<'h, K>, F>;
