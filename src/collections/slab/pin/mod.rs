use std::{mem, pin};

use crate::collections::{
    self,
    slab::{self, key},
};

mod raw;

pub mod fixed;

pub struct Pool<T, Tag = (), const MAX: u32 = { u32::MAX }> {
    core: raw::Core<T, Tag, raw::Lazy<T, MAX>, MAX>,
}

const _: () = assert!(
    mem::size_of::<Pool<u8>>()
        == mem::size_of::<Box<[u8]>>() + mem::size_of::<[u32; 2]>() + mem::size_of::<usize>()
);

#[must_use]
pub struct VacantEntry<'a, T, Tag = (), const MAX: u32 = { u32::MAX }> {
    entry: raw::VacantEntry<'a, T, Tag, raw::Lazy<T, MAX>, MAX>,
}

impl<T, Tag, const MAX: u32> VacantEntry<'_, T, Tag, MAX> {
    pub fn insert(self, value: T) -> key::Key<Tag, MAX> {
        self.entry.insert(value)
    }
}

impl<T, Tag, const MAX: u32> Pool<T, Tag, MAX> {
    pub fn with_capacity(capacity: slab::Capacity) -> Self {
        match Self::try_with_capacity(capacity) {
            Ok(pool) => pool,
            Err(error) => error.abort(),
        }
    }

    pub fn try_with_capacity(
        capacity: slab::Capacity,
    ) -> Result<Self, collections::AllocationError> {
        Ok(Self {
            core: raw::Core::new(raw::Lazy::try_with_capacity(capacity)?),
        })
    }

    pub fn insert(&mut self, value: T) -> Result<key::Key<Tag, MAX>, T> {
        self.core.insert(value)
    }

    pub fn vacant_entry(&mut self) -> Option<VacantEntry<'_, T, Tag, MAX>> {
        Some(VacantEntry {
            entry: self.core.vacant_entry()?,
        })
    }

    pub fn contains_parts(&self, parts: key::Parts<MAX>) -> bool {
        self.core.contains_parts(parts)
    }

    pub fn capacity(&self) -> usize {
        self.core.capacity()
    }

    pub fn key(&self, index: u32) -> Option<key::Key<Tag, MAX>> {
        self.core.key(index)
    }

    pub fn get(&self, key: key::Key<Tag, MAX>) -> Option<pin::Pin<&T>> {
        self.get_parts(key.parts())
    }

    pub fn get_parts(&self, parts: key::Parts<MAX>) -> Option<pin::Pin<&T>> {
        self.core.parts(parts)
    }

    pub fn get_mut(&mut self, key: key::Key<Tag, MAX>) -> Option<pin::Pin<&mut T>> {
        self.get_parts_mut(key.parts())
    }

    pub fn get_parts_mut(&mut self, parts: key::Parts<MAX>) -> Option<pin::Pin<&mut T>> {
        self.core.parts_mut(parts)
    }

    /// Returns the current key and pinned value for an occupied raw index.
    pub fn get_index_mut(&mut self, index: u32) -> Option<(key::Key<Tag, MAX>, pin::Pin<&mut T>)> {
        self.core.get_index_mut(index)
    }

    pub fn remove(&mut self, key: key::Key<Tag, MAX>) -> bool {
        self.remove_parts(key.parts())
    }

    pub fn remove_parts(&mut self, parts: key::Parts<MAX>) -> bool {
        self.core.remove_parts(parts)
    }

    pub fn remove_parts_with<R>(
        &mut self,
        parts: key::Parts<MAX>,
        use_value: impl for<'a> FnOnce(pin::Pin<&'a mut T>) -> R,
    ) -> Option<R> {
        self.core.remove_parts_with(parts, use_value)
    }

    pub fn take_parts(&mut self, parts: key::Parts<MAX>) -> Option<T>
    where
        T: Unpin,
    {
        self.core.take_parts(parts)
    }
}
