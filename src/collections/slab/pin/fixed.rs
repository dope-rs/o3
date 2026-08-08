use std::{marker::PhantomPinned, pin::Pin};

use crate::collections::slab::{key, pin};

pub struct Pool<T, const N: usize, Tag = (), const MAX: u32 = { u32::MAX }> {
    core: pin::Core<T, Tag, [pin::Slot<T, MAX>; N], MAX>,
    _pin: PhantomPinned,
}

#[must_use]
pub struct VacantEntry<'a, T, const N: usize, Tag = (), const MAX: u32 = { u32::MAX }> {
    entry: pin::CoreVacantEntry<'a, T, Tag, [pin::Slot<T, MAX>; N], MAX>,
}

impl<T, const N: usize, Tag, const MAX: u32> VacantEntry<'_, T, N, Tag, MAX> {
    pub fn insert(self, value: T) -> key::Key<Tag, MAX> {
        self.entry.insert(value)
    }
}

impl<T, const N: usize, Tag, const MAX: u32> Pool<T, N, Tag, MAX> {
    pub fn new() -> Self {
        use std::array::from_fn;

        use crate::collections::slab::pin::{Core, Slot};
        Self {
            core: Core::new(from_fn(|index| Slot::new(index, N))),
            _pin: PhantomPinned,
        }
    }

    pub fn vacant_entry(self: Pin<&mut Self>) -> Option<VacantEntry<'_, T, N, Tag, MAX>> {
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

    pub fn get_parts_mut(self: Pin<&mut Self>, parts: key::Parts<MAX>) -> Option<Pin<&mut T>> {
        unsafe { self.get_unchecked_mut() }.core.parts_mut(parts)
    }

    pub fn remove_parts(self: Pin<&mut Self>, parts: key::Parts<MAX>) -> bool {
        unsafe { self.get_unchecked_mut() }.core.remove_parts(parts)
    }
}

impl<T, const N: usize, Tag, const MAX: u32> Default for Pool<T, N, Tag, MAX> {
    fn default() -> Self {
        Self::new()
    }
}
