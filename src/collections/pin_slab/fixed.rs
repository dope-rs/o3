use std::marker::PhantomPinned;
use std::pin::Pin;

use crate::collections::{SlabGeneration, SlabKey, SlabKeyParts};

use super::{Core, CoreOccupiedEntry, CoreVacantEntry, Slot};

pub struct FixedPinSlab<T, const N: usize, Tag = (), const MAX: u32 = { u32::MAX }> {
    core: Core<T, Tag, [Slot<T, MAX>; N], MAX>,
    _pin: PhantomPinned,
}

#[must_use]
pub struct FixedPinSlabOccupiedEntry<'a, T, const N: usize, Tag = (), const MAX: u32 = { u32::MAX }>
{
    entry: CoreOccupiedEntry<'a, T, Tag, [Slot<T, MAX>; N], MAX>,
}

#[must_use]
pub struct FixedPinSlabVacantEntry<'a, T, const N: usize, Tag = (), const MAX: u32 = { u32::MAX }> {
    entry: CoreVacantEntry<'a, T, Tag, [Slot<T, MAX>; N], MAX>,
}

impl<T, const N: usize, Tag, const MAX: u32> FixedPinSlabOccupiedEntry<'_, T, N, Tag, MAX> {
    impl_occupied_entry!();
}

impl<T, const N: usize, Tag, const MAX: u32> FixedPinSlabVacantEntry<'_, T, N, Tag, MAX> {
    impl_vacant_entry!();
}

impl<T, const N: usize, Tag, const MAX: u32> FixedPinSlab<T, N, Tag, MAX> {
    pub fn new() -> Self {
        Self {
            core: Core::new(std::array::from_fn(|index| Slot::new(index, N))),
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

    impl_common!();

    pub fn get(self: Pin<&Self>, key: SlabKey<Tag, MAX>) -> Option<Pin<&T>> {
        self.get_parts(key.parts())
    }

    pub fn get_parts(self: Pin<&Self>, parts: SlabKeyParts<MAX>) -> Option<Pin<&T>> {
        self.get_ref().core.get_parts(parts)
    }

    pub fn get_mut(self: Pin<&mut Self>, key: SlabKey<Tag, MAX>) -> Option<Pin<&mut T>> {
        self.get_parts_mut(key.parts())
    }

    pub fn get_parts_mut(self: Pin<&mut Self>, parts: SlabKeyParts<MAX>) -> Option<Pin<&mut T>> {
        unsafe { self.get_unchecked_mut() }
            .core
            .get_parts_mut(parts)
    }

    pub fn entry(
        self: Pin<&mut Self>,
        key: SlabKey<Tag, MAX>,
    ) -> Option<FixedPinSlabOccupiedEntry<'_, T, N, Tag, MAX>> {
        self.entry_parts(key.parts())
    }

    pub fn entry_parts(
        self: Pin<&mut Self>,
        parts: SlabKeyParts<MAX>,
    ) -> Option<FixedPinSlabOccupiedEntry<'_, T, N, Tag, MAX>> {
        let this = unsafe { self.get_unchecked_mut() };
        Some(FixedPinSlabOccupiedEntry {
            entry: this.core.entry_parts(parts)?,
        })
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
