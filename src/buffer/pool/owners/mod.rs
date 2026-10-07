use std::{marker, num::NonZero, ptr::NonNull};

use account::Accounts;
pub(in crate::buffer) use account::{Account, Registered};
pub use edge::Edge;
pub(in crate::buffer) use edge::{Input, Measured};
use free::Free;
pub(in crate::buffer::pool) use header::Header;
use header::Shape;
pub(in crate::buffer::pool) use key::Key;
pub(in crate::buffer) use ptr::Ptr;
pub(in crate::buffer::pool) use ptr::{Accounted, Any, Block};
use ptr::{Base, Binding};
use slot::Retired;
pub(in crate::buffer) use slot::Slot;
pub(in crate::buffer::pool) use slot::{Active, Claim, Entry, Seed};

/// Capacity of the published pool allocation containing this live slot prefix.
///
/// # Safety
/// `owner` must be the prefix address of one active slot in a published pool.
pub(in crate::buffer) unsafe fn capacity_of(owner: NonNull<()>) -> NonZero<u32> {
    // SAFETY: the caller proves this is one active pool-slot prefix, and the
    // reconstruction reads only the immutable header geometry it retains.
    unsafe { Ptr::<Any, Any>::from_owner(owner) }
        .slots
        .capacity()
}

mod account;
mod edge;
mod free;
mod header;
mod key;

mod ptr;
mod slot;

#[derive(Debug)]
pub struct Plain<L = super::Layout>(marker::PhantomData<fn() -> L>);

#[derive(Debug)]
pub struct Resident<'d> {
    brand: marker::PhantomData<fn(&'d ()) -> &'d ()>,
}
