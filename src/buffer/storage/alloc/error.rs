use std::convert;

use crate::{alloc, mem::credit};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error<E> {
    Budget(E),
    Allocation(alloc::Exhausted),
}

impl Error<convert::Infallible> {
    pub(in crate::buffer) fn into_alloc(self) -> alloc::Exhausted {
        match self {
            Self::Allocation(error) => error,
        }
    }
}

const _: () = {
    assert!(size_of::<Error<convert::Infallible>>() == 0);
    assert!(size_of::<Error<credit::Error>>() == size_of::<credit::Error>());
};
