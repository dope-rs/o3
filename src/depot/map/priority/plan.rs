use std::marker;

use depot::{capacity, map};
use map::priority;

use crate::depot;

/// A capacity validated against the exact priority-map layout.
pub struct Plan<K, P, V> {
    pub(in crate::depot::map::priority) table: map::raw::Shape<u32>,
    pub(in crate::depot::map::priority) shape: Fields<K, P, V>,
}

impl<K, P: Ord, V> Plan<K, P, V> {
    pub fn new(capacity: usize) -> Result<Self, map::Capacity> {
        let plan = map::Plan::new(capacity).map_err(map::Capacity::Domain)?;
        match priority::Layout::try_plan(plan) {
            Ok(layout) => Ok(layout.plan),
            Err(cause) => Err(map::Capacity::Layout {
                requested: capacity,
                cause,
            }),
        }
    }
}

impl<K, P, V> Plan<K, P, V> {
    /// Returns the capacity admitted by this plan's exact layout.
    pub const fn capacity(self) -> capacity::Count {
        self.table.capacity()
    }
}

impl<K, P, V> Clone for Plan<K, P, V> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K, P, V> Copy for Plan<K, P, V> {}

type Fields<K, P, V> = marker::PhantomData<fn() -> (K, P, V)>;
