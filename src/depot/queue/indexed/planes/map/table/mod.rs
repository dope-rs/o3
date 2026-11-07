use std::{array::from_fn, cell, mem};

use planes::map::{Error, Occupied, Vacancy};

use crate::{
    alloc::{self, array},
    depot::{
        capacity, map,
        queue::{
            indexed::{
                planes,
                planes::{
                    map::{Node, Record},
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
    pub(in crate::depot::queue::indexed::planes::map) nodes:
        array::Block<Node<K, V, N>, capacity::Count>,
    pub(in crate::depot::queue::indexed::planes::map) queues: [links::Chain<u32>; N],
    pub(in crate::depot::queue::indexed::planes::map) directory: map::raw::Table<links::Index>,
    pub(in crate::depot::queue::indexed::planes::map) shape: map::raw::Shape<links::Index>,
}

impl<K, V, const N: usize> Table<K, V, N> {
    pub fn try_new(plan: map::Plan, slots: usize) -> Result<Self, alloc::Error> {
        const { assert!(N > 0) };
        let layout = map::raw::Layout::try_plan(plan)?;
        let count = capacity::Count::try_count(slots, alloc::Error::Overflow)?;
        let nodes = array::Block::try_new(count, ())?.init(|index| {
            // SAFETY: the retained count excludes the compact sentinel.
            let index = unsafe { links::Index::new_usize(index) };
            Node {
                record: cell::UnsafeCell::new(mem::MaybeUninit::uninit()),
                hooks: from_fn(|_| links::Hook::new(index)),
            }
        });
        let shape = layout.shape();
        let table = map::raw::Table::try_new(layout)?;
        Ok(Self {
            nodes,
            queues: [const { links::Chain::new() }; N],
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
        let key = key.as_ref();
        // SAFETY: Place retains installed < capacity < buckets under this exclusive loan.
        unsafe { map::raw::Probe::spare(place, hash, |value| value.as_ref() == key) }
            .map(Into::into)
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
        let node = self.nodes.get(index)?;
        if !node.hooks[0].is_linked(index) {
            return None;
        }
        let record = unsafe { (*node.record.get()).assume_init_ref() };
        Some(record)
    }

    pub fn slot(&mut self, index: usize) -> Option<Occupied<'_, K, V, N>> {
        let node = self.nodes.get(index)?;
        if !node.hooks[0].is_linked(index) {
            return None;
        }
        let index = unsafe { links::Index::new_usize(index) };
        Some(Occupied::new(self, index))
    }

    pub fn front<const P: usize>(&self) -> Option<&Record<K, V>> {
        const { assert!(P < N) };
        let index = self.queues[P].first()?;
        let record = unsafe { self.record(index) };
        Some(record)
    }

    pub fn first_entry(&mut self) -> Option<Occupied<'_, K, V, N>> {
        let index = self.queues[0].first()?;
        Some(Occupied::new(self, index))
    }

    pub fn front_entry<const P: usize>(
        &mut self,
    ) -> Option<Occupied<'_, K, V, N, phase::Front<P>>> {
        const { assert!(P > 0 && P < N) };
        let index = self.queues[P].first()?;
        Some(Occupied::new(self, index))
    }

    pub fn is_empty<const P: usize>(&self) -> bool {
        const { assert!(P < N) };
        self.queues[P].first().is_none()
    }

    pub fn len_in<const P: usize>(&self) -> usize {
        const { assert!(P < N) };
        self.queues[P].len()
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
        let (owner, index) = entry.project();
        // SAFETY: the found bucket retains this owner's installed record.
        &unsafe { owner.record(*index) }.key
    }

    fn occupied(entry: map::Found<Self>) -> Self::Occupied {
        let (owner, index) = entry.copied();
        // SAFETY: the consumed directory proof retains its complete shared owner lifetime.
        unsafe { owner.record(index) }
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
        let (owner, index) = entry.project();
        // SAFETY: the found bucket retains this owner's installed record.
        &unsafe { owner.record(*index) }.key
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
        if !mem::needs_drop::<Record<K, V>>() {
            return;
        }
        let mut next = self.queues[0].first();
        while let Some(index) = next {
            let node = unsafe { self.node(index) };
            next = node.hooks[0].next_index();
            unsafe { (*node.record.get()).assume_init_drop() };
        }
    }
}

mod raw;
