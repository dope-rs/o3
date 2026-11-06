//! Fixed-capacity indexed FIFO lanes.

pub use fifo::{Fifo, position::Position, value::Value};
pub use table::{Table, node::Node};

pub mod entry;
pub mod runtime;
pub mod typed;

mod fifo;
mod raw;
mod table;

/// Abandoned initialized insertions are unlinked and their value is dropped.
pub type Rollback<'a, T, const N: usize, const L: usize, C> = crate::mem::Lease<
    crate::depot::raw::LaneUndo<Position<'a, T, N, L, C, true>>,
    crate::mem::lease::Open,
>;
