use std::marker;

use runtime::entry;
pub use vacancy::Vacancy;

use crate::{
    base::num::bounded,
    depot::{
        arena::{
            hash::{self, Position, errors, runtime},
            slab::key,
        },
        index::Initialized,
        map,
        queue::links,
    },
};

/// An initialized resident entry carrying its directory membership phase.
pub struct Live<'a, T, Tag = (), P = ()> {
    pub(in crate::depot) raw: hash::Entry<'a, T, Tag>,
    pub(in crate::depot) phase: marker::PhantomData<fn(P) -> P>,
}

mod vacancy;

#[must_use]
pub enum Entry<'a, T, Tag = ()> {
    Indexed(Live<'a, T, Tag, Indexed>),
    Staged(Live<'a, T, Tag, Staged>),
}

impl<'a, T, Tag> Entry<'a, T, Tag> {
    pub fn key(&self) -> key::Handle<runtime::Domain<Tag>> {
        match self {
            Self::Indexed(entry) => entry.key(),
            Self::Staged(entry) => entry.key(),
        }
    }

    pub fn get(&self) -> &T {
        match self {
            Self::Indexed(entry) => entry.get(),
            Self::Staged(entry) => entry.get(),
        }
    }

    pub fn get_mut(&mut self) -> &mut T {
        match self {
            Self::Indexed(entry) => entry.get_mut(),
            Self::Staged(entry) => entry.get_mut(),
        }
    }

    pub fn into_mut(self) -> &'a mut T {
        match self {
            Self::Indexed(entry) => entry.into_mut(),
            Self::Staged(entry) => entry.into_mut(),
        }
    }

    pub fn remove(self) -> T {
        match self {
            Self::Indexed(entry) => entry.remove(),
            Self::Staged(entry) => entry.remove(),
        }
    }

    pub fn discard(self) {
        match self {
            Self::Indexed(entry) => entry.unindex().discard(),
            Self::Staged(entry) => entry.discard(),
        }
    }
}

/// An indexed slot whose original owner is a Map.
pub struct Occupied;
/// An initialized slot admitted to its resident directory.
pub struct Indexed;
/// An initialized slot absent from its resident directory.
pub struct Staged;

impl<'a, T, Tag> entry::Live<'a, T, Tag, Occupied> {
    pub fn try_new(
        map: &'a mut runtime::Map<T, Tag>,
        key: key::Handle<runtime::Domain<Tag>>,
    ) -> Result<Self, errors::Lookup<Tag>> {
        Ok(Position::try_live(&mut map.core, key)?
            .into_entry()
            .with_phase())
    }
    pub fn at(map: &'a mut runtime::Map<T, Tag>, index: usize) -> Option<Self> {
        Some(map.core.entry_at(index)?.with_phase())
    }
    pub fn from_live<C>(live: IndexedLoan<'a, T, Tag, C>) -> (Self, C) {
        let ((map, context), index) = live.into_inner();
        let index = bounded::U32::from_one(index.position());
        // SAFETY: the original Map loan proves this initialized indexed slot.
        (
            unsafe { hash::Entry::new(&mut map.core, index) }.with_phase(),
            context,
        )
    }
    pub fn project(
        live: crate::depot::raw::Proof<(&Self, links::Index), Initialized<runtime::Indexed>>,
    ) -> (key::Handle<runtime::Domain<Tag>>, &T) {
        let (owner, index) = live.into_inner();
        let index = bounded::U32::from_one(index.position());
        let core = &owner.raw.core;
        // SAFETY: the retained indexed proof bounds this position by the core's
        // capacity, so its metadata slot is initialized.
        let key = unsafe { core.key_for(index) };
        // SAFETY: the same retained proof names this initialized resident value.
        let value = unsafe { core.value(index) };
        (key, value)
    }
    pub fn remove(self) -> T {
        self.with_phase::<Indexed>().remove()
    }
    pub fn discard(self) {
        self.with_phase::<Indexed>().unindex().discard();
    }
}

