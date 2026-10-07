use std::{cell, convert};

use crate::depot::{
    arena::slab::{
        self, entries,
        external::{self, errors, shared},
        lookup,
    },
    capacity,
    index::Initialized,
};

pub struct Cursor<G>(cell::Cell<Option<G>>);

impl<'a, T, G: external::Generation, Tag, const PHYSICAL_MAX: u32>
    Borrowed<'a, T, G, Tag, PHYSICAL_MAX>
{
    pub fn key(&self) -> external::Key<G, Tag> {
        external::Key::new(self.entry.index(), self.entry.value().generation)
    }

    pub(super) fn try_epoch(self, epoch: G) -> Result<Self, errors::Lookup<G, Tag, ()>> {
        let actual = self.entry.value().generation;
        if actual != epoch {
            let key = external::Key::new(self.entry.index(), epoch);
            return Err(errors::Lookup::Generation { key, actual });
        }
        Ok(self)
    }

    pub fn take(self) -> T {
        self.into_native().remove().value
    }

    pub fn discard(self) {
        self.into_native().discard();
    }

    pub fn get_mut(&mut self) -> &mut T {
        &mut self.entry.value_mut().value
    }
}

impl<G: external::Generation> Cursor<G> {
    pub(super) fn new() -> Self {
        Self(cell::Cell::new(Some(G::INITIAL)))
    }

    pub(super) fn vacancy<'a, T, Tag, const PHYSICAL_MAX: u32>(
        &self,
        inner: Option<slab::Vacant<Pending<'a, T, G, PHYSICAL_MAX>, Tag>>,
    ) -> Result<Vacancy<'a, T, G, Tag, PHYSICAL_MAX>, slab::Vacancy> {
        let Some(epoch) = self.0.get() else {
            return Err(slab::Vacancy::IdentityExhausted);
        };
        let Some(inner) = inner else {
            return Err(slab::Vacancy::Full);
        };
        self.0.set(epoch.next());
        Ok(inner.with_state(epoch))
    }
}

impl<'a, T, G: external::Generation, Tag, const MAX: u32>
    crate::depot::raw::Proof<
        (&'a shared::Storage<T, G, Tag, MAX>, u32),
        Initialized<entries::Allocated>,
    >
{
    pub fn vacancy(self) -> Result<Vacancy<'a, T, G, Tag, MAX>, slab::Vacancy> {
        let cursor = &self.source().next;
        cursor.vacancy(slab::Slots::reserve(self.into_native()))
    }

    pub fn try_entry(
        self,
    ) -> Result<Borrowed<'a, T, G, Tag, MAX>, lookup::Error<(), convert::Infallible>> {
        Ok(slab::Slots::current(self.into_native())?.into_external())
    }

    pub fn try_key(
        self,
        epoch: G,
    ) -> Result<Borrowed<'a, T, G, Tag, MAX>, errors::Lookup<G, Tag, ()>> {
        let key = external::Key::new(*self.key(), epoch);
        let current = self
            .try_entry()
            .map_err(|cause| errors::Lookup::Physical { key, cause })?;
        current.try_epoch(epoch)
    }

    fn into_native(self) -> Allocated<'a, T, G, Tag, MAX> {
        // SAFETY: this projection retains the same admitted backing allocation.
        unsafe { self.map(|(owner, index)| (&owner.inner, index)) }
    }
}

/// Shared interior-mutable storage with independent non-wrapping logical identities.
pub struct Storage<T, G: external::Generation, Tag = (), const PHYSICAL_MAX: u32 = { u32::MAX }> {
    pub(in crate::depot::arena::slab::external::shared) inner:
        slab::Cell<external::Stamped<T, G>, Tag, PHYSICAL_MAX, true>,
    pub(in crate::depot::arena::slab::external::shared) next: shared::Cursor<G>,
}

impl<T, G: external::Generation, Tag, const PHYSICAL_MAX: u32> Storage<T, G, Tag, PHYSICAL_MAX> {
    pub fn try_new(cap: capacity::Count) -> Result<Self, crate::alloc::Error> {
        let inner = slab::Cell::<external::Stamped<T, G>, Tag, PHYSICAL_MAX, true>::try_new(cap)?;
        Ok(Self {
            inner,
            next: shared::Cursor::new(),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn vacancy_at(
        &self,
        index: u32,
    ) -> Result<Vacancy<'_, T, G, Tag, PHYSICAL_MAX>, slab::Vacancy> {
        self.next
            .vacancy(slab::Slots::new(&self.inner).vacancy_at(index))
    }

    pub fn vacancy(&self) -> Result<Vacancy<'_, T, G, Tag, PHYSICAL_MAX>, slab::Vacancy> {
        self.next.vacancy(self.inner.vacancy())
    }

    /// Acquires one exact logical identity and retains its physical borrow.
    pub fn try_entry(
        &self,
        key: external::Key<G, Tag>,
    ) -> Result<Borrowed<'_, T, G, Tag, PHYSICAL_MAX>, errors::Lookup<G, Tag, ()>> {
        self.try_entry_at(key.index())
            .map_err(|cause| errors::Lookup::Physical { key, cause })?
            .try_epoch(key.generation())
    }

    /// Acquires the logical identity currently occupying one physical slot.
    pub fn try_entry_at(
        &self,
        index: u32,
    ) -> Result<Borrowed<'_, T, G, Tag, PHYSICAL_MAX>, lookup::Error<(), convert::Infallible>> {
        Ok(slab::Slots::new(&self.inner)
            .try_entry_at(index)?
            .into_external())
    }
}

/// Shared slot phases preserve the same physical allocation and logical epoch owner.
type Pending<'a, T, G, const MAX: u32> =
    entries::Pending<'a, external::Stamped<T, G>, MAX, entries::Interior<true>>;
type Busy<'a, T, G, const MAX: u32> =
    entries::Busy<'a, external::Stamped<T, G>, MAX, entries::Interior<true>>;
type Vacancy<'a, T, G, Tag, const MAX: u32> =
    external::Vacant<Pending<'a, T, G, MAX>, Tag, true, G>;
type Borrowed<'a, T, G, Tag, const MAX: u32> = external::Borrowed<Busy<'a, T, G, MAX>, Tag, true>;

/// Physical allocation proof is retained before checking logical generation.
type Allocated<'a, T, G, Tag, const MAX: u32> = crate::depot::raw::Proof<
    (&'a slab::Cell<external::Stamped<T, G>, Tag, MAX, true>, u32),
    Initialized<entries::Allocated>,
>;
