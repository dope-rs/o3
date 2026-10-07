//! Indexed FIFO lanes whose count is chosen at runtime.

pub use fifo::Fifo;
pub use group::Group;

pub mod shared;

mod capacity;
mod fifo;
mod group;
