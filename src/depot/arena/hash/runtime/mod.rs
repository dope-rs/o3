//! Owned resident tables for keys that must outlive an individual borrow.
//! Generational slab handles retain a distinct [`Domain`] for resident hash identities.

pub use domain::Domain;

use crate::depot::arena::{
    hash::{self, runtime},
    slab::key::Handle,
};
/// The retained slot is a member of its resident hash index.
pub enum Indexed {}
pub use map::Map;

pub mod entry;

mod domain;
mod map;

#[repr(transparent)]
pub struct Registry<T, Tag = ()> {
    pub(in crate::depot::arena::hash) core: hash::Core<T, Tag>,
}

impl<T, Tag> Registry<T, Tag> {
    pub fn try_new(plan: hash::Plan) -> Result<Self, crate::alloc::Error> {
        Ok(Self {
            core: hash::Core::try_new(hash::Parts::try_new(plan.table)?)?,
        })
    }

    pub fn insert(&mut self, value: T) -> Result<Handle<runtime::Domain<Tag>>, T> {
        let Some(vacant) = hash::Slot::new(&mut self.core) else {
            return Err(value);
        };
        Ok(vacant.insert(value).key())
    }

    pub fn entry(&mut self, key: Handle<runtime::Domain<Tag>>) -> Option<entry::Entry<'_, T, Tag>> {
        Some(self.core.entry(key)?.classify())
    }

    pub fn indexed(
        &mut self,
        key: Handle<runtime::Domain<Tag>>,
    ) -> Option<entry::Live<'_, T, Tag, entry::Indexed>> {
        self.core.entry(key)?.indexed()
    }

    pub fn find(
        &mut self,
        hash: u64,
        eq: impl Fn(&T) -> bool,
    ) -> Option<entry::Live<'_, T, Tag, entry::Indexed>> {
        use crate::depot::map::raw::Probe;

        Probe::find(&mut self.core, hash, eq)
    }

    pub fn entry_at(&mut self, index: usize) -> Option<entry::Entry<'_, T, Tag>> {
        Some(self.core.entry_at(index)?.classify())
    }

    pub const fn capacity(&self) -> usize {
        self.core.capacity()
    }

    pub const fn is_empty(&self) -> bool {
        self.core.len() == 0
    }

    pub const fn is_full(&self) -> bool {
        self.core.free.is_none()
    }

    pub fn get(&self, key: Handle<runtime::Domain<Tag>>) -> Option<&T> {
        self.core.get(key)
    }

    pub fn key_value_at(&self, index: usize) -> Option<(Handle<runtime::Domain<Tag>>, &T)> {
        self.core.key_value_at(index)
    }

    pub fn key(&self, hash: u64, eq: impl Fn(&T) -> bool) -> Option<Handle<runtime::Domain<Tag>>> {
        use crate::depot::map::raw::Probe;

        Probe::find(&self.core, hash, eq)
    }
}
