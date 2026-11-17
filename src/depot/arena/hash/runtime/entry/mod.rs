pub use hash::entries::Live;
use runtime::entry;
pub use vacancy::Vacancy;

use crate::{
    base::num::bounded,
    depot::{
        arena::{
            hash::{self, errors, runtime},
            slab::key,
        },
        index::Initialized,
        map,
        queue::links,
    },
};

mod vacancy;

pub type Entry<'a, T, Tag = ()> = Live<'a, T, Tag, ()>;

pub enum Phase<'a, T, Tag = ()> {
    Indexed(Live<'a, T, Tag, Indexed>),
    Staged(Live<'a, T, Tag, Staged>),
}

impl<'a, T, Tag> Entry<'a, T, Tag> {
    pub fn into_phase(self) -> Phase<'a, T, Tag> {
        if unsafe { self.core.meta(self.index) }.bucket == hash::Bucket::NONE {
            Phase::Staged(self.with_phase())
        } else {
            Phase::Indexed(self.with_phase())
        }
    }

    pub fn unindex(self) -> Live<'a, T, Tag, Staged> {
        match self.into_phase() {
            Phase::Indexed(entry) => entry.unindex(),
            Phase::Staged(entry) => entry,
        }
    }

    pub fn remove(self) -> T {
        self.unindex().remove()
    }

    pub fn discard(self) {
        self.unindex().discard();
    }
}

/// An indexed slot with removal-only directory authority.
pub struct Occupied;
/// An indexed slot whose owner permits a retained unindexed value.
pub struct Indexed;
/// An initialized slot absent from its resident directory.
pub struct Staged;

impl<'a, T, Tag> entry::Live<'a, T, Tag, Occupied> {
    pub fn try_new(
        map: &'a mut runtime::Map<T, Tag>,
        key: key::Handle<runtime::Domain<Tag>>,
    ) -> Result<Self, errors::Lookup<Tag>> {
        Ok(map.core.try_entry(key)?.with_phase())
    }
    pub fn at(map: &'a mut runtime::Map<T, Tag>, index: usize) -> Option<Self> {
        Some(map.core.entry_at(index)?.with_phase())
    }
    pub fn from_live<C>(live: IndexedLoan<'a, T, Tag, C>) -> (Self, C) {
        let ((map, context), index) = live.into_inner();
        let index = bounded::U32::from_one(index.position());
        // SAFETY: the original Map loan proves this initialized indexed slot.
        (unsafe { Live::new(&mut map.core, index) }, context)
    }
    pub fn project(
        live: crate::depot::raw::Proof<(&Self, links::Index), Initialized<runtime::Indexed>>,
    ) -> (key::Handle<runtime::Domain<Tag>>, &T) {
        let (owner, index) = live.into_inner();
        let index = bounded::U32::from_one(index.position());
        let core = &owner.core;
        // SAFETY: the retained indexed proof bounds this position by the core's
        // capacity, so its metadata slot is initialized.
        let key = unsafe { core.key_for(index) };
        // SAFETY: the same retained proof names this initialized resident value.
        let value = unsafe { core.value(index) };
        (key, value)
    }
    #[inline]
    pub fn remove(self) -> T {
        self.unindex().remove()
    }
    pub fn discard(self) {
        self.unindex().discard();
    }

    fn unindex(self) -> entry::Live<'a, T, Tag, Staged> {
        let bucket = hash::Bucket::occupied(&self);
        // SAFETY: this indexed loan retires its directory entry. The caller
        // vacates the slot or clears its reverse link before exposing it again.
        unsafe { map::raw::Hole::retire(&mut *self.core, bucket) };
        self.with_phase()
    }
}

impl<'a, T, Tag> entry::Live<'a, T, Tag, Staged> {
    pub fn try_reserve<Q: Eq + ?Sized>(
        self,
        hash: u64,
        key: &Q,
    ) -> Result<map::Vacancy<Self, u64>, Collision<Self, Tag>>
    where
        T: AsRef<Q>,
    {
        // SAFETY: this staged value proves indexed < live <= capacity < buckets.
        unsafe { map::raw::Probe::spare(self, hash, |value| value.as_ref() == key) }
    }

    fn remove(mut self) -> T {
        Self::vacate(&mut self);
        unsafe { self.slot().assume_init_read() }
    }

    fn discard(mut self) {
        Self::vacate(&mut self);
        unsafe { self.slot().assume_init_drop() };
    }
}

impl<'a, T, Tag> entry::Live<'a, T, Tag, Indexed> {
    pub fn unindex(self) -> Live<'a, T, Tag, Staged> {
        let entry = self.with_phase::<Occupied>().unindex();
        // SAFETY: this exclusive loan owns the indexed value. Its empty reverse
        // link persists through staging and later slot reuse in this Registry.
        unsafe { entry.core.meta_mut(entry.index).bucket = hash::Bucket::NONE };
        entry
    }
}

impl<'a, T, Tag> map::Vacancy<Live<'a, T, Tag, Staged>, u64> {
    pub fn get(&self) -> &T {
        self.source().get()
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.source_mut().get_mut()
    }

    pub fn commit(self) -> Live<'a, T, Tag, Indexed> {
        let (staged, bucket, hash) = self.into_parts();
        Live::index(staged, bucket, hash)
    }
}

// SAFETY: the staged entry retains the exclusive core and its live directory.
unsafe impl<T, Tag> map::raw::Query for Live<'_, T, Tag, Staged> {
    type Payload = bounded::U32<0, { u32::MAX - 1 }>;
    type Value = T;
    type Occupied = Collision<Self, Tag>;

    fn table(&self) -> &map::raw::Table<Self::Payload> {
        &self.core.directory
    }

    fn value(entry: map::Found<&Self>) -> &T {
        let (owner, index) = entry.project();
        // SAFETY: a directory payload names a live value in this retained core.
        unsafe { owner.core.value(*index) }
    }

    fn occupied(entry: map::Found<Self>) -> Self::Occupied {
        let (owner, index) = entry.copied();
        // SAFETY: the collision retains its initialized slot and exact generation.
        let key = unsafe { owner.core.key_for(index) };
        (owner, key)
    }
}

const _: () = {
    assert!(size_of::<Entry<'static, u64>>() == 2 * size_of::<usize>());
    assert!(size_of::<entry::Live<'static, u64, (), Occupied>>() == 2 * size_of::<usize>());
    assert!(
        size_of::<Option<entry::Live<'static, u64, (), Occupied>>>()
            == size_of::<entry::Live<'static, u64, (), Occupied>>()
    );
    assert!(size_of::<entry::Live<'static, u64, (), Staged>>() == 2 * size_of::<usize>());
};

impl<'a, T, Tag, P> Live<'a, T, Tag, P> {
    pub fn key(&self) -> key::Handle<runtime::Domain<Tag>> {
        unsafe { self.core.key_for(self.index) }
    }

    pub fn get(&self) -> &T {
        unsafe { self.core.value(self.index) }
    }

    pub fn get_mut(&mut self) -> &mut T {
        unsafe { self.core.value_mut(self.index) }
    }
}

type IndexedLoan<'a, T, Tag, C> = crate::depot::raw::Proof<
    ((&'a mut runtime::Map<T, Tag>, C), links::Index),
    Initialized<runtime::Indexed>,
>;

type Collision<S, Tag> = (S, key::Handle<runtime::Domain<Tag>>);
