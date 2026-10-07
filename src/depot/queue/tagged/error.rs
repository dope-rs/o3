use crate::{
    alloc,
    depot::{capacity, queue::tagged::shape},
};

/// A tagged store's exact construction failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error<C = capacity::Count, L = capacity::Count> {
    Shape(shape::Error<C, L>),
    Allocation(alloc::Error),
}

impl<C, L> From<shape::Error<C, L>> for Error<C, L> {
    fn from(error: shape::Error<C, L>) -> Self {
        Self::Shape(error)
    }
}

impl<C, L> From<alloc::Error> for Error<C, L> {
    fn from(error: alloc::Error) -> Self {
        Self::Allocation(error)
    }
}

impl<C, L> From<alloc::Overflow> for Error<C, L> {
    fn from(error: alloc::Overflow) -> Self {
        Self::Allocation(error.into())
    }
}

const _: () = assert!(
    size_of::<Error<std::num::NonZero<u32>, std::num::NonZero<u32>>>() == size_of::<Error>()
);
