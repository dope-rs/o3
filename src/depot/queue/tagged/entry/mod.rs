use crate::depot::{capacity, queue::links};

/// The original input and state that rejected a node operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error<C = capacity::Count> {
    Index {
        index: usize,
        capacity: C,
    },

    Linked {
        index: links::Index,
        previous: u32,
        next: u32,
    },
}

const _: () = assert!(size_of::<Error<std::num::NonZero<u32>>>() <= size_of::<Error>());

mod linked;
mod position;
mod vacant;
pub use linked::Linked;
pub use position::Position;
pub use vacant::Vacant;
