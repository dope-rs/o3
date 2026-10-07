use std::num::NonZero;

/// The exact capacity that rejected an incoming nonempty owner.
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Bytes {
        requested: NonZero<usize>,
        available: usize,
    },
    Segments {
        capacity: usize,
    },
}

const _: () = assert!(size_of::<Error>() == 2 * size_of::<usize>());
