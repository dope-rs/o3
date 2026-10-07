use crate::{alloc, buffer::segments::assembly::total, depot::capacity};

/// Exact failure while preparing a fresh retained-segment chain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Metadata(alloc::Error),
    Total(total::Error),
    Nodes {
        shortage: capacity::Shortage,
        allocation: alloc::Error,
    },
    Descriptors {
        shortage: capacity::Shortage,
        allocation: alloc::Error,
    },
}

const _: () = assert!(size_of::<Error>() == 4 * size_of::<usize>());
