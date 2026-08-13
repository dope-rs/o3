use std::{cell, fmt, hash, marker, ops};

use crate::collections::{self, slab};

mod sealed;

pub(crate) use sealed::Backing;

/// A non-wrapping logical generation; `None` permanently exhausts identity.
pub trait Generation: Copy + Eq {
    const INITIAL: Self;

    #[must_use]
    fn next(self) -> Option<Self>;
}

/// A typed external identity independent of the slab's private generation.
#[repr(C)]
pub struct Key<G: Generation, Tag = ()> {
    generation: G,
    index: u32,
    tag: marker::PhantomData<*mut Tag>,
}

struct Stamped<T, G: Generation> {
    generation: G,
    value: T,
}

type BuildResult<G, Tag, I, T, R, E> = Result<(Key<G, Tag>, R), slab::BuildError<I, T, E>>;

/// Exclusive storage whose private physical generations may recycle while
/// externally visible logical generations never do.
pub struct Exclusive<
    T,
    G: Generation,
    Tag = (),
    const PHYSICAL_MAX: u32 = { u32::MAX },
    const PARTITIONS: usize = 1,
> {
    inner: slab::Exclusive<Stamped<T, G>, Tag, PHYSICAL_MAX, true, PARTITIONS>,
    next: Option<G>,
}

/// Interior-mutable storage with independent non-wrapping logical identities.
pub struct Cell<T, G: Generation, Tag = (), const PHYSICAL_MAX: u32 = { u32::MAX }> {
    inner: slab::Cell<Stamped<T, G>, Tag, PHYSICAL_MAX, true>,
    next: cell::Cell<Option<G>>,
}

pub struct Entries<
    'a,
    T,
    G: Generation,
    Tag = (),
    const PHYSICAL_MAX: u32 = { u32::MAX },
    const PARTITIONS: usize = 1,
> {
    slab: &'a Exclusive<T, G, Tag, PHYSICAL_MAX, PARTITIONS>,
}

pub struct EntriesMut<
    'a,
    T,
    G: Generation,
    Tag = (),
    const PHYSICAL_MAX: u32 = { u32::MAX },
    const PARTITIONS: usize = 1,
> {
    slab: &'a mut Exclusive<T, G, Tag, PHYSICAL_MAX, PARTITIONS>,
}

pub struct OccupiedEntry<
    'a,
    T,
    G: Generation,
    Tag = (),
    const PHYSICAL_MAX: u32 = { u32::MAX },
    const PARTITIONS: usize = 1,
> {
    inner: slab::OccupiedEntry<'a, Stamped<T, G>, Tag, PHYSICAL_MAX, true, PARTITIONS>,
    generation: marker::PhantomData<G>,
}

pub struct VacantEntry<
    'a,
    T,
    G: Generation,
    Tag = (),
    const PHYSICAL_MAX: u32 = { u32::MAX },
    const PARTITIONS: usize = 1,
> {
    inner: slab::VacantEntry<'a, Stamped<T, G>, Tag, PHYSICAL_MAX, true, PARTITIONS>,
    generation: G,
}

impl<G: Generation, Tag> Key<G, Tag> {
    #[must_use]
    pub const fn new(index: u32, generation: G) -> Self {
        Self {
            generation,
            index,
            tag: marker::PhantomData,
        }
    }

    #[must_use]
    pub const fn index(self) -> u32 {
        self.index
    }

    #[must_use]
    pub const fn generation(self) -> G {
        self.generation
    }

    #[must_use]
    pub const fn retag<Other>(self) -> Key<G, Other> {
        Key::new(self.index, self.generation)
    }
}

impl<G: Generation, Tag> Clone for Key<G, Tag> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<G: Generation, Tag> Copy for Key<G, Tag> {}

impl<G: Generation, Tag> PartialEq for Key<G, Tag> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.generation == other.generation
    }
}

impl<G: Generation, Tag> Eq for Key<G, Tag> {}

