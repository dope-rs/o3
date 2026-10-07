use std::num;

/// The usable capacity and terminal reservation exceed the allocation domain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Error {
    pub capacity: num::NonZero<usize>,
    pub reserved: usize,
}

const _: () = assert!(size_of::<Error>() == 2 * size_of::<usize>());
