use std::marker;

pub use error::Error;
pub use state::State;

use crate::depot::{
    array::Index,
    map::direct,
    queue::{indexed::lanes, links},
};

mod error;
mod state;

/// One native direct-table slot and the queue topology governed by the same borrow.
pub struct Slot<
    'a,
    K: crate::depot::raw::Key,
    V,
    const N: usize,
    B = direct::Occupied<'a, K, lanes::Node<V, N>>,
> {
    slot: B,
    queues: &'a [links::Chain; N],
    value: marker::PhantomData<fn() -> (K, V)>,
}

/// A removed value's exact slot and original FIFO position.
#[must_use = "an unrestored withdrawal leaves its value removed"]
pub struct Withdrawn<'a, K: crate::depot::raw::Key, V, const N: usize, S: lanes::raw::State<V> = ()>
{
    slot: direct::Vacant<'a, K, lanes::Node<V, N, S>>,
    queues: &'a [links::Chain; N],
    lane: Option<Index<N>>,
    hook: links::Hook,
}

impl<'a, K: crate::depot::raw::Key, V, const N: usize, B> Slot<'a, K, V, N, B> {
    pub(in crate::depot) fn new(slot: B, queues: &'a [links::Chain; N]) -> Self {
        Self {
            slot,
            queues,
            value: marker::PhantomData,
        }
    }
}

impl<'a, K: crate::depot::raw::Key, V, const N: usize, S: lanes::raw::State<V>>
    Slot<'a, K, V, N, direct::Occupied<'a, K, lanes::Node<V, N, S>>>
{
    pub fn get(&self) -> &V {
        &self.slot.get().value
    }

    pub fn get_mut(&mut self) -> &mut V {
        &mut self.slot.get_mut().value
    }

    pub fn into_mut(self) -> &'a mut V {
        &mut self.slot.into_mut().value
    }

    pub(in crate::depot) fn state(&self) -> S {
        self.slot.get().state
    }

    pub fn move_to_back(self, target: Index<N>) {
        let index = unsafe { links::Index::new_usize(self.slot.index()) };
        // SAFETY: the retained occupied key proves this exact initialized node.
        let node = unsafe { self.slot.table().value(index) };
        let queue = target.get(self.queues);
        match node.lane.get() {
            Some(source) => {
                if source == target && queue.last() == Some(index) {
                    return;
                }
                unsafe {
                    source.get(self.queues).move_back(
                        queue,
                        &lanes::Hooks(self.slot.table()),
                        index,
                    );
                }
            }
            None => unsafe { queue.link_back(&lanes::Hooks(self.slot.table()), index) },
        }
        node.lane.set(Some(target));
    }

    pub fn unlink(self) {
        let index = unsafe { links::Index::new_usize(self.slot.index()) };
        // SAFETY: the retained occupied key proves this exact initialized node.
        let node = unsafe { self.slot.table().value(index) };
        if let Some(source) = node.lane.get() {
            unsafe {
                source
                    .get(self.queues)
                    .unlink(&lanes::Hooks(self.slot.table()), index)
                    .idle();
            }
            node.lane.set(None);
        }
    }

    /// Removes the value while retaining its exact key and original position.
    pub(in crate::depot) fn take_owned(
        self,
    ) -> (Withdrawn<'a, K, V, N, S>, lanes::raw::Owned<V, S>) {
        let index = self.detach();
        // SAFETY: unlink retained the exact entry and the pre-resolved index.
        let (slot, node) = unsafe { self.slot.take_at(index) };
        let (lane, hook, value) = node.unpack();
        (
            Withdrawn {
                slot,
                queues: self.queues,
                lane,
                hook,
            },
            value,
        )
    }

    pub fn discard(self) {
        let index = self.detach();
        // SAFETY: unlink retained the exact occupied entry and resolved index.
        unsafe { self.slot.discard_at(index) };
    }

    pub fn key(&self) -> K {
        self.slot.key()
    }

    pub fn lane(&self) -> Option<Index<N>> {
        self.slot.get().lane.get()
    }

    fn detach(&self) -> links::Index {
        // SAFETY: this occupied key was admitted within its exact u32-sized table.
        let index = unsafe { links::Index::new_usize(self.slot.index()) };
        let lane = unsafe { self.slot.table().value(index) }.lane.get();
        if let Some(lane) = lane {
            // SAFETY: this occupied entry retains its exact reciprocal lane.
            unsafe {
                lane.get(self.queues)
                    .unlink(&lanes::Hooks(self.slot.table()), index);
            }
        }
        index
    }
}

