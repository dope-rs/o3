use std::marker;

use crate::collections::slab::{self, core, key};

pub struct Cell<T, Tag = (), const MAX: u32 = { u32::MAX }> {
    core: core::Core<T, key::Generation<MAX>, core::Interior>,
    tag: marker::PhantomData<fn() -> Tag>,
}

struct Keys<'a, T, Tag, const MAX: u32> {
    slab: &'a Cell<T, Tag, MAX>,
    position: usize,
    remaining: usize,
}

impl<T, Tag, const MAX: u32> Iterator for Keys<'_, T, Tag, MAX> {
    type Item = key::Key<Tag, MAX>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.remaining != 0 {
            let position = self.position;
            self.position += 1;
            self.remaining -= 1;
            if let Some((index, generation)) = self.slab.core.entries().occupied_at(position) {
                return Some(key::Key::new(index, generation));
            }
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.remaining))
    }
}

impl<T, Tag, const MAX: u32> Cell<T, Tag, MAX> {
    pub fn with_capacity(capacity: slab::Capacity) -> Self {
        use crate::collections::slab::core::Core;
        Self {
            core: Core::with_capacity(capacity),
            tag: marker::PhantomData,
        }
    }

    pub fn capacity(&self) -> usize {
        self.core.capacity()
    }

    pub fn grow_to(&mut self, capacity: slab::Capacity) {
        self.core.grow_to(capacity)
    }

    pub fn len(&self) -> usize {
        self.core.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn keys(&self) -> impl Iterator<Item = key::Key<Tag, MAX>> + '_ {
        Keys {
            slab: self,
            position: 0,
            remaining: self.len(),
        }
    }

    pub fn insert(&self, value: T) -> Result<key::Key<Tag, MAX>, T> {
        use crate::collections::slab::pending::Pending;
        let Some(ticket) = self.core.reservations().take_free() else {
            return Err(value);
        };
        let pending = Pending::new(&self.core, ticket);
        let key = key::Key::new(ticket.index.get(), ticket.generation);
        pending.commit(value);
        Ok(key)
    }

    pub fn update<R>(&self, key: key::Key<Tag, MAX>, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        self.update_parts(key.parts(), f)
    }

    pub fn update_parts<R>(
        &self,
        parts: key::Parts<MAX>,
        f: impl FnOnce(&mut T) -> R,
    ) -> Option<R> {
        self.core
            .entries()
            .update(parts.index(), parts.generation(), f)
    }

    pub fn remove(&self, key: key::Key<Tag, MAX>) -> Option<T> {
        self.remove_parts(key.parts())
    }

    pub fn remove_parts(&self, parts: key::Parts<MAX>) -> Option<T> {
        self.core
            .entries()
            .remove(parts.index(), parts.generation())
            .map(|(value, _)| value)
    }

    pub fn remove_parts_with<R>(
        &self,
        parts: key::Parts<MAX>,
        f: impl FnOnce(&mut T) -> Option<R>,
    ) -> Option<(T, R)> {
        self.core
            .entries()
            .remove_with(parts.index(), parts.generation(), f)
    }
}
