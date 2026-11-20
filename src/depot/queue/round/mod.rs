//! Fixed-domain insertion-ordered round-robin membership.

pub use lanes::{Lanes, group::Group, lane::Lane};
pub use location::{Location, idle::Idle, linked::Linked};
pub use slot::Slot;
pub use snapshot::{Snapshot, front::Front, place::Place, turn::Turn};

use crate::depot::{arena::slab, queue::links};

mod join;
pub mod phase;
pub mod set;
pub use join::Join;

/// The first admission failed against this allocation's exact domain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Bounds {
    Slot { index: usize, capacity: usize },
    Lane { index: usize, capacity: usize },
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub(in crate::depot::queue::round) struct Position {
    pub lane: links::Index,
    pub index: links::Index,
}

mod lanes;
mod location;
mod slot;
mod snapshot;
pub mod values;

/// Generation-addressed residents and their insertion-ordered fair schedule.
pub struct Set<'h, T, Tag = (), const MAX: u32 = { u32::MAX }> {
    values: slab::Store<T, Tag, MAX>,
    order: set::Order<'h, Tag>,
}
