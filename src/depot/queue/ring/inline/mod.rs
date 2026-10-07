//! Inline slots partitioned between available, leased, and queued ownership.

pub use full::Full;
pub use lease::Lease;
pub use pool::Pool;

mod full;

mod index;
mod lease;
mod pool;
mod slot;
