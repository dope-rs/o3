use std::{marker, pin};

use crate::collections::slab::key;

pub struct Pool<T, const N: usize, Tag = (), const MAX: u32 = { u32::MAX }> {
    core: super::raw::Core<T, Tag, [super::raw::Slot<T, MAX>; N], MAX>,
    _pin: marker::PhantomPinned,
}

#[must_use]
pub struct VacantEntry<'a, T, const N: usize, Tag = (), const MAX: u32 = { u32::MAX }> {
    entry: super::raw::VacantEntry<'a, T, Tag, [super::raw::Slot<T, MAX>; N], MAX>,
}

impl<T, const N: usize, Tag, const MAX: u32> VacantEntry<'_, T, N, Tag, MAX> {
    pub fn insert(self, value: T) -> key::Key<Tag, MAX> {
        self.entry.insert(value)
    }
}

impl<T, const N: usize, Tag, const MAX: u32> Pool<T, N, Tag, MAX> {
    pub fn new() -> Self {
        use std::array::from_fn;

        use crate::collections::slab::pin::raw::{Core, Slot};
        Self {
            core: Core::new(from_fn(|index| Slot::linked(index, N))),
            _pin: marker::PhantomPinned,
        }
    }

    pub fn vacant_entry(self: pin::Pin<&mut Self>) -> Option<VacantEntry<'_, T, N, Tag, MAX>> {
        let this = unsafe { self.get_unchecked_mut() };
        Some(VacantEntry {
            entry: this.core.vacant_entry()?,
        })
    }

    pub fn capacity(&self) -> usize {
        self.core.capacity()
    }

    pub fn key(&self, index: u32) -> Option<key::Key<Tag, MAX>> {
        self.core.key(index)
    }

    pub fn get_parts_mut(
        self: pin::Pin<&mut Self>,
        parts: key::Parts<MAX>,
    ) -> Option<pin::Pin<&mut T>> {
        unsafe { self.get_unchecked_mut() }.core.parts_mut(parts)
    }

    pub fn remove_parts(self: pin::Pin<&mut Self>, parts: key::Parts<MAX>) -> bool {
        unsafe { self.get_unchecked_mut() }.core.remove_parts(parts)
    }
}

impl<T, const N: usize, Tag, const MAX: u32> Default for Pool<T, N, Tag, MAX> {
    fn default() -> Self {
        Self::new()
    }
}
