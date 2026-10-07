//! Mutable byte windows grouped by backing and access pattern.
//! Mechanically checked storage operations for byte windows.

pub(in crate::buffer) use capacity::Capacity;
pub(in crate::buffer) use growth::Growth;
pub(in crate::buffer) use plans::{Edit, Fresh};
pub use snapshot::Snapshot;
pub(in crate::buffer) use state::State;

pub mod reserve;

mod capacity;
mod growth;
mod plans;
mod snapshot;
mod state;