impl<G: Generation + hash::Hash, Tag> hash::Hash for Key<G, Tag> {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.generation.hash(state);
        self.index.hash(state);
    }
}

impl<G: Generation + fmt::Debug, Tag> fmt::Debug for Key<G, Tag> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Key")
            .field("index", &self.index)
            .field("generation", &self.generation)
            .finish()
    }
}

fn take_generation<G: Generation>(next: &mut Option<G>) -> Option<G> {
    let generation = next.take()?;
    *next = generation.next();
    Some(generation)
}

impl<T, G: Generation, Tag, const PHYSICAL_MAX: u32, const PARTITIONS: usize>
    Exclusive<T, G, Tag, PHYSICAL_MAX, PARTITIONS>
{
    pub fn with_capacity(capacity: slab::Capacity) -> Self {
        match Self::try_with_capacity(capacity) {
            Ok(slab) => slab,
            Err(error) => error.abort(),
        }
    }

    pub fn try_with_capacity(
        capacity: slab::Capacity,
    ) -> Result<Self, collections::AllocationError> {
        let inner = Backing::external(capacity)?;
        Ok(Self {
            inner,
            next: Some(G::INITIAL),
        })
    }

    pub fn capacity(&self) -> usize {
        self.inner.capacity()
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn available(&self) -> usize {
        self.inner.available()
    }

    pub fn insert(&mut self, value: T) -> Result<Key<G, Tag>, T> {
        let Some(vacant) = self.vacant_entry() else {
            return Err(value);
        };
        Ok(vacant.insert(value))
    }

    pub fn vacant_entry(&mut self) -> Option<VacantEntry<'_, T, G, Tag, PHYSICAL_MAX, PARTITIONS>> {
        let inner = self.inner.vacant_entry()?;
        let generation = take_generation(&mut self.next)?;
        Some(VacantEntry { inner, generation })
    }

    pub fn vacant_entry_at(
        &mut self,
        index: u32,
    ) -> Option<VacantEntry<'_, T, G, Tag, PHYSICAL_MAX, PARTITIONS>> {
        let inner = self.inner.slots().vacant_entry_at(index)?;
        let generation = take_generation(&mut self.next)?;
        Some(VacantEntry { inner, generation })
    }

    pub fn vacant_entry_in(
        &mut self,
        range: ops::Range<u32>,
    ) -> Option<VacantEntry<'_, T, G, Tag, PHYSICAL_MAX, PARTITIONS>> {
        let inner = self.inner.slots().vacant_entry_in(range)?;
        let generation = take_generation(&mut self.next)?;
        Some(VacantEntry { inner, generation })
    }

    pub fn get(&self, key: Key<G, Tag>) -> Option<&T> {
        self.entries().get(key)
    }

    pub fn occupied_entry(
        &mut self,
        key: Key<G, Tag>,
    ) -> Option<OccupiedEntry<'_, T, G, Tag, PHYSICAL_MAX, PARTITIONS>> {
        let inner = self.inner.slots().occupied_entry_at(key.index())?;
        (inner.get().generation == key.generation()).then_some(OccupiedEntry {
            inner,
            generation: marker::PhantomData,
        })
    }

    pub fn entries(&self) -> Entries<'_, T, G, Tag, PHYSICAL_MAX, PARTITIONS> {
        Entries { slab: self }
    }

    pub fn entries_mut(&mut self) -> EntriesMut<'_, T, G, Tag, PHYSICAL_MAX, PARTITIONS> {
        EntriesMut { slab: self }
    }

    pub fn remove(&mut self, key: Key<G, Tag>) -> Option<T> {
        self.occupied_entry(key).map(OccupiedEntry::remove)
    }

    pub fn clear(&mut self) {
        self.inner.clear();
    }
}

