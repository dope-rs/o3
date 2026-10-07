//! Exclusive FIFO lanes over one caller-owned exact-key link plane.

mod fifo;

pub use fifo::Fifo;

mod bound;
pub use bound::Bound;

mod access;
pub use access::Access;
