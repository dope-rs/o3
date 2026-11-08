use std::{marker, ops};

pub use grant::Grant;
pub use pinned::{Anchor, Anchored, Pinned};

use crate::{
    base::num,
    depot::{
        arena::slab::{self, entries, external, storage::Plane},
        capacity,
        index::Initialized,
        queue::links,
    },
    mem::lease,
};

pub mod errors;
mod grant;
pub(in crate::depot) mod pinned;
pub mod shared;

/// A closed non-wrapping logical generation; `None` permanently exhausts identity.
/// Issuance and comparison cannot invoke caller-provided code.
pub trait Generation: Copy + Eq + crate::depot::raw::Numeric {
    const INITIAL: Self;

    #[must_use]
    fn next(self) -> Option<Self>;
}

impl<const MAX: u32> crate::depot::raw::Numeric for num::Generation<MAX> {}
impl<const MAX: u32> crate::depot::raw::Numeric for slab::key::Generation<MAX> {}
impl crate::depot::raw::Numeric for num::Epoch {}

impl Generation for num::Epoch {
    const INITIAL: Self = Self::INITIAL;

    fn next(self) -> Option<Self> {
        self.checked_add(1)
    }
}

impl<const MAX: u32> Generation for num::Generation<MAX> {
    const INITIAL: Self = Self::INITIAL;

    fn next(self) -> Option<Self> {
        num::Generation::next(self)
    }
}

/// A typed external identity independent of the slab's private generation.
#[repr(C)]
#[derive(Debug)]
pub struct Key<G: Generation, Tag = ()> {
    generation: G,
    index: u32,
    tag: marker::PhantomData<*mut Tag>,
}

pub struct Stamped<T, G: Generation> {
    pub(in crate::depot) generation: G,
    pub(in crate::depot) value: T,
}

/// Exclusive storage with non-wrapping logical identities.
pub struct Exclusive<T, G: Generation, Tag = (), const PARTITIONS: usize = 1> {
    inner: slab::Store<Stamped<T, G>, Tag, 1, true, PARTITIONS>,
    next: Option<G>,
}

impl<G: Generation, Tag> Key<G, Tag> {
    #[must_use]
    pub const fn new(index: u32, epoch: G) -> Self {
        Self {
            generation: epoch,
            index,
            tag: marker::PhantomData,
        }
    }

    #[must_use]
    pub const fn index(self) -> u32 {
        self.index
    }

    #[must_use]
    pub const fn generation(self) -> G {
        self.generation
    }
}

impl<G: Generation, Tag> Clone for Key<G, Tag> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<G: Generation, Tag> Copy for Key<G, Tag> {}

impl<G: Generation, Tag> PartialEq for Key<G, Tag> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.generation == other.generation
    }
}

impl<G: Generation, Tag> Eq for Key<G, Tag> {}

impl<T, G: Generation, Tag, const PARTITIONS: usize> Exclusive<T, G, Tag, PARTITIONS> {
    pub fn try_new(cap: capacity::Count) -> Result<Self, crate::alloc::Error> {
        let inner = slab::Store {
            core: entries::Core::try_new(cap)?,
            tag: marker::PhantomData,
        };
        Ok(Self {
            inner,
            next: Some(G::INITIAL),
        })
    }
}

impl<T, Tag, const PARTITIONS: usize> Exclusive<T, num::Epoch, Tag, PARTITIONS> {
    pub const fn next_generation(&self) -> Option<num::Epoch> {
        self.next
    }

    /// Raises the issuance floor; either exhausted domain remains exhausted.
    pub fn advance_to(&mut self, floor: Option<num::Epoch>) {
        self.next = self
            .next
            .zip(floor)
            .map(|(current, floor)| current.max(floor));
    }
}

