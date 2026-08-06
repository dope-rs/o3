use std::{marker::PhantomPinned, pin::Pin};

use crate::collections::slab::{
    key::{SlabKey, SlabKeyParts},
    pin::{Core, CoreVacantEntry, Slot},
};

pub struct FixedPinSlab<T, const N: usize, Tag = (), const MAX: u32 = { u32::MAX }> {
    core: Core<T, Tag, [Slot<T, MAX>; N], MAX>,
    _pin: PhantomPinned,
}

#[must_use]
pub struct FixedPinSlabVacantEntry<'a, T, const N: usize, Tag = (), const MAX: u32 = { u32::MAX }> {
    entry: CoreVacantEntry<'a, T, Tag, [Slot<T, MAX>; N], MAX>,
}

impl<T, const N: usize, Tag, const MAX: u32> FixedPinSlabVacantEntry<'_, T, N, Tag, MAX> {
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

    pub fn vacant_entry(
        self: Pin<&mut Self>,
    ) -> Option<FixedPinSlabVacantEntry<'_, T, N, Tag, MAX>> {
        let this = unsafe { self.get_unchecked_mut() };
        Some(FixedPinSlabVacantEntry {
            entry: this.core.vacant_entry()?,
        })
    }

    pub fn capacity(&self) -> usize {
        self.core.capacity()
    }

    pub fn key(&self, index: u32) -> Option<SlabKey<Tag, MAX>> {
        self.core.key(index)
    }

    pub fn get_parts_mut(self: Pin<&mut Self>, parts: SlabKeyParts<MAX>) -> Option<Pin<&mut T>> {
        unsafe { self.get_unchecked_mut() }.core.parts_mut(parts)
    }

    pub fn remove_parts(self: Pin<&mut Self>, parts: SlabKeyParts<MAX>) -> bool {
        unsafe { self.get_unchecked_mut() }.core.remove_parts(parts)
    }
}

impl<T, const N: usize, Tag, const MAX: u32> Default for FixedPinSlab<T, N, Tag, MAX> {
    fn default() -> Self {
        Self::new()
    }
}
