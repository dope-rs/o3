//! Direct-address minimum heaps.

pub use occupied::Occupied;
mod occupied;
pub use vacant::Vacant;
mod vacant;

use std::mem;

pub use entry::Entry;
pub use error::Error;
use heap::indexed;

use crate::{
    alloc,
    depot::{capacity, heap},
};

mod entry;
mod error;
pub mod resident;
mod slot;
pub use slot::Slot;

/// The retained source proves this exact heap slot is vacant.
pub struct Empty;

/// The retained source proves this exact heap slot is occupied.
pub struct Full;

/// A fixed-domain minimum heap addressed by integer slots.
#[repr(C)]
pub struct Min<P: Ord, C: alloc::raw::Extent = capacity::Count> {
    pub(in crate::depot::heap) storage:
        alloc::raw::Block<heap::Capacity<heap::Stored<heap::Direct, P>, u32, (), C>, u32>,
}

impl<P: Ord, C: alloc::raw::Extent> Min<P, C> {
    pub fn try_new(len: C) -> Result<Self, heap::Error<C>> {
        Ok(Self {
            storage: heap::Capacity::try_new(len)?.allocate()?,
        })
    }

    /// Creates a full heap whose domain shares one priority.
    pub fn try_repeat(len: C, priority: P) -> Result<Self, heap::Error<C>>
    where
        P: Copy,
    {
        let extent = heap::Capacity::try_new(len)?;
        let storage = alloc::raw::Block::try_fill(extent, len.len() as u32, |index| {
            (
                mem::MaybeUninit::new(heap::Stored {
                    identity: unsafe { heap::Direct::from_index(index) },
                    priority,
                }),
                index as u32,
            )
        })
        .map_err(|error| heap::Error::Allocation {
            capacity: len,
            layout: alloc::raw::Shape::layout(&extent),
            error,
        })?;
        Ok(Self { storage })
    }

    pub fn try_entry(
        &mut self,
        index: usize,
    ) -> Result<indexed::Entry<'_, P, C>, indexed::Error<C>> {
        let view = &self.storage;
        if index >= view.tail().len() {
            return Err(indexed::Error {
                index,
                capacity: view.extent().count(),
            });
        }
        // SAFETY: this exact heap admits the index and its initialized reverse map.
        let live =
            unsafe { crate::depot::raw::Proof::new(((self, ()), heap::Direct::from_index(index))) };
        Ok(indexed::Entry::from_slot(heap::Slot::from_source(live).0))
    }

    pub fn get(&self, index: usize) -> Option<&P> {
        let entry = heap::raw::Store::current(self, index)?;
        Some(&entry.priority)
    }

    pub fn peek(&self) -> Option<(usize, &P)> {
        let (key, priority) = heap::raw::Store::peek(self)?;
        Some((key.get(), priority))
    }

    pub fn first_entry(&mut self) -> Option<indexed::Occupied<'_, P, C>> {
        Some(indexed::Occupied {
            entry: heap::Entry::first(self)?,
        })
    }

    pub fn clear(&mut self) {
        heap::raw::Store::clear(self);
    }

    pub(in crate::depot) const fn capacity(&self) -> C {
        self.storage.extent().count()
    }

    pub const fn len(&self) -> usize {
        self.storage.state as usize
    }

    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<P: Ord, C: alloc::raw::Extent> Drop for Min<P, C> {
    fn drop(&mut self) {
        // SAFETY: Drop retains the terminal owner until its MaybeUninit backing is deallocated.
        unsafe { heap::raw::Store::destroy(self) };
    }
}

const _: () = assert!(mem::size_of::<Min<usize>>() == 3 * mem::size_of::<usize>());

unsafe impl<P: Ord, C: alloc::raw::Extent> heap::raw::Store for Min<P, C> {
    type Key = heap::Direct;
    type Priority = P;
    type Position = u32;
    type Storage = heap::Capacity<heap::Stored<heap::Direct, P>, u32, (), C>;
    type Extra = ();

    fn precedes(_: &Self::Key, _: &Self::Key) -> bool {
        false
    }

    fn storage(&self) -> &alloc::raw::Block<Self::Storage, u32, Self::Extra> {
        &self.storage
    }

    fn storage_mut(&mut self) -> &mut alloc::raw::Block<Self::Storage, u32, Self::Extra> {
        &mut self.storage
    }
}
