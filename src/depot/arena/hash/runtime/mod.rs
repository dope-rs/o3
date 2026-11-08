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

pub type Registry<T, Tag = ()> = map::Store<T, Tag, ()>;

impl<T, Tag> Registry<T, Tag> {
    pub fn vacant(&mut self) -> Option<hash::Vacant<'_, T, Tag>> {
        hash::Vacant::new(&mut self.core)
    }

    pub fn insert(&mut self, value: T) -> Result<Handle<runtime::Domain<Tag>>, T> {
        let Some(vacant) = self.vacant() else {
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

    pub const fn is_full(&self) -> bool {
        self.core.free.is_none()
    }
}
