//! Fixed-capacity indexed FIFO lanes.

use crate::depot::{self, index::Initialized, queue::links};

pub use fifo::{Fifo, position::Position, value::Value};
pub use table::{Table, node::Node};

pub mod entry;
pub mod runtime;
pub mod typed;

mod fifo;
mod raw;
mod table;

/// Abandoned initialized insertions are unlinked and their value is dropped.
pub type Rollback<'a, T, const N: usize, C> = crate::mem::Lease<
    depot::raw::LaneUndo<
        depot::raw::Proof<(&'a mut Fifo<T, N, C>, links::Index), Initialized<links::Chain>>,
    >,
    crate::mem::lease::Open,
>;
