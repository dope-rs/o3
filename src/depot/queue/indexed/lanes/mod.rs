//! Fixed-capacity indexed FIFO lanes.

pub use fifo::{Fifo, position::Position, value::Value};
pub(in crate::depot) use table::Hooks;
pub use table::{Table, node::Node};

pub mod entry;
pub mod runtime;
pub mod typed;

mod fifo;
mod raw;
mod table;

/// Abandoned initialized insertions are unlinked and their value is dropped.
pub struct Rollback;
