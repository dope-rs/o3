use crate::{
    buffer::{pool, storage::alloc, write::consume},
    mem::{credit, rc},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AllocError {
    Capacity(consume::Error),
    Budget(credit::Error),
    Retain(rc::Error),
    Layout(pool::LayoutError),
    Exhausted(crate::alloc::Exhausted),
}

impl From<alloc::Error<credit::Error>> for AllocError {
    fn from(error: alloc::Error<credit::Error>) -> Self {
        match error {
            alloc::Error::Policy(error) => Self::Budget(error),
            alloc::Error::Allocation(error) => Self::Exhausted(error),
        }
    }
}
