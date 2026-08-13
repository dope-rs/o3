mod sealed;

use std::{pin, ptr};

pub use sealed::{Lease, Pool, Reservation};

/// Resets a pinned value before its slot returns to a recyclable pool.
///
/// Unlike [`crate::collections::slab::recycle::Recycle`], this operation never
/// moves the value or replaces it with a seed. One initialized value remains in
/// each slot for the complete lifetime of its pool.
pub trait Recycle {
    fn recycle(self: pin::Pin<&mut Self>);
}

/// External lifetime proof for reservations detached from a [`Pool`] borrow.
///
/// # Safety
///
/// The returned pool must be valid and accessible from the current thread for
/// the call which consumes this source. Its pinned backing group must remain
/// alive until every [`Reservation`] and [`Lease`] issued with `'owner` has
/// been dropped. Calls using the same pool must not race on different threads.
pub unsafe trait PoolOwner<'owner, T: Recycle> {
    fn pool(self) -> ptr::NonNull<Pool<T>>;
}