impl<'a, T, G: Generation, Tag, const PARTITIONS: usize> Exclusive<T, G, Tag, PARTITIONS> {
    pub fn capacity(&self) -> capacity::Count {
        self.inner.core.slots.count()
    }

    pub fn len(&self) -> capacity::Count {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == capacity::Count::EMPTY
    }

    pub fn get(&'a self, key: Key<G, Tag>) -> Option<&'a T> {
        let stamp = self.inner.value_at(key.index())?;
        (stamp.generation == key.generation()).then_some(&stamp.value)
    }

    pub fn current(&'a self, index: u32) -> Option<(&'a T, Key<G, Tag>)> {
        let stamp = self.inner.value_at(index)?;
        Some((&stamp.value, Key::new(index, stamp.generation)))
    }

    /// Projects an admitted payload while retaining its exact owner, index, and predicate.
    pub fn value<P>(
        live: crate::depot::raw::Proof<(&'a Self, links::Index), Initialized<P>>,
    ) -> crate::depot::raw::Proof<(&'a T, links::Index), Initialized<P>> {
        // SAFETY: the original external owner retains this same initialized physical store.
        let live = unsafe { live.map(|(owner, index)| (&owner.inner, index)) };
        let value = slab::Store::value(live);
        // SAFETY: the stamped payload remains borrowed from this exact live slot.
        unsafe { value.map(|(stamp, index)| (&stamp.value, index)) }
    }

    pub fn key_at(&'a self, index: u32) -> Option<Key<G, Tag>> {
        let stamp = self.inner.value_at(index)?;
        Some(Key::new(index, stamp.generation))
    }
}

impl<'a, T, G: Generation, Tag, const PARTITIONS: usize> Exclusive<T, G, Tag, PARTITIONS> {
    /// Validates an identity once and retains exclusive access to its live slot.
    pub fn try_occupied(
        &'a mut self,
        key: Key<G, Tag>,
    ) -> Result<OccupiedSlot<'a, T, G, Tag, PARTITIONS>, errors::Lookup<G, Tag>> {
        let inner = self
            .inner
            .try_entry_at(key.index())
            .map_err(|cause| errors::Lookup::Physical { key, cause })?;
        let actual = inner.get().generation;
        if actual != key.generation() {
            return Err(errors::Lookup::Generation { key, actual });
        }
        Ok(external::Occupied::from(inner))
    }

    pub fn occupied(
        &'a mut self,
        key: Key<G, Tag>,
    ) -> Option<OccupiedSlot<'a, T, G, Tag, PARTITIONS>> {
        let inner = self.inner.entry_at(key.index())?;
        (inner.get().generation == key.generation()).then_some(external::Occupied::from(inner))
    }

    /// Borrows the current occupied owner at one physical position.
    pub fn occupied_at(
        &'a mut self,
        index: u32,
    ) -> Option<OccupiedSlot<'a, T, G, Tag, PARTITIONS>> {
        let inner = self.inner.entry_at(index)?;
        Some(external::Occupied::from(inner))
    }

    pub fn get_mut(&'a mut self, key: Key<G, Tag>) -> Option<&'a mut T> {
        Some(self.occupied(key)?.into_mut())
    }
}

impl<'a, T, G: Generation, Tag, const PARTITIONS: usize> OccupiedSlot<'a, T, G, Tag, PARTITIONS> {
    pub fn from_live<C, P>(
        live: Live<'a, T, G, Tag, C, P, PARTITIONS>,
    ) -> (OccupiedSlot<'a, T, G, Tag, PARTITIONS, P>, C) {
        // SAFETY: the consumed proof retains this exact physical owner and context.
        let live =
            unsafe { live.map(|((owner, context), index)| ((&mut owner.inner, context), index)) };
        let (inner, context) = slab::Occupied::from_live(live);
        (inner.into(), context)
    }
}

impl<'a, T, G: Generation, Tag, const PARTITIONS: usize, P>
    OccupiedSlot<'a, T, G, Tag, PARTITIONS, P>
{
    pub fn key(&self) -> Key<G, Tag> {
        Key::new(self.inner.key().index(), self.inner.get().generation)
    }

    pub fn get(&self) -> &T {
        &self.inner.get().value
    }

    pub fn get_mut(&mut self) -> &mut T {
        &mut self.inner.get_mut().value
    }

    fn into_mut(self) -> &'a mut T {
        &mut self.inner.into_mut().value
    }

    pub fn remove(self) -> T {
        self.inner.remove().value
    }

    /// Destroys the resident value and retires its physical identity.
    pub fn discard(self) {
        self.inner.discard();
    }

    /// Retires the value or restores its exact physical and logical identity.
    pub fn try_remove<R, E>(
        self,
        take: impl FnOnce(T) -> Result<R, (T, E)>,
    ) -> Result<R, (Self, E)> {
        let (
            vacant,
            Stamped {
                generation: epoch,
                value,
            },
        ) = self.inner.take();
        match take(value) {
            Ok(result) => Ok(result),
            Err((value, error)) => Err((
                vacant
                    .insert(Stamped {
                        generation: epoch,
                        value,
                    })
                    .into(),
                error,
            )),
        }
    }

    pub fn project<Q>(
        live: crate::depot::raw::Proof<(&Self, links::Index), Initialized<Q>>,
    ) -> (Key<G, Tag>, &T) {
        // SAFETY: the logical phase retains the native root's exact exclusive loan.
        let live = unsafe { live.map(|(owner, index)| (&owner.inner, index)) };
        let (key, stamp) = slab::Occupied::project(live);
        (Key::new(key.index(), stamp.generation), &stamp.value)
    }
}

