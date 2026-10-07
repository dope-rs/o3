use crate::buffer::{resident, resident::write, write::consume};

/// Why one bounded staging reservation could not be satisfied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Capacity(consume::Error),
    Resident(resident::AllocError),
    Spill {
        source: write::Cause,
        error: resident::AllocError,
    },
}

const _: () = assert!(size_of::<Error>() == 6 * size_of::<usize>());
