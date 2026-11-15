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

/// An indexed slot whose original owner is a Map.
pub struct Occupied;
/// An initialized slot absent from its resident directory.
pub(in crate::depot::arena::hash) struct Staged;

impl<'a, T, Tag> entry::Live<'a, T, Tag, Occupied> {
    pub fn try_new(
        map: &'a mut runtime::Map<T, Tag>,
        key: key::Handle<runtime::Domain<Tag>>,
    ) -> Result<Self, errors::Lookup<Tag>> {
        map.core.try_entry(key)
    }
    pub fn at(map: &'a mut runtime::Map<T, Tag>, index: usize) -> Option<Self> {
        map.core.entry_at(index)
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
        // SAFETY: this indexed loan retires its directory entry and immediately
        // vacates the slot; no observer can read its old bucket before reuse.
        unsafe { map::raw::Hole::retire(&mut *self.core, bucket) };
        self.with_phase()
    }
}

impl<'a, T, Tag> entry::Live<'a, T, Tag, Staged> {
    fn remove(mut self) -> T {
        Self::vacate(&mut self);
        unsafe { self.slot().assume_init_read() }
    }

    fn discard(mut self) {
        Self::vacate(&mut self);
        unsafe { self.slot().assume_init_drop() };
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
