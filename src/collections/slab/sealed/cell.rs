use std::marker;

use crate::collections::{
    self,
    slab::{self, key, sealed::core},
};

pub struct Cell<T, Tag = (), const MAX: u32 = { u32::MAX }, const RECYCLE: bool = false> {
    core: core::Core<T, key::Generation<MAX>, core::Interior<RECYCLE>>,
    tag: marker::PhantomData<fn() -> Tag>,
}

/// Scoped access to physical slots and untyped handle parts.
pub struct CellSlots<'a, T, Tag = (), const MAX: u32 = { u32::MAX }, const RECYCLE: bool = false> {
    slab: &'a Cell<T, Tag, MAX, RECYCLE>,
}

struct Keys<'a, T, Tag, const MAX: u32, const RECYCLE: bool> {
    slab: &'a Cell<T, Tag, MAX, RECYCLE>,
    position: usize,
    remaining: usize,
}

impl<T, Tag, const MAX: u32, const RECYCLE: bool> Iterator for Keys<'_, T, Tag, MAX, RECYCLE> {
    type Item = key::Handle<Tag, MAX>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.remaining != 0 {
            let position = self.position;
            self.position += 1;
            self.remaining -= 1;
            if let Some((index, generation)) = self.slab.core.entries().occupied_at(position) {
                return Some(key::Handle::new(index, generation));
            }
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.remaining))
    }
}

impl<T, Tag, const MAX: u32> Cell<T, Tag, MAX, false> {
    pub fn with_capacity(capacity: slab::Capacity) -> Self {
        match Self::try_with_capacity(capacity) {
            Ok(slab) => slab,
            Err(error) => error.abort(),
        }
    }

    /// Reserves every slot and dense index entry transactionally.
    pub fn try_with_capacity(
        capacity: slab::Capacity,
    ) -> Result<Self, collections::AllocationError> {
        Ok(Self {
            core: core::Core::try_with_capacity(capacity)?,
            tag: marker::PhantomData,
        })
    }
}

impl<T, Tag, const MAX: u32> slab::raw::Recycling<T, Tag, MAX> for Cell<T, Tag, MAX, true> {
    unsafe fn try_with_capacity_recycling(
        capacity: slab::Capacity,
    ) -> Result<Self, collections::AllocationError> {
        Ok(Self {
            core: core::Core::try_with_capacity(capacity)?,
            tag: marker::PhantomData,
        })
    }
}

impl<T, Tag, const MAX: u32, const RECYCLE: bool> Cell<T, Tag, MAX, RECYCLE> {
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

    /// Counts free, reusable slots without growing.
    /// This cold O(capacity) scan excludes retired generations and reservations,
    /// preserving the slab's layout and hot-path cost.
    pub fn available(&self) -> usize {
        self.core.available()
    }

    pub fn keys(&self) -> impl Iterator<Item = key::Handle<Tag, MAX>> + '_ {
        Keys {
            slab: self,
            position: 0,
            remaining: self.len(),
        }
    }

    pub fn slots(&self) -> CellSlots<'_, T, Tag, MAX, RECYCLE> {
        CellSlots { slab: self }
    }

    /// Tests live values, conservatively treating a reentrant busy slot as true.
    pub fn any_or_busy(&self, predicate: impl FnMut(&T) -> bool) -> bool {
        self.core.any_or_busy(predicate)
    }

    pub fn insert(&self, value: T) -> Result<key::Handle<Tag, MAX>, T> {
        use crate::collections::slab::sealed::pending::Pending;
        let Some(ticket) = self.core.reservations().take_free() else {
            return Err(value);
        };
        let pending = Pending::new(&self.core, ticket);
        let key = key::Handle::new(ticket.index.get(), ticket.generation);
        pending.commit(value);
        Ok(key)
    }

    /// Runs `f` with an installed value, committing only when it succeeds.
    pub fn try_insert_with<R, E>(
        &self,
        value: T,
        f: impl FnOnce(key::Handle<Tag, MAX>, &mut T) -> Result<R, E>,
    ) -> Result<(key::Handle<Tag, MAX>, R), slab::InsertError<T, E>> {
        self.core
            .try_insert_with(value, |ticket, value| {
                f(
                    key::Handle::new(ticket.index.get(), ticket.generation),
                    value,
                )
            })
            .map(|(ticket, result)| {
                (
                    key::Handle::new(ticket.index.get(), ticket.generation),
                    result,
                )
            })
            .map_err(|error| match error {
                core::TryInsertError::Full(value) => slab::InsertError::Full(value),
                core::TryInsertError::Rejected(value, error) => {
                    slab::InsertError::Rejected(value, error)
                }
            })
    }

    /// Builds after reserving a slot, committing only when `f` succeeds.
    pub fn try_insert_build<I, R, E>(
        &self,
        input: I,
        build: impl FnOnce(I) -> T,
        f: impl FnOnce(key::Handle<Tag, MAX>, &mut T) -> Result<R, E>,
    ) -> Result<(key::Handle<Tag, MAX>, R), slab::BuildError<I, T, E>> {
        self.core
            .try_insert_build(input, build, |ticket, value| {
                f(
                    key::Handle::new(ticket.index.get(), ticket.generation),
                    value,
                )
            })
            .map(|(ticket, result)| {
                (
                    key::Handle::new(ticket.index.get(), ticket.generation),
                    result,
                )
            })
            .map_err(|error| match error {
                core::TryBuildError::Full(input) => slab::BuildError::Full(input),
                core::TryBuildError::Rejected(value, error) => {
                    slab::BuildError::Rejected(value, error)
                }
            })
    }

    pub fn update<R>(&self, key: key::Handle<Tag, MAX>, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        self.core.entries().update(key.index(), key.generation(), f)
    }

    pub fn remove(&self, key: key::Handle<Tag, MAX>) -> Option<T> {
        self.core
            .entries()
            .remove(key.index(), key.generation())
            .map(|(value, _)| value)
    }
}

impl<T, Tag, const MAX: u32, const RECYCLE: bool> CellSlots<'_, T, Tag, MAX, RECYCLE> {
    /// Returns the live key at one dense position without scanning.
    pub fn key_at(self, position: usize) -> Option<key::Handle<Tag, MAX>> {
        let (index, generation) = self.slab.core.entries().occupied_at(position)?;
        Some(key::Handle::new(index, generation))
    }

    /// Updates a physical slot regardless of its private generation.
    pub fn update_index<R>(self, index: u32, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        self.slab.core.entries().update_index(index, f)
    }

    pub fn update_parts<R>(self, parts: key::Parts<MAX>, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        self.slab
            .core
            .entries()
            .update(parts.index(), parts.generation(), f)
    }

    pub fn remove_parts(self, parts: key::Parts<MAX>) -> Option<T> {
        self.slab
            .core
            .entries()
            .remove(parts.index(), parts.generation())
            .map(|(value, _)| value)
    }

    pub fn remove_parts_with<R>(
        self,
        parts: key::Parts<MAX>,
        f: impl FnOnce(&mut T) -> Option<R>,
    ) -> Option<(T, R)> {
        self.slab
            .core
            .entries()
            .remove_with(parts.index(), parts.generation(), f)
    }

    /// Conditionally removes a slot regardless of its private generation.
    pub fn remove_index_with<R>(
        self,
        index: u32,
        f: impl FnOnce(&mut T) -> Option<R>,
    ) -> Option<(T, R)> {
        self.slab
            .core
            .entries()
            .remove_index_with(index, |value, _| f(value))
            .map(|(value, result, _)| (value, result))
    }
}
