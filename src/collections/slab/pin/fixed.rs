use std::marker::PhantomPinned;
use std::pin::Pin;

use super::super::key::{SlabKey, SlabKeyParts};
use super::{Core, CoreVacantEntry, Slot};

pub struct FixedPinSlab<T, const N: usize, Tag = (), const MAX: u32 = { u32::MAX }> {
    core: Core<T, Tag, [Slot<T, MAX>; N], MAX>,
    _pin: PhantomPinned,
}

#[must_use]
pub struct FixedPinSlabVacantEntry<'a, T, const N: usize, Tag = (), const MAX: u32 = { u32::MAX }> {
    entry: CoreVacantEntry<'a, T, Tag, [Slot<T, MAX>; N], MAX>,
}

impl<T, const N: usize, Tag, const MAX: u32> FixedPinSlabVacantEntry<'_, T, N, Tag, MAX> {
    pub fn index(&self) -> u32 {
        self.entry.index()
    }

    pub fn key(&self) -> SlabKey<Tag, MAX> {
        self.entry.key()
    }

    pub fn insert(self, value: T) -> SlabKey<Tag, MAX> {
        self.entry.insert(value)
    }
}

impl<T, const N: usize, Tag, const MAX: u32> FixedPinSlab<T, N, Tag, MAX> {
    pub fn new() -> Self {
        use std::array::from_fn;
        Self {
            core: Core::new(from_fn(|index| Slot::new(index, N))),
            _pin: PhantomPinned,
        }
    }

    pub fn insert(self: Pin<&mut Self>, value: T) -> Result<SlabKey<Tag, MAX>, T> {
        unsafe { self.get_unchecked_mut() }.core.insert(value)
    }

    pub fn vacant_entry(
        self: Pin<&mut Self>,
    ) -> Option<FixedPinSlabVacantEntry<'_, T, N, Tag, MAX>> {
        let this = unsafe { self.get_unchecked_mut() };
        Some(FixedPinSlabVacantEntry {
            entry: this.core.vacant_entry()?,
        })
    }

    pub fn contains_key(&self, key: SlabKey<Tag, MAX>) -> bool {
        self.contains_parts(key.parts())
    }

    pub fn contains_parts(&self, parts: SlabKeyParts<MAX>) -> bool {
        self.core.contains_parts(parts)
    }

    pub fn capacity(&self) -> usize {
        self.core.capacity()
    }

    pub fn len(&self) -> usize {
        self.core.len()
    }

    pub fn is_empty(&self) -> bool {
        self.core.is_empty()
    }

    pub fn is_full(&self) -> bool {
        self.core.is_full()
    }

    pub fn key(&self, index: u32) -> Option<SlabKey<Tag, MAX>> {
        self.core.key(index)
    }

    pub fn get(self: Pin<&Self>, key: SlabKey<Tag, MAX>) -> Option<Pin<&T>> {
        self.get_parts(key.parts())
    }

    pub fn get_parts(self: Pin<&Self>, parts: SlabKeyParts<MAX>) -> Option<Pin<&T>> {
        self.get_ref().core.parts(parts)
    }

    pub fn get_mut(self: Pin<&mut Self>, key: SlabKey<Tag, MAX>) -> Option<Pin<&mut T>> {
        self.get_parts_mut(key.parts())
    }

    pub fn get_parts_mut(self: Pin<&mut Self>, parts: SlabKeyParts<MAX>) -> Option<Pin<&mut T>> {
        unsafe { self.get_unchecked_mut() }.core.parts_mut(parts)
    }

    pub fn remove(self: Pin<&mut Self>, key: SlabKey<Tag, MAX>) -> bool {
        self.remove_parts(key.parts())
    }

    pub fn remove_parts(self: Pin<&mut Self>, parts: SlabKeyParts<MAX>) -> bool {
        unsafe { self.get_unchecked_mut() }.core.remove_parts(parts)
    }

    pub fn take(self: Pin<&mut Self>, key: SlabKey<Tag, MAX>) -> Option<T>
    where
        T: Unpin,
    {
        unsafe { self.get_unchecked_mut() }.core.take(key)
    }
}

impl<T, const N: usize, Tag, const MAX: u32> Default for FixedPinSlab<T, N, Tag, MAX> {
    fn default() -> Self {
        Self::new()
    }
}
