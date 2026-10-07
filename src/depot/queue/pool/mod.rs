//! Fixed nodes shared by borrowing FIFOs and persistent lanes.

use std::mem;

pub use lane::{Lane, front::Front, vacancy::Vacancy, view::View};
pub use lanes::Lanes;
pub use missing::Missing;

use crate::depot::capacity;

pub mod fair;

/// The retained lane owns its current initialized head until projection.
/// A unit selector reads that head from the lane; a slot selector retains its exact identity.
/// Head removal, replacement, or reclamation consumes this authority.
pub enum Head {}

pub mod fifo;
mod lane;
mod lanes;
mod missing;

/// Validated dimensions for one persistent lane pool.
#[derive(Clone, Copy, Eq, PartialEq)]
#[repr(C)]
pub struct Plan {
    slots: u32,
    lanes: u32,
}

/// A dimension outside the lane-pool index domain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Slots { requested: usize },
    Lanes { requested: usize },
}

impl Plan {
    pub fn new(slots: usize, lanes: usize) -> Result<Self, Error> {
        let slots = capacity::Count::try_from(slots).map_err(|error| Error::Slots {
            requested: error.requested(),
        })?;
        let lanes = capacity::Count::try_from(lanes).map_err(|error| Error::Lanes {
            requested: error.requested(),
        })?;
        Ok(Self::from_counts(slots, lanes))
    }

    pub const fn from_counts(slots: capacity::Count, lanes: capacity::Count) -> Self {
        Self {
            slots: slots.raw(),
            lanes: lanes.raw(),
        }
    }

    pub const fn slots(self) -> usize {
        self.slots as usize
    }

    pub const fn lanes(self) -> usize {
        self.lanes as usize
    }
}

const _: () = {
    assert!(mem::size_of::<Plan>() == mem::size_of::<u64>());
    assert!(mem::size_of::<Error>() == 2 * mem::size_of::<usize>());
};
