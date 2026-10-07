use std::{convert, mem};

use super::{entries::Mode, key::cycle::Policy};
use crate::depot::capacity;

/// The exact physical slot observation that rejected a lookup.
#[derive(Debug, PartialEq, Eq)]
pub enum Error<B = convert::Infallible, R = ()> {
    Index {
        index: u32,
        capacity: capacity::Count,
    },
    Free {
        index: u32,
    },
    Reserved {
        index: u32,
    },
    Retired {
        index: u32,
        retired: R,
    },
    Busy {
        index: u32,
        borrow: B,
    },
}

/// Lookup outcomes retain their owner's borrow and retirement policy.
pub(in crate::depot) type Result<T, M> =
    std::result::Result<T, Error<<M as Mode>::Busy, <<M as Mode>::Cycle as Policy>::Retired>>;

/// A live slot currently held by another interior borrow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Busy {
    pub index: u32,
}

const _: () = assert!(mem::size_of::<Error>() == 3 * mem::size_of::<u32>());
