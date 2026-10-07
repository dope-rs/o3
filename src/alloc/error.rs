use std::mem;

use crate::alloc::{Exhausted, Overflow};

/// Failure to validate or satisfy an allocation layout.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The allocator could not satisfy a valid layout.
    Exhausted,
    /// Capacity or layout arithmetic exceeded a representable bound.
    Overflow,
}

impl From<Exhausted> for Error {
    fn from(_: Exhausted) -> Self {
        Self::Exhausted
    }
}

impl From<Overflow> for Error {
    fn from(_: Overflow) -> Self {
        Self::Overflow
    }
}

const _: () = assert!(mem::size_of::<Error>() == 1);
const _: () = assert!(mem::size_of::<Result<(), Error>>() == 1);