impl<'a, T, G: Generation, Tag, const PHYSICAL_MAX: u32, const PARTITIONS: usize>
    Entries<'a, T, G, Tag, PHYSICAL_MAX, PARTITIONS>
{
    pub fn get(self, key: Key<G, Tag>) -> Option<&'a T> {
        let (stamped, _) = slab::raw::ExternalAccess::entry_at(&self.slab.inner, key.index())?;
        (stamped.generation == key.generation()).then_some(&stamped.value)
    }

    pub fn current(self, index: u32) -> Option<(&'a T, Key<G, Tag>)> {
        let (stamped, _) = slab::raw::ExternalAccess::entry_at(&self.slab.inner, index)?;
        Some((&stamped.value, Key::new(index, stamped.generation)))
    }

    pub fn values(self) -> impl Iterator<Item = &'a T>
    where
        T: 'a,
        G: 'a,
    {
        (0..self.slab.inner.capacity() as u32).filter_map(|index| {
            slab::raw::ExternalAccess::entry_at(&self.slab.inner, index)
                .map(|(stamped, _)| &stamped.value)
        })
    }
}

impl<'a, T, G: Generation, Tag, const PHYSICAL_MAX: u32, const PARTITIONS: usize>
    EntriesMut<'a, T, G, Tag, PHYSICAL_MAX, PARTITIONS>
{
    pub fn get(self, key: Key<G, Tag>) -> Option<&'a mut T> {
        let (stamped, _) =
            slab::raw::ExternalAccess::entry_at_mut(&mut self.slab.inner, key.index())?;
        (stamped.generation == key.generation()).then_some(&mut stamped.value)
    }

    pub fn values(self) -> impl Iterator<Item = &'a mut T>
    where
        T: 'a,
        G: 'a,
    {
        self.slab
            .inner
            .slots()
            .values_mut()
            .map(|stamped| &mut stamped.value)
    }
}

impl<T, G: Generation, Tag, const PHYSICAL_MAX: u32, const PARTITIONS: usize>
    OccupiedEntry<'_, T, G, Tag, PHYSICAL_MAX, PARTITIONS>
{
    pub fn key(&self) -> Key<G, Tag> {
        Key::new(self.inner.key().index(), self.inner.get().generation)
    }

    pub fn get(&self) -> &T {
        &self.inner.get().value
    }

    pub fn get_mut(&mut self) -> &mut T {
        &mut self.inner.get_mut().value
    }

    pub fn remove(self) -> T {
        self.inner.remove().value
    }
}

impl<'a, T, G: Generation, Tag, const PHYSICAL_MAX: u32, const PARTITIONS: usize>
    VacantEntry<'a, T, G, Tag, PHYSICAL_MAX, PARTITIONS>
{
    pub fn key(&self) -> Key<G, Tag> {
        Key::new(self.inner.key().index(), self.generation)
    }

    pub fn insert(self, value: T) -> Key<G, Tag> {
        let key = self.key();
        let _ = self.inner.insert(Stamped {
            generation: self.generation,
            value,
        });
        key
    }

    pub fn insert_occupied(
        self,
        value: T,
    ) -> OccupiedEntry<'a, T, G, Tag, PHYSICAL_MAX, PARTITIONS> {
        OccupiedEntry {
            inner: self.inner.insert_occupied(Stamped {
                generation: self.generation,
                value,
            }),
            generation: marker::PhantomData,
        }
    }
}

impl<T, G: Generation, Tag, const PHYSICAL_MAX: u32> Cell<T, G, Tag, PHYSICAL_MAX> {
    pub fn with_capacity(capacity: slab::Capacity) -> Self {
        match Self::try_with_capacity(capacity) {
            Ok(slab) => slab,
            Err(error) => error.abort(),
        }
    }

    pub fn try_with_capacity(
        capacity: slab::Capacity,
    ) -> Result<Self, collections::AllocationError> {
        let inner = Backing::external(capacity)?;
        Ok(Self {
            inner,
            next: cell::Cell::new(Some(G::INITIAL)),
        })
    }

