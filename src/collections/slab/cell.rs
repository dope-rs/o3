use std::marker::PhantomData;

use super::core::{Interior, SlabCore};
use super::pending::Pending;
use super::{SlabGeneration, SlabKey, SlabKeyParts};

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
    impl_slab_common!();

    pub fn keys(&self) -> impl Iterator<Item = SlabKey<Tag, MAX>> + '_ {
        Keys {
            slab: self,
            position: 0,
            remaining: self.len(),
        }
    }

    pub fn insert(&self, value: T) -> Result<SlabKey<Tag, MAX>, T> {
        self.insert_with(value, |key, _| key)
    }

    pub fn insert_with<R>(
        &self,
        value: T,
        f: impl FnOnce(SlabKey<Tag, MAX>, &mut T) -> R,
    ) -> Result<R, T> {
        let Some(ticket) = self.core.take_free() else {
            return Err(value);
        };
        let pending = Pending::new(&self.core, ticket);
        let key = SlabKey::new(ticket.index.get(), ticket.generation);
        let result = pending.commit_with(value, |value| f(key, value));
        Ok(result)
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

    pub fn remove_with<R>(
        &self,
        key: SlabKey<Tag, MAX>,
        f: impl FnOnce(&mut T) -> Option<R>,
    ) -> Option<(T, R)> {
        self.remove_parts_with(key.parts(), f)
    }

    pub fn remove_parts_with<R>(
        &self,
        parts: SlabKeyParts<MAX>,
        f: impl FnOnce(&mut T) -> Option<R>,
    ) -> Option<(T, R)> {
        self.core.remove_with(parts.index(), parts.generation(), f)
    }
}
