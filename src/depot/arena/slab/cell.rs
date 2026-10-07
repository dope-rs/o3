use std::marker;

use cycle::Policy;

use crate::depot::{
    arena::slab::{
        self, entries,
        key::{self, cycle},
        storage::Plane as _,
    },
    capacity,
    index::{Initialized, Unborrowed},
    queue::links,
};

impl<T, Tag, const MAX: u32> slab::Cell<T, Tag, MAX, false> {
    /// Reserves every slot and dense index entry transactionally.
    pub fn try_new(cap: capacity::Count) -> Result<Self, crate::alloc::Error> {
        Ok(Self {
            core: entries::Core::try_new(cap)?,
            tag: marker::PhantomData,
        })
    }
}

impl<T, Tag, const MAX: u32> slab::Cell<T, Tag, MAX, true> {
    pub(in crate::depot::arena::slab) fn try_new(
        cap: capacity::Count,
    ) -> Result<Self, crate::alloc::Error> {
        Ok(Self {
            core: entries::Core::try_new(cap)?,
            tag: marker::PhantomData,
        })
    }
}

impl<T, Tag, const MAX: u32, const RECYCLE: bool> slab::Cell<T, Tag, MAX, RECYCLE>
where
    cycle::Mode<RECYCLE>: Policy,
{
    pub fn capacity(&self) -> usize {
        self.core.slots.capacity()
    }

    /// Nonretired slots, including occupied slots and unpublished reservations.
    pub fn remaining(&self) -> capacity::Count {
        self.core.remaining()
    }

    pub fn len(&self) -> capacity::Count {
        self.core.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == capacity::Count::EMPTY
    }

    pub fn is_full(&self) -> bool {
        self.core.is_full()
    }

    pub fn vacancy(
        &self,
    ) -> Option<slab::Vacant<entries::Pending<'_, T, MAX, entries::Interior<RECYCLE>>, Tag>> {
        Some(slab::Vacant {
            entry: self.core.free()?,
            tag: marker::PhantomData,
        })
    }

    /// Reserves a slot or preserves the exact temporary or permanent refusal.
    pub fn try_vacancy(
        &self,
    ) -> Result<
        slab::Vacant<entries::Pending<'_, T, MAX, entries::Interior<RECYCLE>>, Tag>,
        slab::Vacancy,
    > {
        self.vacancy().ok_or_else(|| self.core.refusal())
    }

    pub fn update<R>(&self, key: key::Handle<Tag, MAX>, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        let mut busy = self.entry(key.parts())?;
        Some(f(busy.value_mut()))
    }
}

impl<'a, T, Tag, const MAX: u32, const RECYCLE: bool> slab::Cell<T, Tag, MAX, RECYCLE>
where
    cycle::Mode<RECYCLE>: Policy,
{
    pub fn entry(
        &'a self,
        parts: key::Parts<MAX>,
    ) -> Option<slab::Borrowed<entries::Busy<'a, T, MAX, entries::Interior<RECYCLE>>, Tag>> {
        let entry = entries::Occupied::key(&self.core, parts.index(), parts.generation())?;
        Some(slab::Borrowed::new(entry))
    }

    pub fn try_entry(&'a self, key: key::Handle<Tag, MAX>) -> Lookup<'a, T, Tag, MAX, RECYCLE> {
        Ok(slab::Borrowed::new(entries::Occupied::try_key(
            &self.core, key,
        )?))
    }
}

/// A branded slab owner with dynamically checked interior access.
pub struct Cell<T, Tag = (), const MAX: u32 = { u32::MAX }, const RECYCLE: bool = false>
where
    cycle::Mode<RECYCLE>: Policy,
{
    pub(in crate::depot) core: entries::Core<T, MAX, entries::Interior<RECYCLE>>,
    pub(in crate::depot::arena::slab) tag: marker::PhantomData<fn() -> Tag>,
}

impl<T, Tag, const MAX: u32> Cell<T, Tag, MAX, false> {
    pub fn project<R: ?Sized, F>(
        proof: crate::depot::raw::Proof<(&Self, links::Index, F), slab::Field>,
    ) -> &R
    where
        F: FnOnce(*const T) -> *const R,
    {
        let (owner, index, project) = proof.into_inner();
        let slots = owner.core.slots.as_ref();
        // SAFETY: Field retains this initialized slot and the valid disjoint field for the full loan.
        unsafe { &*project(slots.get_unchecked(index.get()).value_ptr()) }
    }
}

type Lookup<'a, T, Tag, const MAX: u32, const RECYCLE: bool> = Result<
    slab::Borrowed<entries::Busy<'a, T, MAX, entries::Interior<RECYCLE>>, Tag>,
    key::Error<Tag, MAX, (), <cycle::Mode<RECYCLE> as Policy>::Retired>,
>;

impl<'a, T, Tag, const MAX: u32, C, P> crate::depot::raw::Admit
    for crate::depot::raw::Proof<
        ((&'a slab::Cell<T, Tag, MAX, false>, C), links::Index),
        Unborrowed<P>,
    >
{
    type Output = crate::depot::raw::Proof<
        (
            (
                slab::Borrowed<entries::Busy<'a, T, MAX, entries::Interior>, Tag>,
                C,
            ),
            links::Index,
        ),
        Initialized<P>,
    >;

    fn admit(self) -> Self::Output {
        let ((owner, context), index) = self.into_inner();
        // SAFETY: the retained unborrowed predicate names this exact initialized slot.
        let slot = unsafe { owner.core.slots.as_ref().get_unchecked(index.get()) };
        let entry = entries::Occupied {
            core: &owner.core,
            slot,
        };
        // SAFETY: interior admission preserves initialization, exclusivity, and plane P.
        unsafe { crate::depot::raw::Proof::new(((slab::Borrowed::new(entry), context), index)) }
    }
}