impl<'a, T, G: Generation, Tag, const PARTITIONS: usize> VacantSlot<'a, T, G, Tag, PARTITIONS> {
    pub fn key(&self) -> Key<G, Tag> {
        Key::new(self.entry.index().raw(), self.state)
    }

    pub fn commit(self, value: T) {
        self.stage(value).publish();
    }

    pub fn stage(self, value: T) -> StagedSlot<'a, T, G, Tag, PARTITIONS> {
        self.stage_with(|_| value)
    }

    pub fn stage_with(
        self,
        build: impl FnOnce(links::Index) -> T,
    ) -> StagedSlot<'a, T, G, Tag, PARTITIONS> {
        let epoch = self.state;
        self.into_native()
            .stage_with(|index| Stamped {
                generation: epoch,
                value: build(index),
            })
            .into_external()
    }
}

impl<'a, T, G: external::Generation, Tag, const PARTITIONS: usize>
    external::Exclusive<T, G, Tag, PARTITIONS>
{
    pub fn try_grant(
        &'a mut self,
    ) -> Result<external::Grant<'a, T, G, Tag, PARTITIONS>, slab::Vacancy> {
        external::Grant::try_new(&mut self.next, || self.inner.try_vacant().ok())
    }

    pub fn try_grant_at(
        &'a mut self,
        index: u32,
    ) -> Result<external::Grant<'a, T, G, Tag, PARTITIONS>, slab::Vacancy> {
        external::Grant::try_new(&mut self.next, || self.inner.vacant_at(index))
    }

    pub fn try_grant_in(
        &'a mut self,
        range: ops::Range<u32>,
    ) -> Result<external::Grant<'a, T, G, Tag, PARTITIONS>, slab::Vacancy> {
        external::Grant::try_new(&mut self.next, || self.inner.vacant_in(range))
    }

    /// Grants a reusable slot in `range`, excluding one physical position.
    /// Physical absence reports `Full` before logical identity exhaustion.
    /// Selection does not reserve a slot or spend an identity on failure.
    pub fn try_except(
        &'a mut self,
        range: ops::Range<u32>,
        excluded: u32,
    ) -> Result<external::Grant<'a, T, G, Tag, PARTITIONS>, slab::Vacancy> {
        let free = entries::Free::in_range(&mut self.inner.core, range, excluded)
            .ok_or(slab::Vacancy::Full)?;
        external::Grant::try_new(&mut self.next, || Some(slab::Vacant::new(free.take())))
    }
}

mod borrowed;
mod occupied;
mod staged;
mod vacant;
pub use self::{borrowed::Borrowed, occupied::Occupied, staged::Staged, vacant::Vacant};

/// Exclusive physical phases retain the exact stamped payload and partition domain.
type Live<'a, T, G, Tag, C, P, const N: usize> =
    crate::depot::raw::Live<(&'a mut Exclusive<T, G, Tag, N>, C), links::Index, P>;
type Busy<'a, T, G, const N: usize, P = ()> =
    entries::Busy<'a, Stamped<T, G>, 1, entries::Exclusive<true>, N, P>;
type Pending<'a, T, G, const N: usize> =
    entries::Pending<'a, Stamped<T, G>, 1, entries::Exclusive<true>, N>;
pub type OccupiedSlot<'a, T, G, Tag = (), const N: usize = 1, P = ()> =
    Occupied<slab::Occupied<Busy<'a, T, G, N, P>, Tag>>;
pub type VacantSlot<'a, T, G, Tag = (), const N: usize = 1> =
    Vacant<Pending<'a, T, G, N>, Tag, true, G>;
pub type StagedSlot<'a, T, G, Tag = (), const N: usize = 1> =
    Staged<crate::mem::Lease<Busy<'a, T, G, N>, lease::Open>, Tag, true>;
