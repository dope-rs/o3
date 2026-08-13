use std::{marker, mem, pin};

use crate::collections::{
    self,
    slab::{self, key},
};

mod core;

/// A heap-backed pinned generational pool.
pub struct Pool<T, Tag = (), const MAX: u32 = { u32::MAX }> {
    core: core::Core<T, Tag, core::Lazy<T, MAX>, MAX>,
}

const _: () = assert!(
    mem::size_of::<Pool<u8>>()
        == mem::size_of::<Box<[u8]>>() + mem::size_of::<[u32; 2]>() + mem::size_of::<usize>()
);

#[must_use]
pub struct VacantEntry<'a, T, Tag = (), const MAX: u32 = { u32::MAX }> {
    entry: core::Vacant<'a, T, Tag, core::Lazy<T, MAX>, MAX>,
}

impl<T, Tag, const MAX: u32> VacantEntry<'_, T, Tag, MAX> {
    pub fn insert(self, value: T) -> key::Handle<Tag, MAX> {
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
            core: core::Core::new(core::Lazy::try_with_capacity(capacity)?),
        })
    }

    pub fn insert(&mut self, value: T) -> Result<key::Handle<Tag, MAX>, T> {
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

    pub const fn len(&self) -> usize {
        self.core.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn key(&self, index: u32) -> Option<key::Handle<Tag, MAX>> {
        self.core.key(index)
    }

    pub fn get(&self, key: key::Handle<Tag, MAX>) -> Option<pin::Pin<&T>> {
        self.parts(key.parts())
    }

    pub fn parts(&self, parts: key::Parts<MAX>) -> Option<pin::Pin<&T>> {
        self.core.parts(parts)
    }

    pub fn get_mut(&mut self, key: key::Handle<Tag, MAX>) -> Option<pin::Pin<&mut T>> {
        self.parts_mut(key.parts())
    }

    pub fn parts_mut(&mut self, parts: key::Parts<MAX>) -> Option<pin::Pin<&mut T>> {
        self.core.parts_mut(parts)
    }

    /// Returns the current key and pinned value for an occupied raw index.
    pub fn index_mut(&mut self, index: u32) -> Option<(key::Handle<Tag, MAX>, pin::Pin<&mut T>)> {
        self.core.index_mut(index)
    }

    pub fn remove(&mut self, key: key::Handle<Tag, MAX>) -> bool {
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

impl<T, Tag, const MAX: u32> Drop for Pool<T, Tag, MAX> {
    fn drop(&mut self) {
        use crate::collections::ClearGuard;

        ClearGuard::run(&mut self.core, core::Core::clear);
    }
}

/// A stack-backed pinned generational pool.
pub struct Fixed<T, const N: usize, Tag = (), const MAX: u32 = { u32::MAX }> {
    core: core::Core<T, Tag, [core::Slot<T, MAX>; N], MAX>,
    _pin: marker::PhantomPinned,
}

#[must_use]
pub struct FixedVacantEntry<'a, T, const N: usize, Tag = (), const MAX: u32 = { u32::MAX }> {
    entry: core::Vacant<'a, T, Tag, [core::Slot<T, MAX>; N], MAX>,
}

impl<T, const N: usize, Tag, const MAX: u32> FixedVacantEntry<'_, T, N, Tag, MAX> {
    pub fn insert(self, value: T) -> key::Handle<Tag, MAX> {
        self.entry.insert(value)
    }
}

impl<T, const N: usize, Tag, const MAX: u32> Fixed<T, N, Tag, MAX> {
    pub fn new() -> Self {
        use std::array::from_fn;

        Self {
            core: core::Core::new(from_fn(|index| core::Slot::linked(index, N))),
            _pin: marker::PhantomPinned,
        }
    }

    pub fn vacant_entry(self: pin::Pin<&mut Self>) -> Option<FixedVacantEntry<'_, T, N, Tag, MAX>> {
        let this = unsafe { self.get_unchecked_mut() };
        Some(FixedVacantEntry {
            entry: this.core.vacant_entry()?,
        })
    }

    pub fn capacity(&self) -> usize {
        self.core.capacity()
    }

    pub const fn len(&self) -> usize {
        self.core.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn key(&self, index: u32) -> Option<key::Handle<Tag, MAX>> {
        self.core.key(index)
    }

    pub fn parts_mut(
        self: pin::Pin<&mut Self>,
        parts: key::Parts<MAX>,
    ) -> Option<pin::Pin<&mut T>> {
        unsafe { self.get_unchecked_mut() }.core.parts_mut(parts)
    }

    pub fn remove_parts(self: pin::Pin<&mut Self>, parts: key::Parts<MAX>) -> bool {
        unsafe { self.get_unchecked_mut() }.core.remove_parts(parts)
    }
}

impl<T, const N: usize, Tag, const MAX: u32> Default for Fixed<T, N, Tag, MAX> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const N: usize, Tag, const MAX: u32> Drop for Fixed<T, N, Tag, MAX> {
    fn drop(&mut self) {
        use crate::collections::ClearGuard;

        ClearGuard::run(&mut self.core, core::Core::clear);
    }
}
