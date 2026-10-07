use std::cell::Cell;

use crate::{
    base::num::bounded,
    depot::queue::round::{self, values::Snapshot},
};

/// One resident admitted by the owner that alone controls payload initialization.
pub struct Slot<'a, T: Copy, const G: usize, const N: usize = 0>(
    pub(super) round::Slot<'a, Cell<Option<T>>, G, N>,
);

impl<'a, T: Copy, const G: usize, const N: usize> Slot<'a, T, G, N> {
    fn inner(&self) -> round::Slot<'a, Cell<Option<T>>, G, N> {
        round::Slot {
            owner: self.0.owner,
            index: self.0.index,
        }
    }

    pub fn get(&self) -> Option<T> {
        self.0.get().get()
    }

    /// Replaces this payload and leaves it outside every queue.
    pub fn write(&self, value: T) {
        self.inner().clear().set(Some(value));
    }

    /// Removes membership before transferring this payload's initialization.
    pub fn take(&self) -> Option<T> {
        self.inner().clear().take()
    }

    pub fn try_lane(
        &self,
        index: usize,
    ) -> Result<Snapshot<'a, T, G, round::Place, N>, round::Bounds> {
        self.0.try_lane(index).map(Snapshot)
    }

    pub fn lane<const MAX: u32>(
        &self,
        index: bounded::U32<0, MAX>,
    ) -> Snapshot<'a, T, G, round::Place, N> {
        Snapshot(self.0.lane(index))
    }
}
