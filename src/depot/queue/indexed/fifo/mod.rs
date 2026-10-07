use std::{marker, mem, num::NonZero};

pub use entry::Entry;
pub(in crate::depot) use nodes::{NodeEntry, Nodes};

use crate::depot::{
    capacity,
    queue::{indexed::fifo, links},
};

mod entry;
mod nodes;
mod store;
pub(in crate::depot::queue::indexed) use store::Store;
pub mod staged;

/// A fixed-capacity FIFO whose physical index is supplied by the caller.
pub struct List<T = (), I: crate::depot::raw::Compact = u32, P = ()> {
    pub(in crate::depot::queue::indexed) store: fifo::Store<T>,
    index: marker::PhantomData<fn() -> (I, P)>,
}

#[derive(Clone, Copy)]
struct Counted<I>(capacity::Count, marker::PhantomData<fn(I) -> I>);

impl<I: crate::depot::raw::Compact> Counted<I> {
    fn try_new(capacity: usize) -> Result<Self, crate::alloc::Error> {
        let count =
            capacity::Count::try_index(capacity, I::MAX as usize, crate::alloc::Error::Overflow)?;
        Ok(Self(count, marker::PhantomData))
    }
}

// SAFETY: the private constructor admits only positions representable by I.
unsafe impl<I: crate::depot::raw::Compact> crate::depot::raw::Count<I> for Counted<I> {
    fn count(self) -> capacity::Count {
        self.0
    }
}

impl<T> List<T> {
    /// Allocates a nonempty domain and queues its first physical slot.
    pub fn try_with(count: NonZero<u32>, first: T) -> Result<Self, crate::alloc::Error> {
        let queue = Self::try_count(count)?;
        let index = links::Index::from(NonZero::<u32>::MIN);
        // SAFETY: the retained nonzero count admits this initialized vacant slot.
        let entry = unsafe { queue.store.nodes.at(index) };
        Slot::<_, fifo::Vacant> {
            entry,
            phase: marker::PhantomData,
        }
        .push_back(first);
        Ok(queue)
    }
}

impl<T, I: crate::depot::raw::Compact, P> List<T, I, P> {
    pub fn try_new(capacity: usize) -> Result<Self, crate::alloc::Error> {
        Self::try_count(Counted::try_new(capacity)?)
    }

    pub fn try_count<C: crate::depot::raw::Count<I>>(
        count: C,
    ) -> Result<Self, crate::alloc::Error> {
        Ok(Self {
            store: fifo::Store::try_count(count.count())?,
            index: marker::PhantomData,
        })
    }

    pub fn pop_front(&mut self) -> Option<T> {
        Some(self.store.front_entry()?.take().1)
    }

    pub fn len(&self) -> usize {
        self.store.len()
    }

    pub fn capacity(&self) -> usize {
        self.store.capacity()
    }

    pub fn is_empty(&self) -> bool {
        self.store.nodes.state().is_empty()
    }
}

impl<T, I: crate::depot::raw::Compact> List<T, I> {
    pub fn entry(&mut self, index: I) -> Option<Entry<'_, T, I>> {
        let entry = self.store.nodes.get(index.into_raw() as usize)?;
        Some(if entry.slot().hook.previous.get() == entry.index().raw() {
            Entry::Vacant(Slot {
                entry,
                phase: marker::PhantomData,
            })
        } else {
            Entry::Occupied(Slot {
                entry,
                phase: marker::PhantomData,
            })
        })
    }

    pub fn vacant_entry(&mut self, index: I) -> Option<fifo::Slot<'_, T, fifo::Vacant, I>> {
        match self.entry(index)? {
            Entry::Vacant(entry) => Some(entry),
            Entry::Occupied(_) => None,
        }
    }
}

const _: () = {
    assert!(mem::size_of::<List<u64>>() == 3 * mem::size_of::<usize>());
    assert!(
        mem::size_of::<fifo::Slot<'static, u64, fifo::Vacant>>() == 2 * mem::size_of::<usize>()
    );
    assert!(
        mem::size_of::<fifo::Slot<'static, u64, fifo::Occupied>>() == 2 * mem::size_of::<usize>()
    );
};

/// One exclusive FIFO position with its occupied or vacant phase retained.
#[must_use]
pub struct Slot<'a, T, P, I: crate::depot::raw::Compact = u32> {
    entry: NodeEntry<'a, T, links::Chain<u32>>,
    phase: Domain<'a, T, P, I>,
}

impl<'a, T, I: crate::depot::raw::Compact> Slot<'a, T, fifo::Vacant, I> {
    pub fn push_back(self, value: T) {
        let store = self.entry.store();
        unsafe { store.state().link_back(store, self.entry.index()) };
        unsafe { *self.entry.slot().value.get() = mem::MaybeUninit::new(value) };
    }
}

impl<'a, T, I: crate::depot::raw::Compact> Slot<'a, T, fifo::Occupied, I> {
    pub fn get(&self) -> &T {
        let node = self.entry.slot();
        unsafe { (*node.value.get()).assume_init_ref() }
    }

    pub fn remove(self) -> (Slot<'a, T, fifo::Vacant, I>, T) {
        let store = self.entry.store();
        unsafe { store.state().unlink(store, self.entry.index()).idle() };
        let value = unsafe { (*self.entry.slot().value.get()).assume_init_read() };
        (
            Slot {
                entry: self.entry,
                phase: marker::PhantomData,
            },
            value,
        )
    }

    pub fn clear(self) {
        let store = self.entry.store();
        unsafe { store.state().unlink(store, self.entry.index()).idle() };
        unsafe { (*self.entry.slot().value.get()).assume_init_drop() };
    }
}

pub enum Vacant {}
pub enum Occupied {}

type Domain<'a, T, P, I> = marker::PhantomData<(&'a mut T, fn(P, I) -> (P, I))>;
