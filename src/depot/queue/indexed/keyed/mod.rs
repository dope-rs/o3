//! Generation-checked FIFO topology over caller-owned keyed slots.

mod fifo;
pub mod lanes;
mod link;

pub use fifo::Fifo;
pub use link::Link;

mod slot;
pub use slot::Slot;

mod entry;
mod iter;
pub use entry::Entry;
pub use iter::Iter;

/// The retained owner holds its exact initialized slot, key, link, FIFO and plane.
/// LINKED means membership in that FIFO; otherwise its hook is canonically vacant.
/// The phase holds until its authorized transition; borrowed outputs retain their prescribed root authority.
pub struct Membership<const LINKED: bool>;

impl<const LINKED: bool> crate::depot::raw::Sealed for Membership<LINKED> {}
impl<const LINKED: bool> crate::depot::Predicate for Membership<LINKED> {}

/// The retained source owns a nonempty topology in its exact initialized storage and plane.
/// Its endpoint remains a member until the consuming projection returns its proof.
pub struct NonEmpty;

impl crate::depot::raw::Sealed for NonEmpty {}
impl crate::depot::Predicate for NonEmpty {}

/// A mutable owner projection preserves the retained slot's exact identity, root,
/// key, link, FIFO, plane and membership throughout every resulting borrow.
pub struct Exclusive;

impl crate::depot::raw::Sealed for Exclusive {}
impl crate::depot::Predicate for Exclusive {}
