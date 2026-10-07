use crate::{alloc::Exhausted, buffer::storage::alloc, mem::credit};

/// Exact copy failure after the byte capacity has been proven.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Budget(credit::Error),
    Allocation(Exhausted),
}

impl From<alloc::Error<credit::Error>> for Error {
    fn from(error: alloc::Error<credit::Error>) -> Self {
        match error {
            alloc::Error::Policy(error) => Self::Budget(error),
            alloc::Error::Allocation(error) => Self::Allocation(error),
        }
    }
}

const _: () = assert!(size_of::<Error>() == size_of::<credit::Error>());
