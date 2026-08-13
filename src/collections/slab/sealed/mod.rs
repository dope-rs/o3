use std::{error, fmt, marker, num, ops};

mod cell;
mod core;
mod pending;
mod vacant;

pub use cell::{Cell, CellSlots};
pub use vacant::VacantEntry;

use crate::collections::{
    self,
    slab::{self, key},
};

/// Failure from a transactional cell-slab insertion.
pub enum InsertError<T, E> {
    Full(T),
    Rejected(T, E),
}

/// Failure from building a transactional cell-slab insertion.
pub enum BuildError<I, T, E> {
    Full(I),
    Rejected(T, E),
}

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
pub struct NonZeroCapacity(num::NonZeroU32);

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
    pub const fn raw(self) -> u32 {
        self.0
    }

    #[must_use]
    pub const fn nonzero(self) -> Option<NonZeroCapacity> {
        match num::NonZeroU32::new(self.0) {
            Some(capacity) => Some(NonZeroCapacity(capacity)),
            None => None,
        }
    }

    pub(super) fn box_with<T>(self, initialize: impl FnMut(usize) -> T) -> Box<[T]> {
        match self.try_box_with(initialize) {
            Ok(values) => values,
            Err(error) => error.abort(),
        }
    }

    pub(super) fn try_box_with<T>(
        self,
        initialize: impl FnMut(usize) -> T,
    ) -> Result<Box<[T]>, collections::AllocationError> {
        let capacity = self.get();
        collections::BoxSliceExt::try_box_with(capacity, initialize)
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

impl error::Error for CapacityError {}

pub struct Exclusive<
    T,
    Tag = (),
    const MAX: u32 = { u32::MAX },
    const RECYCLE: bool = false,
    const PARTITIONS: usize = 1,
> {
    core: core::Core<T, key::Generation<MAX>, core::Exclusive<RECYCLE>, PARTITIONS>,
    tag: marker::PhantomData<fn() -> Tag>,
}

/// Scoped access to physical slots and untyped handle parts.
pub struct Slots<
    'a,
    T,
    Tag = (),
    const MAX: u32 = { u32::MAX },
    const RECYCLE: bool = false,
    const PARTITIONS: usize = 1,
> {
    slab: &'a mut Exclusive<T, Tag, MAX, RECYCLE, PARTITIONS>,
}

pub struct OccupiedEntry<
    'a,
    T,
    Tag = (),
    const MAX: u32 = { u32::MAX },
    const RECYCLE: bool = false,
    const PARTITIONS: usize = 1,
> {
    busy: core::Busy<'a, T, key::Generation<MAX>, core::Exclusive<RECYCLE>, PARTITIONS>,
    tag: marker::PhantomData<fn() -> Tag>,
}

impl<T, Tag, const MAX: u32, const PARTITIONS: usize> Exclusive<T, Tag, MAX, false, PARTITIONS> {
    pub fn with_capacity(capacity: Capacity) -> Self {
        match Self::try_with_capacity(capacity) {
            Ok(slab) => slab,
            Err(error) => error.abort(),
        }
    }

    /// Reserves every slot and dense index entry transactionally.
    pub fn try_with_capacity(capacity: Capacity) -> Result<Self, collections::AllocationError> {
        use core::Core;
        Ok(Self {
            core: Core::try_with_capacity(capacity)?,
            tag: marker::PhantomData,
        })
    }
}

impl<T, Tag, const MAX: u32, const PARTITIONS: usize> slab::raw::Recycling<T, Tag, MAX>
    for Exclusive<T, Tag, MAX, true, PARTITIONS>
{
    unsafe fn try_with_capacity_recycling(
        capacity: Capacity,
    ) -> Result<Self, collections::AllocationError> {
        use core::Core;
        Ok(Self {
            core: Core::try_with_capacity(capacity)?,
            tag: marker::PhantomData,
        })
    }
}

impl<T, Tag, const MAX: u32, const RECYCLE: bool, const PARTITIONS: usize>
    slab::raw::ExternalAccess<T, Tag, MAX> for Exclusive<T, Tag, MAX, RECYCLE, PARTITIONS>
{
    fn entry_at(&self, index: u32) -> Option<(&T, key::Handle<Tag, MAX>)> {
        self.core
            .entries()
            .index(index)
            .map(|(value, generation)| (value, key::Handle::new(index, generation)))
    }

    fn entry_at_mut(&mut self, index: u32) -> Option<(&mut T, key::Handle<Tag, MAX>)> {
        self.core
            .index_mut(index)
            .map(|(value, generation)| (value, key::Handle::new(index, generation)))
    }
}

impl<const MAX: u32> GenerationState for key::Generation<MAX> {
    const MIN: Self = Self::MIN;
    const VALID: () = assert!(MAX != 0, "generation limit must be nonzero");

    fn next(self) -> Option<Self> {
        self.checked_add(1)
    }
}

impl<T, Tag, const MAX: u32, const RECYCLE: bool, const PARTITIONS: usize>
    Exclusive<T, Tag, MAX, RECYCLE, PARTITIONS>
{
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
        self.core.available() == 0
    }

    /// Counts free, reusable slots without growing.
    /// This cold O(capacity) scan excludes retired generations and reservations,
    /// preserving the slab's layout and hot-path cost.
    pub fn available(&self) -> usize {
        self.core.available()
    }

    pub fn insert(&mut self, value: T) -> Result<key::Handle<Tag, MAX>, T> {
        self.insert_entry(value).map(|(key, _)| key)
    }

    pub fn insert_entry(&mut self, value: T) -> Result<(key::Handle<Tag, MAX>, &mut T), T> {
        let Some(ticket) = self.core.reservations().take_free() else {
            return Err(value);
        };
        use crate::collections::slab::sealed::pending::Pending;
        let key = key::Handle::new(ticket.index.get(), ticket.generation);
        Pending::new(&self.core, ticket).commit(value);
        let value = unsafe {
            self.core
                .get_mut(ticket.index.get(), ticket.generation)
                .unwrap_unchecked()
        };
        Ok((key, value))
    }

    pub fn vacant_entry(&mut self) -> Option<VacantEntry<'_, T, Tag, MAX, RECYCLE, PARTITIONS>> {
        let ticket = self.core.reservations().take_free()?;
        Some(VacantEntry {
            slab: self,
            ticket: Some(ticket),
        })
    }

    pub fn slots(&mut self) -> Slots<'_, T, Tag, MAX, RECYCLE, PARTITIONS> {
        Slots { slab: self }
    }
}

