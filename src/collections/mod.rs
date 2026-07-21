mod batch_set;
mod bitmap;
mod grow;
mod heap;
pub mod intrusive;
mod pin_cell_slab;
mod pin_slab;
mod queue;
mod slab;
mod table;

pub use batch_set::{BatchDrain, BatchSet};
pub use bitmap::CellBitmap;
pub use heap::{FixedHeap, IndexedMinHeap, IndexedMinHeapVacantEntry};
pub use pin_cell_slab::{PinCellSlab, PinCellSlabOccupiedEntry, PinCellSlabVacantEntry};
pub use pin_slab::{
    FixedPinSlab, FixedPinSlabOccupiedEntry, FixedPinSlabVacantEntry, PinSlab,
    PinSlabOccupiedEntry, PinSlabVacantEntry,
};
pub use queue::{CellQueue, FixedQueue, FixedQueueVacantEntry, SlotQueue, SlotQueueVacantEntry};
pub use slab::{CellSlab, Slab, SlabGeneration, SlabKey, SlabKeyParts, SlabVacantEntry};
pub use table::FixedHashTable;

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
