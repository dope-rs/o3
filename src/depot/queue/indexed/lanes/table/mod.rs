use std::mem;

use node::Node;

use crate::depot::{array::Index, queue::links};
pub(in crate::depot) mod node;

use lanes::entry;

use crate::depot::{capacity, index::Initialized, map::direct, queue::indexed::lanes};

pub(in crate::depot) struct Hooks<
    'a,
    K: crate::depot::raw::Key,
    V,
    const N: usize,
    S: lanes::raw::State<V> = (),
>(pub(in crate::depot) &'a direct::Table<K, Node<V, N, S>>);

/// Key-indexed values and their exclusive FIFO membership.
pub struct Table<K: crate::depot::raw::Key, V, const N: usize, S: lanes::raw::State<V> = ()> {
    pub(in crate::depot) values: direct::Table<K, Node<V, N, S>>,
    pub(in crate::depot) queues: [links::Chain; N],
}

// SAFETY: the retained table admits stable hooks at every initialized index.
unsafe impl<K: crate::depot::raw::Key, V, const N: usize, S: lanes::raw::State<V>>
    crate::depot::raw::Hooks for Hooks<'_, K, V, N, S>
{
    unsafe fn hook_raw(&self, index: links::Index) -> &links::Hook {
        // SAFETY: this exact table retains the admitted initialized hook slot.
        &unsafe { self.0.value(index) }.hook
    }
}

impl<K: crate::depot::raw::Key, V, const N: usize, S: lanes::raw::State<V>> Table<K, V, N, S> {
    /// Bytes reserved for each exact key, value, and its lane links.
    pub const NODE_BYTES: usize = mem::size_of::<direct::raw::Entry<K, Node<V, N, S>>>();

    pub fn try_new(cap: capacity::Count) -> Result<Self, crate::alloc::Error> {
        const { assert!(N <= u8::MAX as usize) };
        Ok(Self {
            values: direct::Table::try_new(cap)?,
            queues: [const { links::Chain::new() }; N],
        })
    }

    pub const fn capacity(&self) -> usize {
        self.values.capacity()
    }

    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub const fn len(&self) -> usize {
        self.values.len()
    }

    pub fn get(&self, key: K) -> Option<&V> {
        self.values.get(key).map(|node| &*node.value)
    }

    /// Returns the stored identity and value from one occupied index.
    pub fn entry_at(&self, index: usize) -> Option<(K, &V)> {
        let (key, node) = self.values.entry_at(index)?;
        Some((key, &*node.value))
    }

    pub fn key_at(&self, index: usize) -> Option<K> {
        self.values.key_at(index)
    }

    pub fn lane(&self, key: K) -> Option<Option<Index<N>>> {
        Some(self.values.get(key)?.lane.get())
    }

    pub fn try_vacant(
        &mut self,
        key: K,
    ) -> Result<entry::Vacancy<'_, K, V, N, S>, direct::Error<K>> {
        let slot = self.values.try_vacant(key)?;
        Ok(entry::Slot::new(slot, &self.queues))
    }

    pub fn vacant(&mut self, key: K) -> Option<entry::Vacancy<'_, K, V, N, S>> {
        let slot = self.values.vacant(key)?;
        Some(entry::Slot::new(slot, &self.queues))
    }

    pub fn entry(&mut self, key: K) -> Option<entry::Occupied<'_, K, V, N, S>> {
        let Self { values, queues } = self;
        let slot = values.entry(key)?;
        Some(entry::Slot::new(slot, queues))
    }

    pub fn front(&mut self, lane: Index<N>) -> Option<entry::Occupied<'_, K, V, N, S>> {
        let Self { values, queues } = self;
        let index = lane.get(queues).front()?;
        // SAFETY: membership in this exact queue proves its front is occupied
        // in the paired value store.
        let slot = unsafe { direct::raw::Entry::occupied(values, index) };
        Some(entry::Slot::new(slot, queues))
    }

    pub fn back(&self, lane: Index<N>) -> Option<K> {
        let index = lane.get(&self.queues).last()?;
        // SAFETY: this exact queue's retained tail belongs to the paired table.
        let live = unsafe {
            crate::depot::raw::Proof::<(_, _), Initialized<()>>::new((&self.values, index))
        };
        Some(direct::Table::project(live).into_inner().1)
    }

    pub fn is_vacant(&self, lane: Index<N>) -> bool {
        lane.get(&self.queues).is_empty()
    }

    pub fn move_all(&mut self, source: Index<N>, target: Index<N>) -> usize {
        if source == target {
            return 0;
        }
        let mut moved = 0;
        let mut next = source.get(&self.queues).first();
        while let Some(index) = next {
            // SAFETY: the original queue proves this exact initialized member.
            let node = unsafe { self.values.value(index) };
            next = node.hook.next_index();
            node.lane.set(Some(target));
            moved += 1;
        }
        if moved != 0 {
            unsafe {
                target
                    .get(&self.queues)
                    .append(source.get(&self.queues), &Hooks(&self.values));
            }
        }
        moved
    }

    pub fn clear(&mut self) {
        if self.is_empty() {
            return;
        }
        self.queues = [const { links::Chain::new() }; N];
        self.values.clear();
    }
}
