use std::{error::Error, fmt, marker::PhantomData, num::NonZeroU32};

pub mod cell;
mod core;
pub mod key;
pub mod lease;
mod pending;
pub mod pin;

use core::{Exclusive, Reservations, SlabCore, Ticket, entries::Entries};

use key::{SlabGeneration, SlabKey, SlabKeyParts};
use pending::Pending;

trait GenerationState: Copy + Eq {
    const MIN: Self;
    const VALID: ();
    #[must_use]
    fn next(self) -> Option<Self>;
}

/// A slot count representable by every dynamically allocated slab.
/// Allocation follows the standard collection policy: exhaustion aborts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct SlabCapacity(u32);

/// A nonzero slot count representable by every dynamically allocated slab.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct NonZeroSlabCapacity(NonZeroU32);

impl SlabCapacity {
    pub const EMPTY: Self = Self(0);
    pub const MAX: Self = Self(u32::MAX);

    #[must_use]
    pub const fn new(capacity: u32) -> Self {
        Self(capacity)
    }

    #[must_use]
    pub const fn get(self) -> usize {
        self.0 as usize
    }

    #[must_use]
    pub const fn nonzero(self) -> Option<NonZeroSlabCapacity> {
        match NonZeroU32::new(self.0) {
            Some(capacity) => Some(NonZeroSlabCapacity(capacity)),
            None => None,
        }
    }

    pub(super) fn collect_box<T>(self, source: impl IntoIterator<Item = T>) -> Box<[T]> {
        let capacity = self.get();
        let mut values = Vec::with_capacity(capacity);
        values.extend(source);
        debug_assert_eq!(values.len(), capacity);
        debug_assert_eq!(values.capacity(), capacity);
        values.into_boxed_slice()
    }
}

impl NonZeroSlabCapacity {
    #[must_use]
    pub const fn get(self) -> usize {
        self.0.get() as usize
    }

    #[must_use]
    pub const fn slab(self) -> SlabCapacity {
        SlabCapacity::new(self.0.get())
    }
}

impl TryFrom<usize> for SlabCapacity {
    type Error = SlabCapacityError;

    fn try_from(capacity: usize) -> Result<Self, Self::Error> {
        match u32::try_from(capacity) {
            Ok(capacity) => Ok(Self(capacity)),
            Err(_) => Err(SlabCapacityError {
                requested: capacity,
            }),
        }
    }
}

impl From<SlabCapacity> for usize {
    fn from(capacity: SlabCapacity) -> Self {
        capacity.get()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlabCapacityError {
    requested: usize,
}

impl SlabCapacityError {
    #[must_use]
    pub const fn requested(self) -> usize {
        self.requested
    }
}

impl fmt::Display for SlabCapacityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "slab capacity {} exceeds the u32 index domain",
            self.requested
        )
    }
}

impl Error for SlabCapacityError {}

pub struct Slab<T, Tag = (), const MAX: u32 = { u32::MAX }> {
    core: SlabCore<T, SlabGeneration<MAX>, Exclusive>,
    tag: PhantomData<fn() -> Tag>,
}

impl<T, Tag, const MAX: u32> Slab<T, Tag, MAX> {
    pub fn with_capacity(capacity: SlabCapacity) -> Self {
        Self {
            core: SlabCore::with_capacity(capacity),
            tag: PhantomData,
        }
    }

    pub fn capacity(&self) -> usize {
        self.core.capacity()
    }

    pub fn len(&self) -> usize {
        self.core.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

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
}

impl<T, Tag, const MAX: u32> Slab<T, Tag, MAX> {
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
            .index(index)
            .map(|(value, generation)| (value, SlabKey::new(index, generation)))
    }

    pub fn get_index_mut(&mut self, index: u32) -> Option<(&mut T, SlabKey<Tag, MAX>)> {
        self.core
            .index_mut(index)
            .map(|(value, generation)| (value, SlabKey::new(index, generation)))
    }

    pub fn key(&self, index: u32) -> Option<SlabKey<Tag, MAX>> {
        self.core
            .generation(index)
            .map(|generation| SlabKey::new(index, generation))
    }

    pub fn values<'a>(&'a self) -> impl Iterator<Item = &'a T>
    where
        T: 'a,
    {
        self.core.values()
    }

    pub fn values_mut<'a>(&'a mut self) -> impl Iterator<Item = &'a mut T>
    where
        T: 'a,
    {
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
