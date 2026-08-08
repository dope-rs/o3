use std::{error::Error, fmt, marker::PhantomData, num::NonZeroU32};

mod cell;
mod core;
pub mod key;
pub mod lease;
mod pending;
pub mod pin;

pub use cell::Cell;

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
pub struct Capacity(u32);

/// A nonzero slot count representable by every dynamically allocated slab.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct NonZeroCapacity(NonZeroU32);

impl Capacity {
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
    pub const fn nonzero(self) -> Option<NonZeroCapacity> {
        match NonZeroU32::new(self.0) {
            Some(capacity) => Some(NonZeroCapacity(capacity)),
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

impl NonZeroCapacity {
    #[must_use]
    pub const fn get(self) -> usize {
        self.0.get() as usize
    }

    #[must_use]
    pub const fn capacity(self) -> Capacity {
        Capacity::new(self.0.get())
    }
}

impl TryFrom<usize> for Capacity {
    type Error = CapacityError;

    fn try_from(capacity: usize) -> Result<Self, Self::Error> {
        match u32::try_from(capacity) {
            Ok(capacity) => Ok(Self(capacity)),
            Err(_) => Err(CapacityError {
                requested: capacity,
            }),
        }
    }
}

impl From<Capacity> for usize {
    fn from(capacity: Capacity) -> Self {
        capacity.get()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapacityError {
    requested: usize,
}

impl CapacityError {
    #[must_use]
    pub const fn requested(self) -> usize {
        self.requested
    }
}

impl fmt::Display for CapacityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "slab capacity {} exceeds the u32 index domain",
            self.requested
        )
    }
}

impl Error for CapacityError {}

pub struct Slab<T, Tag = (), const MAX: u32 = { u32::MAX }> {
    core: core::Core<T, key::Generation<MAX>, core::Exclusive>,
    tag: PhantomData<fn() -> Tag>,
}

impl<T, Tag, const MAX: u32> Slab<T, Tag, MAX> {
    pub fn with_capacity(capacity: Capacity) -> Self {
        use core::Core;
        Self {
            core: Core::with_capacity(capacity),
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

    pub fn insert(&mut self, value: T) -> Result<key::Key<Tag, MAX>, T> {
        self.insert_entry(value).map(|(key, _)| key)
    }

    pub fn insert_entry(&mut self, value: T) -> Result<(key::Key<Tag, MAX>, &mut T), T> {
        let Some(ticket) = self.core.reservations().take_free() else {
            return Err(value);
        };
        Ok(self.insert_ticket(ticket, value))
    }

    fn insert_ticket(
        &mut self,
        ticket: core::Ticket<key::Generation<MAX>>,
        value: T,
    ) -> (key::Key<Tag, MAX>, &mut T) {
        use crate::collections::slab::pending::Pending;
        let pending = Pending::new(&self.core, ticket);
        let raw_index = ticket.index.get();
        let key = key::Key::new(raw_index, ticket.generation);
        pending.commit(value);
        let value = unsafe {
            self.core
                .get_mut(raw_index, ticket.generation)
                .unwrap_unchecked()
        };
        (key, value)
    }

    pub fn vacant_entry(&mut self) -> Option<VacantEntry<'_, T, Tag, MAX>> {
        let ticket = self.core.reservations().take_free()?;
        Some(VacantEntry {
            slab: self,
            ticket: Some(ticket),
        })
    }

    pub fn vacant_entry_at(&mut self, index: u32) -> Option<VacantEntry<'_, T, Tag, MAX>> {
        let ticket = self.core.reservations().take_index(index)?;
        Some(VacantEntry {
            slab: self,
            ticket: Some(ticket),
        })
    }
}

impl<T, Tag, const MAX: u32> Slab<T, Tag, MAX> {
    pub fn get(&self, key: key::Key<Tag, MAX>) -> Option<&T> {
        self.get_parts(key.parts())
    }

    pub fn get_parts(&self, parts: key::Parts<MAX>) -> Option<&T> {
        self.core.entries().get(parts.index(), parts.generation())
    }

    pub fn get_mut(&mut self, key: key::Key<Tag, MAX>) -> Option<&mut T> {
        self.get_parts_mut(key.parts())
    }

    pub fn get_parts_mut(&mut self, parts: key::Parts<MAX>) -> Option<&mut T> {
        self.core.get_mut(parts.index(), parts.generation())
    }

    pub fn remove(&mut self, key: key::Key<Tag, MAX>) -> Option<T> {
        self.remove_parts(key.parts())
    }

    pub fn remove_parts(&mut self, parts: key::Parts<MAX>) -> Option<T> {
        let (value, _) = self
            .core
            .entries()
            .remove(parts.index(), parts.generation())?;
        Some(value)
    }

    pub fn remove_index_with<R>(
        &mut self,
        index: u32,
        f: impl FnOnce(&mut T, key::Key<Tag, MAX>) -> Option<R>,
    ) -> Option<(T, R)> {
        let (value, result, _) = self
            .core
            .entries()
            .remove_index_with(index, |value, generation| {
                f(value, key::Key::new(index, generation))
            })?;
        Some((value, result))
    }

    pub fn get_index(&self, index: u32) -> Option<(&T, key::Key<Tag, MAX>)> {
        self.core
            .entries()
            .index(index)
            .map(|(value, generation)| (value, key::Key::new(index, generation)))
    }

    pub fn get_index_mut(&mut self, index: u32) -> Option<(&mut T, key::Key<Tag, MAX>)> {
        self.core
            .index_mut(index)
            .map(|(value, generation)| (value, key::Key::new(index, generation)))
    }

    pub fn key(&self, index: u32) -> Option<key::Key<Tag, MAX>> {
        self.core
            .entries()
            .generation(index)
            .map(|generation| key::Key::new(index, generation))
    }

    pub fn values<'a>(&'a self) -> impl Iterator<Item = &'a T>
    where
        T: 'a,
    {
        self.core.entries().values()
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

pub struct VacantEntry<'a, T, Tag = (), const MAX: u32 = { u32::MAX }> {
    slab: &'a mut Slab<T, Tag, MAX>,
    ticket: Option<core::Ticket<key::Generation<MAX>>>,
}

impl<T, Tag, const MAX: u32> VacantEntry<'_, T, Tag, MAX> {
    pub fn key(&self) -> key::Key<Tag, MAX> {
        let ticket = unsafe { self.ticket.unwrap_unchecked() };
        key::Key::new(ticket.index.get(), ticket.generation)
    }

    pub fn insert(mut self, value: T) -> key::Key<Tag, MAX> {
        let ticket = unsafe { self.ticket.unwrap_unchecked() };
        let key = key::Key::new(ticket.index.get(), ticket.generation);
        let ticket = unsafe { self.ticket.take().unwrap_unchecked() };
        self.slab.core.reservations().commit(ticket, value);
        key
    }
}

impl<T, Tag, const MAX: u32> Drop for VacantEntry<'_, T, Tag, MAX> {
    fn drop(&mut self) {
        if let Some(ticket) = self.ticket.take() {
            self.slab.core.reservations().rollback(ticket);
        }
    }
}