    fn take_generation(&self) -> Option<G> {
        let generation = self.next.take()?;
        self.next.set(generation.next());
        Some(generation)
    }

    pub fn capacity(&self) -> usize {
        self.inner.capacity()
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn available(&self) -> usize {
        self.inner.available()
    }

    pub fn key_at(&self, position: usize) -> Option<Key<G, Tag>> {
        let raw = self.inner.slots().key_at(position)?;
        self.inner
            .update(raw, |stamped| Key::new(raw.index(), stamped.generation))
    }

    pub fn keys(&self) -> impl Iterator<Item = Key<G, Tag>> + '_ {
        self.inner.keys().filter_map(|raw| {
            self.inner
                .update(raw, |stamped| Key::new(raw.index(), stamped.generation))
        })
    }

    pub fn any_or_busy(&self, mut predicate: impl FnMut(&T) -> bool) -> bool {
        self.inner.any_or_busy(|stamped| predicate(&stamped.value))
    }

    pub fn insert(&self, value: T) -> Result<Key<G, Tag>, T> {
        let Some(generation) = self.take_generation() else {
            return Err(value);
        };
        self.inner
            .insert(Stamped { generation, value })
            .map(|raw| Key::new(raw.index(), generation))
            .map_err(|stamped| stamped.value)
    }

    pub fn try_insert_with<R, E>(
        &self,
        value: T,
        f: impl FnOnce(Key<G, Tag>, &mut T) -> Result<R, E>,
    ) -> Result<(Key<G, Tag>, R), slab::InsertError<T, E>> {
        let Some(generation) = self.take_generation() else {
            return Err(slab::InsertError::Full(value));
        };
        self.inner
            .try_insert_with(Stamped { generation, value }, |raw, stamped| {
                f(Key::new(raw.index(), generation), &mut stamped.value)
            })
            .map(|(raw, result)| (Key::new(raw.index(), generation), result))
            .map_err(|error| match error {
                slab::InsertError::Full(stamped) => slab::InsertError::Full(stamped.value),
                slab::InsertError::Rejected(stamped, error) => {
                    slab::InsertError::Rejected(stamped.value, error)
                }
            })
    }

    pub fn try_insert_build<I, R, E>(
        &self,
        input: I,
        build: impl FnOnce(I) -> T,
        f: impl FnOnce(Key<G, Tag>, &mut T) -> Result<R, E>,
    ) -> BuildResult<G, Tag, I, T, R, E> {
        let Some(generation) = self.take_generation() else {
            return Err(slab::BuildError::Full(input));
        };
        self.inner
            .try_insert_build(
                input,
                |input| Stamped {
                    generation,
                    value: build(input),
                },
                |raw, stamped| f(Key::new(raw.index(), generation), &mut stamped.value),
            )
            .map(|(raw, result)| (Key::new(raw.index(), generation), result))
            .map_err(|error| match error {
                slab::BuildError::Full(input) => slab::BuildError::Full(input),
                slab::BuildError::Rejected(stamped, error) => {
                    slab::BuildError::Rejected(stamped.value, error)
                }
            })
    }

    pub fn update<R>(&self, key: Key<G, Tag>, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        self.inner
            .slots()
            .update_index(key.index(), |stamped| {
                (stamped.generation == key.generation()).then(|| f(&mut stamped.value))
            })
            .flatten()
    }

    pub fn remove(&self, key: Key<G, Tag>) -> Option<T> {
        self.remove_with(key, |_| Some(())).map(|(value, ())| value)
    }

    pub fn remove_with<R>(
        &self,
        key: Key<G, Tag>,
        f: impl FnOnce(&mut T) -> Option<R>,
    ) -> Option<(T, R)> {
        self.inner
            .slots()
            .remove_index_with(key.index(), |stamped| {
                (stamped.generation == key.generation())
                    .then(|| f(&mut stamped.value))
                    .flatten()
            })
            .map(|(stamped, result)| (stamped.value, result))
    }
}