impl<'a, T, Tag> entry::Live<'a, T, Tag, Indexed> {
    pub fn unindex(mut self) -> entry::Live<'a, T, Tag, Staged> {
        let bucket = hash::Bucket::occupied(&self);
        let owner = &mut self.raw;
        unsafe { owner.core.meta_mut(owner.index).bucket = hash::Bucket::NONE };
        unsafe { map::raw::Hole::retire(&mut *owner.core, bucket) };
        self.with_phase()
    }
    pub fn remove(self) -> T {
        self.unindex().remove()
    }
}

impl<'a, T, Tag> entry::Live<'a, T, Tag, Staged> {
    pub fn try_reserve<Q: Eq + ?Sized>(self, hash: u64, key: &Q) -> Admission<Self, Tag>
    where
        T: AsRef<Q>,
    {
        map::raw::Spare::try_reserve(self, hash, key)
    }
    pub fn remove(mut self) -> T {
        hash::Entry::vacate(&mut self);
        unsafe { self.raw.slot().assume_init_read() }
    }

    pub(in crate::depot) fn discard(mut self) {
        hash::Entry::vacate(&mut self);
        unsafe { self.raw.slot().assume_init_drop() };
    }
}

const _: () = {
    assert!(size_of::<entry::Live<'static, u64, (), Occupied>>() == 2 * size_of::<usize>());
    assert!(
        size_of::<Option<entry::Live<'static, u64, (), Occupied>>>()
            == size_of::<entry::Live<'static, u64, (), Occupied>>()
    );
    assert!(size_of::<entry::Live<'static, u64, (), Staged>>() == 2 * size_of::<usize>());
};

impl<'a, T, Tag> map::Vacancy<entry::Live<'a, T, Tag, entry::Staged>, u64> {
    pub fn get(&self) -> &T {
        self.source().get()
    }
    pub fn get_mut(&mut self) -> &mut T {
        self.source_mut().get_mut()
    }
    pub fn commit(self) -> entry::Live<'a, T, Tag, entry::Indexed> {
        let (staged, bucket, hash) = self.into_parts();
        hash::Entry::index(staged, bucket, hash)
    }
}
const _: () = {
    assert!(
        size_of::<map::Vacancy<entry::Live<'static, u64, (), entry::Staged>, u64>>()
            == 4 * size_of::<usize>()
    );
};

impl<'a, T, Tag, P> Live<'a, T, Tag, P> {
    pub(in crate::depot) fn with_phase<N>(self) -> Live<'a, T, Tag, N> {
        self.raw.with_phase()
    }

    pub fn key(&self) -> key::Handle<runtime::Domain<Tag>> {
        unsafe { self.raw.core.key_for(self.raw.index) }
    }

    pub fn get(&self) -> &T {
        unsafe { self.raw.core.value(self.raw.index) }
    }

    pub fn get_mut(&mut self) -> &mut T {
        unsafe { self.raw.core.value_mut(self.raw.index) }
    }

    pub fn into_mut(self) -> &'a mut T {
        self.raw.into_mut()
    }
}

type IndexedLoan<'a, T, Tag, C> = crate::depot::raw::Proof<
    ((&'a mut runtime::Map<T, Tag>, C), links::Index),
    Initialized<runtime::Indexed>,
>;

type Admission<S, Tag> = Result<map::Vacancy<S, u64>, (S, key::Handle<runtime::Domain<Tag>>)>;

// SAFETY: the staged entry retains the exclusive core and its live directory.
unsafe impl<T, Tag> map::raw::Query for entry::Live<'_, T, Tag, entry::Staged> {
    type Payload = bounded::U32<0, { u32::MAX - 1 }>;
    type Value = T;
    type Occupied = (Self, key::Handle<runtime::Domain<Tag>>);
    fn table(&self) -> &map::raw::Table<Self::Payload> {
        &self.raw.core.directory
    }
    fn value(entry: map::Found<&Self>) -> &T {
        let (owner, index) = entry.project();
        unsafe { owner.raw.core.value(*index) }
    }
    fn occupied(entry: map::Found<Self>) -> Self::Occupied {
        let (owner, index) = entry.copied();
        let key = unsafe { owner.raw.core.key_for(index) };
        (owner, key)
    }
}
// SAFETY: the live unindexed slot proves indexed < live <= capacity < buckets.
unsafe impl<T, Tag> map::raw::Spare for entry::Live<'_, T, Tag, entry::Staged> {}
