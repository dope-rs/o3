use std::{marker, ops};

pub use grant::Grant;

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
        self.checked_add(1)
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
    pub(in crate::depot::arena::slab) generation: G,
    pub(in crate::depot::arena::slab) value: T,
}

/// Exclusive storage whose private physical generations may recycle while
/// externally visible logical generations never do.
pub struct Exclusive<
    T,
    G: Generation,
    Tag = (),
    const PHYSICAL_MAX: u32 = { u32::MAX },
    const PARTITIONS: usize = 1,
> {
    inner: slab::Store<Stamped<T, G>, Tag, PHYSICAL_MAX, true, PARTITIONS>,
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

impl<T, G: Generation, Tag, const PHYSICAL_MAX: u32, const PARTITIONS: usize>
    Exclusive<T, G, Tag, PHYSICAL_MAX, PARTITIONS>
{
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

impl<'a, T, G: Generation, Tag, const PHYSICAL_MAX: u32, const PARTITIONS: usize>
    Exclusive<T, G, Tag, PHYSICAL_MAX, PARTITIONS>
{
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
        let stamp = slab::Slots::new(&self.inner).value_at(key.index())?;
        (stamp.generation == key.generation()).then_some(&stamp.value)
    }

    pub fn current(&'a self, index: u32) -> Option<(&'a T, Key<G, Tag>)> {
        let stamp = slab::Slots::new(&self.inner).value_at(index)?;
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
        let stamp = slab::Slots::new(&self.inner).value_at(index)?;
        Some(Key::new(index, stamp.generation))
    }
}

impl<'a, T, G: Generation, Tag, const PHYSICAL_MAX: u32, const PARTITIONS: usize>
    Exclusive<T, G, Tag, PHYSICAL_MAX, PARTITIONS>
{
    /// Validates an identity once and retains exclusive access to its live slot.
    pub fn try_occupied(
        &'a mut self,
        key: Key<G, Tag>,
    ) -> Result<OccupiedSlot<'a, T, G, Tag, PHYSICAL_MAX, PARTITIONS>, errors::Lookup<G, Tag>> {
        let inner = slab::Slots::new(&mut self.inner)
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
    ) -> Option<OccupiedSlot<'a, T, G, Tag, PHYSICAL_MAX, PARTITIONS>> {
        let inner = slab::Slots::new(&mut self.inner).entry_at(key.index())?;
        (inner.get().generation == key.generation()).then_some(external::Occupied::from(inner))
    }

    /// Borrows the current occupied owner at one physical position.
    pub fn occupied_at(
        &'a mut self,
        index: u32,
    ) -> Option<OccupiedSlot<'a, T, G, Tag, PHYSICAL_MAX, PARTITIONS>> {
        let inner = slab::Slots::new(&mut self.inner).entry_at(index)?;
        Some(external::Occupied::from(inner))
    }

    pub fn get_mut(&'a mut self, key: Key<G, Tag>) -> Option<&'a mut T> {
        Some(self.occupied(key)?.into_mut())
    }
}

impl<'a, T, G: Generation, Tag, const PHYSICAL_MAX: u32, const PARTITIONS: usize>
    OccupiedSlot<'a, T, G, Tag, PHYSICAL_MAX, PARTITIONS>
{
    pub fn key(&self) -> Key<G, Tag> {
        Key::new(self.entry.index(), self.entry.value().generation)
    }

    pub fn get(&self) -> &T {
        &self.entry.value().value
    }

    pub fn get_mut(&mut self) -> &mut T {
        &mut self.entry.value_mut().value
    }

    fn into_mut(self) -> &'a mut T {
        &mut self.entry.into_mut().value
    }

    pub fn remove(self) -> T {
        self.into_native().remove().value
    }

    /// Destroys the resident value and retires its physical identity.
    pub fn discard(self) {
        self.into_native().discard();
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
        ) = self.into_native().take();
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
}

impl<'a, T, G: Generation, Tag, const PHYSICAL_MAX: u32, const PARTITIONS: usize>
    external::Vacant<
        entries::Pending<
            'a,
            external::Stamped<T, G>,
            PHYSICAL_MAX,
            entries::Exclusive<true>,
            PARTITIONS,
        >,
        Tag,
        true,
        G,
    >
{
    pub fn key(&self) -> Key<G, Tag> {
        Key::new(self.entry.index().raw(), self.state)
    }

    pub fn commit(self, value: T) {
        let epoch = self.state;
        self.into_native().commit(Stamped {
            generation: epoch,
            value,
        });
    }

    pub fn stage(
        self,
        value: T,
    ) -> external::Staged<
        crate::mem::Lease<Busy<'a, T, G, PHYSICAL_MAX, PARTITIONS>, lease::Open>,
        Tag,
        true,
        external::Logical,
    > {
        let epoch = self.state;
        self.into_native()
            .stage(Stamped {
                generation: epoch,
                value,
            })
            .with_state(Logical)
    }
}

/// Projects the logical identity and payload while keeping physical metadata private.
pub struct Logical;

impl<'a, T, G: external::Generation, Tag, const PHYSICAL_MAX: u32, const PARTITIONS: usize>
    external::Exclusive<T, G, Tag, PHYSICAL_MAX, PARTITIONS>
{
    pub fn try_grant(
        &'a mut self,
    ) -> Result<external::Grant<'a, T, G, Tag, PHYSICAL_MAX, PARTITIONS>, slab::Vacancy> {
        external::Grant::try_new(&mut self.next, || self.inner.try_vacant().ok())
    }

    pub fn try_grant_at(
        &'a mut self,
        index: u32,
    ) -> Result<external::Grant<'a, T, G, Tag, PHYSICAL_MAX, PARTITIONS>, slab::Vacancy> {
        external::Grant::try_new(&mut self.next, || {
            slab::Slots::new(&mut self.inner).vacant_at(index)
        })
    }

    pub fn try_grant_in(
        &'a mut self,
        range: ops::Range<u32>,
    ) -> Result<external::Grant<'a, T, G, Tag, PHYSICAL_MAX, PARTITIONS>, slab::Vacancy> {
        external::Grant::try_new(&mut self.next, || {
            slab::Slots::new(&mut self.inner).vacant_in(range)
        })
    }

    /// Grants a reusable slot in `range`, excluding one physical position.
    /// Physical absence reports `Full` before logical identity exhaustion.
    /// Selection does not reserve a slot or spend an identity on failure.
    pub fn try_except(
        &'a mut self,
        range: ops::Range<u32>,
        excluded: u32,
    ) -> Result<external::Grant<'a, T, G, Tag, PHYSICAL_MAX, PARTITIONS>, slab::Vacancy> {
        let free = entries::Free::in_range(&mut self.inner.core, range, excluded)
            .ok_or(slab::Vacancy::Full)?;
        external::Grant::try_new(&mut self.next, || {
            Some(slab::Vacant::from_pending(free.take()))
        })
    }
}

mod borrowed;
mod occupied;
mod staged;
mod vacant;
pub use self::{borrowed::Borrowed, occupied::Occupied, staged::Staged, vacant::Vacant};

/// Exclusive physical phases retain the exact stamped payload and partition domain.
type Busy<'a, T, G, const MAX: u32, const N: usize> =
    entries::Busy<'a, Stamped<T, G>, MAX, entries::Exclusive<true>, N>;
type OccupiedSlot<'a, T, G, Tag, const MAX: u32, const N: usize> =
    Occupied<Busy<'a, T, G, MAX, N>, Tag, true>;
