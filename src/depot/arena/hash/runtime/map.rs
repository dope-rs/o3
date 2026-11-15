use hash::errors;
use runtime::entry;

use crate::depot::{
    arena::{
        hash::{self, runtime},
        slab::key,
    },
    map,
    queue::links,
};

#[repr(transparent)]
pub struct Map<T, Tag = ()> {
    pub(in crate::depot::arena::hash) core: hash::Core<T, Tag>,
}

impl<T, Tag> Map<T, Tag> {
    pub fn try_new(plan: hash::Plan) -> Result<Self, crate::alloc::Error> {
        Ok(Self {
            core: hash::Core::try_new(plan)?,
        })
    }

    pub const fn capacity(&self) -> usize {
        self.core.capacity()
    }

    pub const fn is_empty(&self) -> bool {
        self.core.len() == 0
    }

    pub fn get(&self, key: key::Handle<runtime::Domain<Tag>>) -> Option<&T> {
        self.core.get(key)
    }

    pub fn key_value_at(&self, index: usize) -> Option<(key::Handle<runtime::Domain<Tag>>, &T)> {
        self.core.key_value_at(index)
    }

    pub fn values(&self) -> impl Iterator<Item = &T> {
        self.core.values()
    }

    pub fn key(
        &self,
        hash: u64,
        eq: impl Fn(&T) -> bool,
    ) -> Option<key::Handle<runtime::Domain<Tag>>> {
        map::raw::Probe::find(&self.core, hash, eq)
    }
}

impl<T, Tag> Map<T, Tag> {
    pub const fn len(&self) -> usize {
        self.core.len()
    }

    pub fn key_at(&self, index: usize) -> Option<key::Handle<runtime::Domain<Tag>>> {
        self.core.key_at(index)
    }

    pub fn try_key<E>(
        &self,
        hash: u64,
        eq: impl Fn(&T) -> Result<bool, E>,
    ) -> Result<Option<key::Handle<runtime::Domain<Tag>>>, E> {
        map::raw::Probe::try_find(&self.core, hash, eq)
    }

    pub fn try_entry<E>(
        &mut self,
        hash: u64,
        eq: impl Fn(&T) -> Result<bool, E>,
    ) -> Entry<'_, T, Tag, E> {
        Ok(map::raw::Probe::try_new(&mut self.core, hash, eq)?.entry())
    }

    pub fn try_create(
        &mut self,
        hash: u64,
        eq: impl Fn(&T) -> bool,
        make: impl FnOnce(links::Index) -> T,
    ) -> Result<key::Handle<runtime::Domain<Tag>>, errors::Create<Tag>> {
        Ok(self.try_live(hash, eq, make)?.key())
    }

    /// Creates one resident and retains its occupied source loan without a key lookup.
    pub fn try_live(
        &mut self,
        hash: u64,
        eq: impl Fn(&T) -> bool,
        make: impl FnOnce(links::Index) -> T,
    ) -> Result<entry::Live<'_, T, Tag, entry::Occupied>, errors::Create<Tag>> {
        let capacity = self.capacity();
        match map::raw::Probe::new(&mut self.core, hash, eq).entry() {
            map::Entry::Occupied(entry) => Err(errors::Create::Occupied { key: entry.key() }),
            map::Entry::Vacant(None) => Err(errors::Create::Full { capacity }),
            map::Entry::Vacant(Some(entry)) => {
                let value = make(entry.index());
                Ok(entry.insert_entry(value))
            }
        }
    }

    pub fn get_mut(&mut self, key: key::Handle<runtime::Domain<Tag>>) -> Option<&mut T> {
        Some(self.core.entry(key)?.into_mut())
    }

    pub fn remove(&mut self, key: key::Handle<runtime::Domain<Tag>>) -> Option<T> {
        Some(self.core.entry(key)?.remove())
    }
}

type Entry<'a, T, Tag, E> = Result<
    map::Entry<entry::Live<'a, T, Tag, entry::Occupied>, Option<entry::Vacancy<'a, T, Tag>>>,
    E,
>;
