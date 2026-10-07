//! Owner-bound endpoint-tagged FIFO operations.

pub mod entry;
mod error;
mod hook;
pub mod lane;
pub mod raw;
pub use entry::{Linked, Position, Vacant};
pub use hook::Hook;
pub use lane::Endpoint;
pub use shape::Domain;
pub mod shape;

use std::num::NonZero;

pub use error::Error;

use crate::depot::{index::Initialized, queue::links};

pub(in crate::depot) fn clear<S: crate::depot::raw::LaneStore>(store: &mut S) {
    for lane in 0..store.heads().as_ref().len() {
        // SAFETY: this owner's disjoint endpoint domain represents every head below u32::MAX.
        let endpoint = unsafe { NonZero::new_unchecked(u32::MAX - lane as u32) };
        // SAFETY: the exclusive owner retains its head extent through payload destruction.
        while let Some(index) =
            raw::Head::first(unsafe { store.heads().as_ref().get_unchecked(lane) })
        {
            // SAFETY: the private head and exclusive owner prove this live initialized payload.
            let entry = unsafe {
                crate::depot::raw::Proof::<_, Initialized<links::Chain>>::new((&mut *store, index))
            };
            unsafe { Linked::new(entry).unlink_front(endpoint).retire() };
        }
    }
}
