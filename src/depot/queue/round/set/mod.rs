//! Exclusive resident capabilities for a fair set.

pub use head::Head;
pub use idle::Idle;
pub use occupied::Occupied;
pub(in crate::depot::queue::round) use order::Order;
pub use ready::Ready;
pub use reuse::Reuse;
pub use vacant::Vacant;

/// One independently borrowed physical resident.
pub enum Entry<'a, T, Tag = (), const MAX: u32 = { u32::MAX }, B = ()> {
    Vacant(Vacant<'a, T, Tag, MAX, false, B>),
    Occupied(Occupied<'a, T, Tag, MAX, false, B>),
}

/// The resident's membership, retained until its consuming transition.
pub enum State<'a, T, Tag = (), const MAX: u32 = { u32::MAX }, const ROOT: bool = true, B = ()> {
    Idle(Occupied<'a, T, Tag, MAX, ROOT, B, Idle>),
    Ready(Occupied<'a, T, Tag, MAX, ROOT, B, Ready>),
}

mod head;
mod idle;
mod occupied;
mod order;
mod ready;
mod reuse;
mod vacant;