impl<T, Tag, const MAX: u32, const RECYCLE: bool, const PARTITIONS: usize>
    Exclusive<T, Tag, MAX, RECYCLE, PARTITIONS>
{
    pub fn get(&self, key: key::Handle<Tag, MAX>) -> Option<&T> {
        self.core.entries().get(key.index(), key.generation())
    }

    pub fn get_mut(&mut self, key: key::Handle<Tag, MAX>) -> Option<&mut T> {
        self.core.get_mut(key.index(), key.generation())
    }

    pub fn occupied_entry(
        &mut self,
        key: key::Handle<Tag, MAX>,
    ) -> Option<OccupiedEntry<'_, T, Tag, MAX, RECYCLE, PARTITIONS>> {
        Some(OccupiedEntry {
            busy: core::Busy::take_key(&self.core, key.index(), key.generation())?,
            tag: marker::PhantomData,
        })
    }

    pub fn remove(&mut self, key: key::Handle<Tag, MAX>) -> Option<T> {
        let (value, _) = self.core.entries().remove(key.index(), key.generation())?;
        Some(value)
    }

    pub fn clear(&mut self) {
        self.core.clear();
    }
}

impl<'a, T, Tag, const MAX: u32, const RECYCLE: bool, const PARTITIONS: usize>
    Slots<'a, T, Tag, MAX, RECYCLE, PARTITIONS>
{
    pub fn vacant_entry_at(
        self,
        index: u32,
    ) -> Option<VacantEntry<'a, T, Tag, MAX, RECYCLE, PARTITIONS>> {
        let ticket = self.slab.core.reservations().take_index(index)?;
        Some(VacantEntry {
            slab: self.slab,
            ticket: Some(ticket),
        })
    }

    /// Reserves the first reusable slot in `range` without leaving it.
    /// Retired, occupied, and reserved slots are skipped.
    pub fn vacant_entry_in(
        self,
        range: ops::Range<u32>,
    ) -> Option<VacantEntry<'a, T, Tag, MAX, RECYCLE, PARTITIONS>> {
        let ticket = self.slab.core.reservations().take_range(range)?;
        Some(VacantEntry {
            slab: self.slab,
            ticket: Some(ticket),
        })
    }

    pub fn parts(self, parts: key::Parts<MAX>) -> Option<&'a T> {
        self.slab
            .core
            .entries()
            .get(parts.index(), parts.generation())
    }

    pub fn parts_mut(self, parts: key::Parts<MAX>) -> Option<&'a mut T> {
        self.slab.core.get_mut(parts.index(), parts.generation())
    }

    pub fn occupied_entry_parts(
        self,
        parts: key::Parts<MAX>,
    ) -> Option<OccupiedEntry<'a, T, Tag, MAX, RECYCLE, PARTITIONS>> {
        Some(OccupiedEntry {
            busy: core::Busy::take_key(&self.slab.core, parts.index(), parts.generation())?,
            tag: marker::PhantomData,
        })
    }

    pub fn occupied_entry_at(
        self,
        index: u32,
    ) -> Option<OccupiedEntry<'a, T, Tag, MAX, RECYCLE, PARTITIONS>> {
        Some(OccupiedEntry {
            busy: core::Busy::take(&self.slab.core, index)?,
            tag: marker::PhantomData,
        })
    }

    pub fn remove_parts(self, parts: key::Parts<MAX>) -> Option<T> {
        self.slab
            .core
            .entries()
            .remove(parts.index(), parts.generation())
            .map(|(value, _)| value)
    }

    pub fn remove_index_with<R>(
        self,
        index: u32,
        f: impl FnOnce(&mut T, key::Handle<Tag, MAX>) -> Option<R>,
    ) -> Option<(T, R)> {
        self.slab
            .core
            .entries()
            .remove_index_with(index, |value, generation| {
                f(value, key::Handle::new(index, generation))
            })
            .map(|(value, result, _)| (value, result))
    }

    pub fn index(self, index: u32) -> Option<(&'a T, key::Handle<Tag, MAX>)> {
        self.slab
            .core
            .entries()
            .index(index)
            .map(|(value, generation)| (value, key::Handle::new(index, generation)))
    }

    pub fn index_mut(self, index: u32) -> Option<(&'a mut T, key::Handle<Tag, MAX>)> {
        self.slab
            .core
            .index_mut(index)
            .map(|(value, generation)| (value, key::Handle::new(index, generation)))
    }

    pub fn key(self, index: u32) -> Option<key::Handle<Tag, MAX>> {
        self.slab
            .core
            .entries()
            .generation(index)
            .map(|generation| key::Handle::new(index, generation))
    }

    pub fn values(self) -> impl Iterator<Item = &'a T>
    where
        T: 'a,
    {
        self.slab.core.entries().values()
    }

    pub fn values_mut(self) -> impl Iterator<Item = &'a mut T>
    where
        T: 'a,
    {
        self.slab.core.values_mut()
    }
}

impl<T, Tag, const MAX: u32, const RECYCLE: bool, const PARTITIONS: usize>
    OccupiedEntry<'_, T, Tag, MAX, RECYCLE, PARTITIONS>
{
    pub fn key(&self) -> key::Handle<Tag, MAX> {
        key::Handle::new(self.busy.index(), self.busy.generation())
    }

    pub fn get(&self) -> &T {
        self.busy.value()
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.busy.value_mut()
    }

    pub fn remove(self) -> T {
        self.busy.commit_removal().0
    }
}
