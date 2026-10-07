//! Copy payloads whose initialization and lane membership have one owner.

use std::{cell::Cell, num};

use crate::{alloc, base::num::bounded, depot::queue::round};

mod group;
mod head;
mod lane;
mod slot;
mod snapshot;

pub use group::Group;
pub use head::Head;
pub use lane::Lane;
pub use slot::Slot;
pub use snapshot::Snapshot;

/// Closed ownership of resident payloads and their insertion-ordered fair lanes.
pub struct Values<T: Copy, const G: usize, const N: usize = 0>(round::Lanes<Cell<Option<T>>, G, N>);

impl<T: Copy, const G: usize> Values<T, G> {
    pub fn try_new(capacity: usize, lanes: num::NonZero<usize>) -> Result<Self, alloc::Error> {
        Ok(Self(round::Lanes::try_new(capacity, lanes)?))
    }
}

impl<T: Copy, const G: usize, const N: usize> Values<T, G, N> {
    pub fn try_fixed(capacity: usize) -> Result<Self, alloc::Error> {
        Ok(Self(round::Lanes::try_fixed(capacity)?))
    }

    pub fn try_slot(&self, index: usize) -> Result<Slot<'_, T, G, N>, round::Bounds> {
        self.0.try_slot(index).map(Slot)
    }

    pub fn try_lane(&self, index: usize) -> Result<Lane<'_, T, G, N>, round::Bounds> {
        round::Lane::try_new(&self.0, index).map(Lane)
    }

    pub fn lane<const MAX: u32>(&self, index: bounded::U32<0, MAX>) -> Lane<'_, T, G, N> {
        Lane(self.0.lane(index))
    }

    pub fn group<const I: usize>(&self) -> Group<'_, T, G, N> {
        Group(self.0.group::<I>())
    }

    pub fn has_pending(&self) -> bool {
        self.0.groups.iter().any(|ring| !ring.is_empty())
    }
}
