use std::mem;

use pool::Missing;

use crate::depot::{
    capacity,
    queue::{links, pool},
};

/// Owns a fixed index domain of persistent FIFO lanes.
pub struct Lanes<T, S = (), const TRACK: bool = true> {
    pub(in crate::depot) block: crate::depot::raw::QueueBlock<T, S>,
}

impl<T, S, const TRACK: bool> Lanes<T, S, TRACK> {
    pub fn try_new(plan: pool::Plan) -> Result<Self, crate::alloc::Error>
    where
        S: Default,
    {
        Self::try_from_fn(plan, |_| S::default())
    }

    /// Initializes each persistent lane's state from its exact index.
    pub fn try_from_fn(
        plan: pool::Plan,
        init: impl FnMut(links::Index) -> S,
    ) -> Result<Self, crate::alloc::Error> {
        Ok(Self {
            block: crate::depot::raw::QueueBlock::try_with(
                capacity::Count::new(plan.slots),
                capacity::Count::new(plan.lanes),
                init,
            )?,
        })
    }

    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub const fn len(&self) -> usize {
        self.block.count().get()
    }

    pub fn get(&self, index: usize) -> Option<pool::View<'_, T, S, TRACK>> {
        let slot = self.block.slots().get(index)?;
        Some(pool::View::new(&self.block.store, slot))
    }

    pub fn try_bind(&self, index: usize) -> Result<pool::View<'_, T, S, TRACK>, Missing> {
        self.get(index).ok_or(Missing {
            index,
            lanes: self.len(),
        })
    }

    pub fn get_mut(&mut self, index: usize) -> Option<pool::Lane<'_, T, S, TRACK>> {
        let (store, slots) = self.block.split();
        Some(pool::Lane::new(store, slots.get_mut(index)?))
    }

    /// Borrows every initialized lane from this exact owner once.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = pool::Lane<'_, T, S, TRACK>> {
        let (store, slots) = self.block.split();
        slots
            .iter_mut()
            .map(move |slot| pool::Lane::new(store, slot))
    }

    pub fn try_bind_mut(&mut self, index: usize) -> Result<pool::Lane<'_, T, S, TRACK>, Missing> {
        let lanes = self.len();
        self.get_mut(index).ok_or(Missing { index, lanes })
    }
}

impl<T, S> Lanes<T, S> {
    pub fn available(&self) -> usize {
        self.block.store.available()
    }
}

const _: () = {
    assert!(mem::size_of::<Lanes<u64>>() == 5 * mem::size_of::<usize>());
};
