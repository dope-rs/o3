pub mod raw;
mod sealed;

use std::pin;

pub use sealed::{Lease, Pool, Reservation};

/// Resets a pinned value in place before its slot is recycled.
pub trait Recycle {
    fn recycle(self: pin::Pin<&mut Self>);
}
