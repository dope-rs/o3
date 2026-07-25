use std::marker::PhantomData;

use super::core::{Interior, SlabCore};
use super::key::{SlabGeneration, SlabKey, SlabKeyParts};
use super::pending::Pending;

pub struct CellSlab<T, Tag = (), const MAX: u32 = { u32::MAX }> {
    core: SlabCore<T, SlabGeneration<MAX>, Interior>,
    tag: PhantomData<fn() -> Tag>,
}

struct Keys<'a, T, Tag, const MAX: u32> {
    slab: &'a CellSlab<T, Tag, MAX>,
    position: usize,
    remaining: usize,
}

impl<T, Tag, const MAX: u32> Iterator for Keys<'_, T, Tag, MAX> {
    type Item = SlabKey<Tag, MAX>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.remaining != 0 {
            let position = self.position;
            self.position += 1;
            self.remaining -= 1;
            if let Some((index, generation)) = self.slab.core.occupied_at(position) {
                return Some(SlabKey::new(index, generation));
            }
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.remaining))
    }
}

impl<T, Tag, const MAX: u32> CellSlab<T, Tag, MAX> {
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            core: SlabCore::with_capacity(capacity),
            tag: PhantomData,
        }
    }

    pub fn capacity(&self) -> usize {
        self.core.capacity()
    }

    pub fn grow_to(&mut self, capacity: usize) {
        self.core.grow_to(capacity);
    }

    pub fn len(&self) -> usize {
        self.core.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn contains_key(&self, key: SlabKey<Tag, MAX>) -> bool {
        self.contains_parts(key.parts())
    }

    pub fn contains_parts(&self, parts: SlabKeyParts<MAX>) -> bool {
        self.core.contains(parts.index(), parts.generation())
    }

    pub fn resolve(&self, parts: SlabKeyParts<MAX>) -> Option<SlabKey<Tag, MAX>> {
        self.contains_parts(parts)
            .then(|| SlabKey::from_parts(parts))
    }

    pub fn keys(&self) -> impl Iterator<Item = SlabKey<Tag, MAX>> + '_ {
        Keys {
            slab: self,
            position: 0,
            remaining: self.len(),
        }
    }

    pub fn insert(&self, value: T) -> Result<SlabKey<Tag, MAX>, T> {
        let Some(ticket) = self.core.take_free() else {
            return Err(value);
        };
        let pending = Pending::new(&self.core, ticket);
        let key = SlabKey::new(ticket.index.get(), ticket.generation);
        pending.commit(value);
        Ok(key)
    }

    pub fn update<R>(&self, key: SlabKey<Tag, MAX>, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        self.update_parts(key.parts(), f)
    }

    pub fn update_parts<R>(
        &self,
        parts: SlabKeyParts<MAX>,
        f: impl FnOnce(&mut T) -> R,
    ) -> Option<R> {
        self.core.update(parts.index(), parts.generation(), f)
    }

    pub fn remove(&self, key: SlabKey<Tag, MAX>) -> Option<T> {
        self.remove_parts(key.parts())
    }

    pub fn remove_parts(&self, parts: SlabKeyParts<MAX>) -> Option<T> {
        self.core
            .remove(parts.index(), parts.generation())
            .map(|(value, _)| value)
    }

    pub fn remove_parts_with<R>(
        &self,
        parts: SlabKeyParts<MAX>,
        f: impl FnOnce(&mut T) -> Option<R>,
    ) -> Option<(T, R)> {
        self.core.remove_with(parts.index(), parts.generation(), f)
    }
}
