//! Generation-checked FIFO topology over caller-owned keyed slots.

use crate::depot::{queue::links, raw};

mod fifo;
pub mod lanes;
mod link;

pub use fifo::Fifo;
pub use link::Link;

mod slot;
pub use slot::Slot;

mod iter;
pub use iter::Iter;

fn selected<O: raw::KeyOwner<P>, P>(owner: &O) -> (links::Index, &Link<O::Key, P>) {
    let (key, link) = owner.parts();
    // SAFETY: this owner loan retains the exact key/link pair and an index below u32::MAX.
    let index = unsafe { links::Index::new_usize(raw::Address::index(key)) };
    (index, link)
}

/// The retained owner holds its selected-slot authority, exact key/link pair, FIFO and plane.
/// The link is initialized; LINKED means FIFO membership, otherwise canonical vacancy.
/// The phase holds until its authorized transition; borrowed outputs retain their prescribed root authority.
pub struct Membership<const LINKED: bool>;

impl<const LINKED: bool> crate::depot::raw::Sealed for Membership<LINKED> {}

/// The retained source owns a nonempty topology in its exact initialized storage and plane.
/// Its endpoint remains a member until the consuming projection returns its proof.
pub struct NonEmpty;

impl crate::depot::raw::Sealed for NonEmpty {}

/// A mutable owner projection preserves the retained slot's exact identity, root,
/// key, link, FIFO, plane and membership throughout every resulting borrow.
pub struct Exclusive;

impl crate::depot::raw::Sealed for Exclusive {}
