//! FIFO values with an independent minimum-heap order.

use std::{num, ops::ControlFlow};

use crate::depot::{
    capacity,
    heap::{self, queue},
    queue::links,
};

const _: () = assert!(std::mem::size_of::<Min<(), ()>>() == 6 * std::mem::size_of::<usize>());

/// A bounded FIFO whose minimum priority can be removed independently.
pub struct Min<P: Ord, V> {
    pub(in crate::depot::heap::queue) store: heap::raw::Queue<P, V>,
    pub(in crate::depot::heap::queue) ready: links::Chain<()>,
    pub(in crate::depot::heap::queue) free: links::Free,
}

impl<P: Ord, V> Min<P, V> {
    pub fn try_new(capacity: capacity::Count) -> Result<Self, heap::Error> {
        Ok(Self {
            store: heap::raw::Queue::try_new(capacity)?,
            ready: links::Chain::new(),
            free: links::Free::seeded(capacity.get()),
        })
    }

    pub const fn capacity(&self) -> usize {
        self.store.capacity()
    }

    pub const fn len(&self) -> usize {
        self.store.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.store.is_empty()
    }

    pub fn front(&mut self) -> Option<Occupied<'_, P, V>> {
        let index = self.ready.first()?;
        Some(Occupied {
            state: self,
            index,
            position: None,
        })
    }

    /// Visits only the admitted heads after acquiring one exclusive context.
    pub fn try_sweep<C, E, B>(
        &mut self,
        acquire: impl FnOnce() -> Result<C, E>,
        mut visit: impl FnMut(C, Occupied<'_, P, V>) -> ControlFlow<B, C>,
    ) -> Result<Option<ControlFlow<B, C>>, E> {
        let Some(count) = num::NonZero::new(self.store.len()) else {
            return Ok(None);
        };
        Ok(Some((0..count.get()).try_fold(acquire()?, |context, _| {
            // SAFETY: the initial FIFO cardinality admits this visit, and each
            // earlier exclusive entry can remove only its own current head.
            let index = unsafe { self.ready.front_key() };
            visit(
                context,
                Occupied {
                    state: self,
                    index,
                    position: None,
                },
            )
        })))
    }

    pub fn min(&self) -> Option<&P> {
        Some(self.store.min()?.1)
    }

    pub fn min_entry(&mut self) -> Option<Occupied<'_, P, V>> {
        let index = self.store.min()?.0;
        Some(Occupied {
            state: self,
            index,
            // SAFETY: the nonempty store retains this index at its initialized root.
            position: Some(unsafe { links::Index::new_usize(0) }),
        })
    }

    pub fn vacant(&mut self) -> Option<Vacant<'_, P, V>> {
        let index = self.free.front()?;
        Some(Vacant { state: self, index })
    }
}

/// A linked value retained under an exclusive queue borrow.
#[must_use = "an occupied entry retains its exclusive owner borrow"]
pub struct Occupied<'a, P: Ord, V> {
    pub(in crate::depot::heap::queue) state: &'a mut queue::Min<P, V>,
    pub(in crate::depot::heap::queue) index: links::Index,
    pub(in crate::depot::heap::queue) position: Option<links::Index>,
}

impl<P: Ord, V> Occupied<'_, P, V> {
    pub fn priority(&self) -> &P {
        // SAFETY: the exclusive queue loan retains this live index and any known
        // heap position; payload access and FIFO relinking cannot change either.
        unsafe { self.state.store.priority(self.index, self.position) }
    }

    pub fn get_mut(&mut self) -> &mut V {
        // SAFETY: this entry retains the exact occupied index and the exclusive queue borrow.
        unsafe { self.state.store.value_mut(self.index) }
    }

    pub fn move_to_back(self) {
        if self.state.ready.last() == Some(self.index) {
            return;
        }
        // SAFETY: this entry retains the exact live index in the ready chain and
        // owns the exclusive queue borrow, so the relink preserves the topology.
        unsafe {
            self.state
                .ready
                .move_back(&self.state.ready, &self.state.store, self.index)
        };
    }

    pub fn remove(self) -> (P, V) {
        // SAFETY: this entry retains the exact live index and any known heap
        // position under the same exclusive queue loan until removal begins.
        let output = unsafe { self.state.store.remove(self.index, self.position) };
        // SAFETY: the same index is still linked, so unlinking returns its exact vacancy hook.
        let vacant = unsafe { self.state.ready.unlink(&self.state.store, self.index) };
        self.state.free.recycle(vacant);
        output
    }
}

/// The exact vacant slot reserved by an exclusive queue borrow.
#[must_use = "a vacant queue slot has not inserted a value"]
pub struct Vacant<'a, P: Ord, V> {
    pub(in crate::depot::heap::queue) state: &'a mut queue::Min<P, V>,
    pub(in crate::depot::heap::queue) index: links::Index,
}

impl<P: Ord, V> Vacant<'_, P, V> {
    pub fn push_back(self, priority: P, value: V) {
        let queue::Min { store, ready, free } = self.state;
        // SAFETY: this admitted free head is absent from the heap. Ordering
        // completes before its resident and FIFO membership are published below.
        unsafe { store.insert(self.index, priority) };
        // SAFETY: this capability retains the exact free head and the exclusive state loan.
        unsafe { free.acquire_at(store, self.index) };
        // SAFETY: the acquired slot is vacant with uninitialized value storage.
        unsafe { store.write(self.index, value) };
        // SAFETY: the same slot now holds a live value and its FIFO hook is detached.
        unsafe { ready.link_back(store, self.index) };
    }
}

const _: () =
    assert!(std::mem::size_of::<Vacant<'static, u64, u64>>() == 2 * std::mem::size_of::<usize>());
