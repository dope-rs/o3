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

impl<'a, T, G: external::Generation, Tag> Borrowed<'a, T, G, Tag> {
    pub fn key(&self) -> external::Key<G, Tag> {
        external::Key::new(
            self.inner.entry.index(),
            self.inner.entry.value().generation,
        )
    }

    pub(super) fn try_epoch(self, epoch: G) -> Result<Self, errors::Lookup<G, Tag, ()>> {
        let actual = self.inner.entry.value().generation;
        if actual != epoch {
            let key = external::Key::new(self.inner.entry.index(), epoch);
            return Err(errors::Lookup::Generation { key, actual });
        }
        Ok(self)
    }

    pub fn take(self) -> T {
        self.inner.remove().value
    }

    pub fn discard(self) {
        self.inner.discard();
    }

    pub fn get_mut(&mut self) -> &mut T {
        &mut self.inner.entry.value_mut().value
    }
}

impl<'a, T, G: external::Generation, Tag>
    crate::depot::raw::Proof<(&'a shared::Storage<T, G, Tag>, u32), Initialized<entries::Allocated>>
{
    pub fn vacancy(self) -> Result<Vacancy<'a, T, G, Tag>, slab::Vacancy> {
        let owner = *self.source();
        owner.issue(|| slab::Cell::reserve(self.into_native()))
    }

    pub fn try_entry(
        self,
    ) -> Result<Borrowed<'a, T, G, Tag>, lookup::Error<(), convert::Infallible>> {
        Ok(slab::Cell::current(self.into_native())?.into_external())
    }

    pub fn try_key(self, epoch: G) -> Result<Borrowed<'a, T, G, Tag>, errors::Lookup<G, Tag, ()>> {
        let key = external::Key::new(*self.key(), epoch);
        let current = self
            .try_entry()
            .map_err(|cause| errors::Lookup::Physical { key, cause })?;
        current.try_epoch(epoch)
    }

    fn into_native(self) -> Allocated<'a, T, G, Tag> {
        // SAFETY: this projection retains the same admitted backing allocation.
        unsafe { self.map(|(owner, index)| (&owner.inner, index)) }
    }
}

/// Shared interior-mutable storage with independent non-wrapping logical identities.
pub struct Storage<T, G: external::Generation, Tag = ()> {
    pub(in crate::depot::arena::slab::external::shared) inner:
        slab::Cell<external::Stamped<T, G>, Tag, 1, true>,
    pub(in crate::depot::arena::slab::external::shared) next: cell::Cell<Option<G>>,
}

impl<T, G: external::Generation, Tag> Storage<T, G, Tag> {
    pub fn try_new(cap: capacity::Count) -> Result<Self, crate::alloc::Error> {
        let inner = slab::Cell::<external::Stamped<T, G>, Tag, 1, true>::try_new(cap)?;
        Ok(Self {
            inner,
            next: cell::Cell::new(Some(G::INITIAL)),
        })
    }

    fn issue<'a>(
        &'a self,
        acquire: impl FnOnce() -> Option<slab::Vacant<Pending<'a, T, G>, Tag>>,
    ) -> Result<Vacancy<'a, T, G, Tag>, slab::Vacancy> {
        let epoch = self.next.get().ok_or(slab::Vacancy::IdentityExhausted)?;
        let inner = acquire().ok_or(slab::Vacancy::Full)?;
        self.next.set(epoch.next());
        Ok(inner.with_state(epoch))
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn vacancy_at(&self, index: u32) -> Result<Vacancy<'_, T, G, Tag>, slab::Vacancy> {
        self.issue(|| self.inner.vacancy_at(index))
    }

    pub fn vacancy(&self) -> Result<Vacancy<'_, T, G, Tag>, slab::Vacancy> {
        self.issue(|| self.inner.vacancy())
    }

    /// Acquires one exact logical identity and retains its physical borrow.
    pub fn try_entry(
        &self,
        key: external::Key<G, Tag>,
    ) -> Result<Borrowed<'_, T, G, Tag>, errors::Lookup<G, Tag, ()>> {
        self.try_entry_at(key.index())
            .map_err(|cause| errors::Lookup::Physical { key, cause })?
            .try_epoch(key.generation())
    }

    /// Acquires the logical identity currently occupying one physical slot.
    pub fn try_entry_at(
        &self,
        index: u32,
    ) -> Result<Borrowed<'_, T, G, Tag>, lookup::Error<(), convert::Infallible>> {
        Ok(self.inner.try_entry_at(index)?.into_external())
    }
}

/// Shared slot phases preserve the same physical allocation and logical epoch owner.
type Pending<'a, T, G> = entries::Pending<'a, external::Stamped<T, G>, 1, entries::Interior<true>>;
type Busy<'a, T, G> = entries::Busy<'a, external::Stamped<T, G>, 1, entries::Interior<true>>;
pub type Vacancy<'a, T, G, Tag = ()> = external::Vacant<Pending<'a, T, G>, Tag, true, G>;
pub type Borrowed<'a, T, G, Tag = ()> = external::Borrowed<Busy<'a, T, G>, Tag, true>;

/// Physical allocation proof is retained before checking logical generation.
type Allocated<'a, T, G, Tag> = crate::depot::raw::Proof<
    (&'a slab::Cell<external::Stamped<T, G>, Tag, 1, true>, u32),
    Initialized<entries::Allocated>,
>;