impl<'a, K: crate::depot::raw::Key, V, const N: usize> Slot<'a, K, V, N> {
    pub fn remove(self) -> V {
        self.take_owned().1.into_value()
    }

    pub fn take(self) -> (Withdrawn<'a, K, V, N>, V) {
        let (slot, value) = self.take_owned();
        (slot, value.into_value())
    }
}

impl<'a, K: crate::depot::raw::Key, V, const N: usize, S: lanes::raw::State<V>>
    Withdrawn<'a, K, V, N, S>
{
    /// Restores the exact original lane and FIFO position without another lookup.
    pub(in crate::depot) fn restore_with(
        self,
        value: lanes::raw::Owned<V, S>,
    ) -> Occupied<'a, K, V, N, S> {
        // SAFETY: the retained vacancy belongs to this exact u32-sized table.
        let index = unsafe { links::Index::new_usize(self.slot.key().index()) };
        let slot = self
            .slot
            .occupy(lanes::Node::from_parts(self.lane, self.hook, value));
        if let Some(lane) = self.lane {
            // SAFETY: this vacancy retained the exclusive table borrow and the
            // original hook, so its two neighbors still bound the same gap.
            unsafe {
                crate::depot::raw::Detach::restore(
                    lane.get(self.queues),
                    &lanes::Hooks(slot.table()),
                    index,
                );
            }
        }
        Slot::new(slot, self.queues)
    }

    /// Inserts the successor unlinked, ready for its selected destination.
    pub(in crate::depot) fn insert_with(
        self,
        value: lanes::raw::Owned<V, S>,
    ) -> Occupied<'a, K, V, N, S> {
        Slot::new(self.slot, self.queues).insert_with(value)
    }
}

impl<'a, K: crate::depot::raw::Key, V, const N: usize> Withdrawn<'a, K, V, N> {
    pub fn restore(self, value: V) -> Slot<'a, K, V, N> {
        self.restore_with(lanes::raw::Owned::new(value))
    }

    pub fn insert(self, value: V) -> Slot<'a, K, V, N> {
        self.insert_with(lanes::raw::Owned::new(value))
    }
}

impl<'a, K: crate::depot::raw::Key, V, const N: usize, S: lanes::raw::State<V>>
    Slot<'a, K, V, N, direct::Vacant<'a, K, lanes::Node<V, N, S>>>
{
    pub(in crate::depot) fn insert_with(
        self,
        value: lanes::raw::Owned<V, S>,
    ) -> Occupied<'a, K, V, N, S> {
        // SAFETY: the retained vacancy lies inside its original u32-sized table.
        let index = unsafe { links::Index::new_usize(self.slot.key().index()) };
        let slot = self.slot.occupy(lanes::Node::new(index, value));
        Slot::new(slot, self.queues)
    }
}

impl<'a, K: crate::depot::raw::Key, V, const N: usize>
    Slot<'a, K, V, N, direct::Vacant<'a, K, lanes::Node<V, N>>>
{
    pub fn insert(self, value: V) -> Slot<'a, K, V, N> {
        self.insert_with(lanes::raw::Owned::new(value))
    }
}

pub(super) type Occupied<'a, K, V, const N: usize, S> =
    Slot<'a, K, V, N, direct::Occupied<'a, K, lanes::Node<V, N, S>>>;

/// A table vacancy retains the value reservation and all lane metadata.
pub(super) type Vacancy<'a, K, V, const N: usize, S> =
    Slot<'a, K, V, N, direct::Vacant<'a, K, lanes::Node<V, N, S>>>;
