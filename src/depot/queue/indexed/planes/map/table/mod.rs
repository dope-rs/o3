use planes::map::{Error, Occupied, Vacancy};

use crate::{
    alloc,
    depot::{
        map,
        queue::{
            indexed::{
                planes,
                planes::{
                    map::{Core, Record},
                    phase,
                },
            },
            links,
        },
    },
};

/// Fixed physical slots, a hash directory, and independent FIFO memberships.
/// Plane zero owns installation and can only be changed by insert/remove.
pub struct Table<K, V, const N: usize> {
    pub(in crate::depot::queue::indexed::planes::map) core: Core<K, V, N>,
    pub(in crate::depot::queue::indexed::planes::map) directory: map::raw::Table<links::Index>,
    pub(in crate::depot::queue::indexed::planes::map) shape: map::raw::Shape<links::Index>,
}

impl<K, V, const N: usize> Table<K, V, N> {
    pub fn try_new(plan: map::Plan, slots: usize) -> Result<Self, alloc::Error> {
        let layout = map::raw::Layout::try_plan(plan)?;
        let core = Core::try_new(slots)?;
        let shape = layout.shape();
        let table = map::raw::Table::try_new(layout)?;
        Ok(Self {
            core,
            directory: table,
            shape,
        })
    }

    pub fn try_vacant<Q: Eq + ?Sized>(
        &mut self,
        index: usize,
        hash: u64,
        key: K,
    ) -> Result<Vacancy<'_, K, V, N>, Error<K>>
    where
        K: Copy + AsRef<Q>,
    {
        use planes::map::vacancy::Place;

        let place = Place::try_new(self, index, key)?;
        map::raw::Spare::try_reserve(place, hash, key.as_ref()).map(Into::into)
    }

    pub fn get<Q: Eq + ?Sized>(&self, hash: u64, key: &Q) -> Option<&Record<K, V>>
    where
        K: AsRef<Q>,
    {
        map::raw::Probe::find(self, hash, |stored| stored.as_ref() == key)
    }

    pub fn entry<Q: Eq + ?Sized>(&mut self, hash: u64, key: &Q) -> Option<Occupied<'_, K, V, N>>
    where
        K: AsRef<Q>,
    {
        map::raw::Probe::find(self, hash, |stored| stored.as_ref() == key)
    }

    pub fn at(&self, index: usize) -> Option<&Record<K, V>> {
        let node = self.core.nodes.get(index)?;
        if !node.hooks[0].is_linked(index) {
            return None;
        }
        let record = unsafe { (*node.record.get()).assume_init_ref() };
        Some(record)
    }

    pub fn slot(&mut self, index: usize) -> Option<Occupied<'_, K, V, N>> {
        let node = self.core.nodes.get(index)?;
        if !node.hooks[0].is_linked(index) {
            return None;
        }
        let index = unsafe { links::Index::new_usize(index) };
        Some(Occupied::new(self, index))
    }

    pub fn front<const P: usize>(&self) -> Option<&Record<K, V>> {
        const { assert!(P < N) };
        let index = self.core.queues[P].first()?;
        let record = unsafe { self.core.record(index) };
        Some(record)
    }

    pub fn first_entry(&mut self) -> Option<Occupied<'_, K, V, N>> {
        let index = self.core.queues[0].first()?;
        Some(Occupied::new(self, index))
    }

    pub fn front_entry<const P: usize>(
        &mut self,
    ) -> Option<Occupied<'_, K, V, N, phase::Front<P>>> {
        const { assert!(P > 0 && P < N) };
        let index = self.core.queues[P].first()?;
        Some(Occupied::new(self, index))
    }

    pub fn is_empty<const P: usize>(&self) -> bool {
        const { assert!(P < N) };
        self.core.queues[P].first().is_none()
    }

    pub fn len_in<const P: usize>(&self) -> usize {
        const { assert!(P < N) };
        self.core.queues[P].len()
    }
}

/// A directory query retaining the exact installed-record owner.
/// # Safety
/// Query's directory and every published payload belong to this unchanged owner.
pub(in crate::depot::queue::indexed::planes::map) unsafe trait Source<K, V, const N: usize>:
    map::raw::Query<Payload = links::Index>
{
    fn owner(&self) -> &Table<K, V, N>;

    fn record(entry: map::Found<&Self>) -> &Record<K, V> {
        let (source, index) = entry.project();
        // SAFETY: the found bucket and Source retain this exact installed record.
        unsafe { source.owner().core.record(*index) }
    }
}

// SAFETY: both query implementations project the same borrowed table's directory.
unsafe impl<K, V, const N: usize> Source<K, V, N> for &Table<K, V, N> {
    fn owner(&self) -> &Table<K, V, N> {
        self
    }
}

// SAFETY: the exclusive query retains this same installed-record owner.
unsafe impl<K, V, const N: usize> Source<K, V, N> for &mut Table<K, V, N> {
    fn owner(&self) -> &Table<K, V, N> {
        self
    }
}

// SAFETY: only installation publishes a directory index; removal retires it first.
unsafe impl<'a, K, V, const N: usize> map::raw::Query for &'a Table<K, V, N> {
    const SPARE: bool = true;
    type Payload = links::Index;
    type Value = K;
    type Occupied = &'a Record<K, V>;

    fn table(&self) -> &map::raw::Table<Self::Payload> {
        &self.directory
    }

    fn value(entry: map::Found<&Self>) -> &K {
        &Self::record(entry).key
    }

    fn occupied(entry: map::Found<Self>) -> Self::Occupied {
        let (owner, index) = entry.copied();
        // SAFETY: the consumed directory proof retains its complete shared owner lifetime.
        unsafe { owner.core.record(index) }
    }
}

// SAFETY: the exclusive owner preserves the located directory and initialized record.
unsafe impl<'a, K, V, const N: usize> map::raw::Query for &'a mut Table<K, V, N> {
    const SPARE: bool = true;
    type Payload = links::Index;
    type Value = K;
    type Occupied = Occupied<'a, K, V, N>;

    fn table(&self) -> &map::raw::Table<Self::Payload> {
        &self.directory
    }

    fn value(entry: map::Found<&Self>) -> &K {
        &Self::record(entry).key
    }

    fn occupied(entry: map::Found<Self>) -> Self::Occupied {
        let (owner, index) = entry.copied();
        Occupied::new(owner, index)
    }
}

// SAFETY: directory shifts update the same initialized records' reverse buckets.

impl<K, V, const N: usize> Drop for Table<K, V, N> {
    fn drop(&mut self) {
        let layout = self.shape.layout();
        unsafe { layout.bundle().deallocate(self.directory.base()) };
    }
}

mod raw;
