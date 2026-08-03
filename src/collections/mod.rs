mod arena;
mod array;
mod batch;
mod heap;
mod indexed;
pub mod intrusive;
mod queue;
mod slab;
mod table;

use std::mem;
use std::ops::{Deref, DerefMut};

pub use arena::{LinkedArena, StackArena, StackDrain};
pub use array::{ArrayVec, ArrayVecIntoIter, CopyArrayVec};
pub use batch::{BatchDrain, BatchSet};
pub use heap::{FixedHeap, IndexedMinHeap, IndexedMinHeapVacantEntry};
pub use indexed::FixedIndexTable;
pub use queue::cell::CellQueue;
pub use queue::fixed::{FixedQueue, FixedQueueVacantEntry};
pub use queue::round::RoundRobinSet;
pub use queue::slot::{CellSlotQueue, SlotQueue, SlotQueueVacantEntry};
pub use slab::cell::CellSlab;
pub use slab::key::{SlabGeneration, SlabKey, SlabKeyParts};
pub use slab::lease::{LeaseSlab, LeaseSlabError, LeaseSlabVacantEntry, SlabLease};
pub use slab::pin::fixed::{FixedPinSlab, FixedPinSlabVacantEntry};
pub use slab::pin::{PinSlab, PinSlabVacantEntry};
pub use slab::{Slab, SlabVacantEntry};
pub use table::{FixedHashTable, FixedHashTablePlan};

#[doc(hidden)]
pub mod __private {
    pub use super::batch::{BatchMap, BatchMapDrain};
}

pub(crate) mod index {
    pub trait Sealed {}
}

#[doc(hidden)]
pub trait IndexKey: index::Sealed + Copy + Eq {
    fn index(self) -> usize;
}

impl IndexKey for usize {
    fn index(self) -> usize {
        self
    }
}

impl index::Sealed for usize {}

pub(super) struct BoxSliceGrowth<'a, T> {
    target: &'a mut Box<[T]>,
    values: Vec<T>,
}

impl<'a, T> BoxSliceGrowth<'a, T> {
    pub(super) fn take(target: &'a mut Box<[T]>) -> Self {
        let values = mem::take(target).into_vec();
        Self { target, values }
    }
}

impl<T> Deref for BoxSliceGrowth<'_, T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Self::Target {
        &self.values
    }
}

impl<T> DerefMut for BoxSliceGrowth<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.values
    }
}

impl<T> Drop for BoxSliceGrowth<'_, T> {
    fn drop(&mut self) {
        *self.target = mem::take(&mut self.values).into_boxed_slice();
    }
}

pub(crate) struct ClearGuard<'a, T: ?Sized> {
    value: &'a mut T,
    clear: fn(&mut T),
    armed: bool,
}

impl<'a, T: ?Sized> ClearGuard<'a, T> {
    pub(crate) fn run(value: &'a mut T, clear: fn(&mut T)) {
        let mut guard = Self {
            value,
            clear,
            armed: true,
        };
        (guard.clear)(guard.value);
        guard.armed = false;
    }
}

impl<T: ?Sized> Drop for ClearGuard<'_, T> {
    fn drop(&mut self) {
        if self.armed {
            (self.clear)(self.value);
        }
    }
}
