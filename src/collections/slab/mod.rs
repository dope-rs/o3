use std::marker::PhantomData;

macro_rules! impl_slab_common {
    () => {
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
    };
}

mod cell;
mod core;
mod key;
mod pending;

pub use cell::CellSlab;
pub use key::{SlabGeneration, SlabKey, SlabKeyParts};

use core::{Exclusive, SlabCore, Ticket};
use pending::Pending;

pub(crate) trait GenerationState: Copy + Eq {
    const MIN: Self;
    const VALID: ();
    #[must_use]
    fn next(self) -> Option<Self>;
}

pub struct Slab<T, Tag = (), const MAX: u32 = { u32::MAX }> {
    core: SlabCore<T, SlabGeneration<MAX>, Exclusive>,
    tag: PhantomData<fn() -> Tag>,
}

impl<T, Tag, const MAX: u32> Slab<T, Tag, MAX> {
    impl_slab_common!();

    pub fn is_full(&self) -> bool {
        self.core.is_full()
    }

    pub fn insert(&mut self, value: T) -> Result<SlabKey<Tag, MAX>, T> {
        self.insert_entry(value).map(|(key, _)| key)
    }

    pub fn insert_entry(&mut self, value: T) -> Result<(SlabKey<Tag, MAX>, &mut T), T> {
        let Some(ticket) = self.core.take_free() else {
            return Err(value);
        };
        Ok(self.insert_ticket(ticket, value))
    }

    fn insert_ticket(
        &mut self,
        ticket: Ticket<SlabGeneration<MAX>>,
        value: T,
    ) -> (SlabKey<Tag, MAX>, &mut T) {
        let pending = Pending::new(&self.core, ticket);
        let raw_index = ticket.index.get();
        let key = SlabKey::new(raw_index, ticket.generation);
        pending.commit(value);
        let value = unsafe {
            self.core
                .get_mut(raw_index, ticket.generation)
                .unwrap_unchecked()
        };
        (key, value)
    }

    pub fn vacant_entry(&mut self) -> Option<SlabVacantEntry<'_, T, Tag, MAX>> {
        let ticket = self.core.take_free()?;
        Some(SlabVacantEntry {
            slab: self,
            ticket: Some(ticket),
        })
    }

    pub fn vacant_entry_at(&mut self, index: u32) -> Option<SlabVacantEntry<'_, T, Tag, MAX>> {
        let ticket = self.core.take_index(index)?;
        Some(SlabVacantEntry {
            slab: self,
            ticket: Some(ticket),
        })
    }

    pub fn insert_at_with(
        &mut self,
        index: u32,
        make: impl FnOnce(SlabKey<Tag, MAX>) -> T,
    ) -> Option<SlabKey<Tag, MAX>> {
        let entry = self.vacant_entry_at(index)?;
        let key = entry.key();
        let value = make(key);
        Some(entry.insert(value))
    }

    pub fn get(&self, key: SlabKey<Tag, MAX>) -> Option<&T> {
        self.get_parts(key.parts())
    }

    pub fn get_parts(&self, parts: SlabKeyParts<MAX>) -> Option<&T> {
        self.core.get(parts.index(), parts.generation())
    }

    pub fn get_mut(&mut self, key: SlabKey<Tag, MAX>) -> Option<&mut T> {
        self.get_parts_mut(key.parts())
    }

    pub fn get_parts_mut(&mut self, parts: SlabKeyParts<MAX>) -> Option<&mut T> {
        self.core.get_mut(parts.index(), parts.generation())
    }

    pub fn remove(&mut self, key: SlabKey<Tag, MAX>) -> Option<T> {
        self.remove_parts(key.parts())
    }

    pub fn remove_parts(&mut self, parts: SlabKeyParts<MAX>) -> Option<T> {
        let (value, _) = self.core.remove(parts.index(), parts.generation())?;
        Some(value)
    }

    pub fn remove_index(&mut self, index: u32) -> Option<T> {
        let (value, _) = self.core.remove_index(index)?;
        Some(value)
    }

    pub fn remove_index_with<R>(
        &mut self,
        index: u32,
        f: impl FnOnce(&mut T, SlabKey<Tag, MAX>) -> Option<R>,
    ) -> Option<(T, R)> {
        let (value, result, _) = self.core.remove_index_with(index, |value, generation| {
            f(value, SlabKey::new(index, generation))
        })?;
        Some((value, result))
    }

    pub fn get_index(&self, index: u32) -> Option<(&T, SlabKey<Tag, MAX>)> {
        self.core
            .get_index(index)
            .map(|(value, generation)| (value, SlabKey::new(index, generation)))
    }

    pub fn get_index_mut(&mut self, index: u32) -> Option<(&mut T, SlabKey<Tag, MAX>)> {
        self.core
            .get_index_mut(index)
            .map(|(value, generation)| (value, SlabKey::new(index, generation)))
    }

    pub fn key(&self, index: u32) -> Option<SlabKey<Tag, MAX>> {
        self.core
            .generation(index)
            .map(|generation| SlabKey::new(index, generation))
    }

    pub fn values(&self) -> impl Iterator<Item = &T> {
        self.core.values()
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.core.values_mut()
    }

    pub fn clear(&mut self) {
        self.core.clear();
    }
}

pub struct SlabVacantEntry<'a, T, Tag = (), const MAX: u32 = { u32::MAX }> {
    slab: &'a mut Slab<T, Tag, MAX>,
    ticket: Option<Ticket<SlabGeneration<MAX>>>,
}

impl<T, Tag, const MAX: u32> SlabVacantEntry<'_, T, Tag, MAX> {
    pub fn index(&self) -> u32 {
        unsafe { self.ticket.unwrap_unchecked() }.index.get()
    }

    pub fn key(&self) -> SlabKey<Tag, MAX> {
        let ticket = unsafe { self.ticket.unwrap_unchecked() };
        SlabKey::new(ticket.index.get(), ticket.generation)
    }

    pub fn insert(mut self, value: T) -> SlabKey<Tag, MAX> {
        let ticket = unsafe { self.ticket.unwrap_unchecked() };
        let key = SlabKey::new(ticket.index.get(), ticket.generation);
        let ticket = unsafe { self.ticket.take().unwrap_unchecked() };
        self.slab.core.commit(ticket, value);
        key
    }
}

impl<T, Tag, const MAX: u32> Drop for SlabVacantEntry<'_, T, Tag, MAX> {
    fn drop(&mut self) {
        if let Some(ticket) = self.ticket.take() {
            self.slab.core.rollback(ticket);
        }
    }
}
