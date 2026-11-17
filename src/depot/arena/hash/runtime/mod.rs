//! Owned resident tables for keys that must outlive an individual borrow.
//! Generational slab handles retain a distinct [`Domain`] for resident hash identities.

pub use domain::Domain;

use crate::depot::arena::{hash, slab::key::Handle};

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

    pub fn entry(&mut self, key: Handle<Domain<Tag>>) -> Option<entry::Entry<'_, T, Tag>> {
        self.core.entry(key)
    }

    pub fn entry_at(&mut self, index: usize) -> Option<entry::Entry<'_, T, Tag>> {
        self.core.entry_at(index)
    }

    pub fn find(
        &mut self,
        hash: u64,
        eq: impl Fn(&T) -> bool,
    ) -> Option<entry::Live<'_, T, Tag, entry::Indexed>> {
        crate::depot::map::raw::Probe::find(&mut self.core, hash, eq)
    }
}
