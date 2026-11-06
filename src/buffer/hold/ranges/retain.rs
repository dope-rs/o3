use crate::{alloc, mem::rc};

#[derive(Debug)]
/// Failure while retaining a completely validated compact range set.
pub enum Error {
    Metadata(alloc::Error),
    Retain {
        index: usize,
        error: rc::Error,
    },
    Resident {
        index: u32,
        current: usize,
        added: usize,
    },
}

const _: () = assert!(size_of::<Error>() <= 3 * size_of::<usize>());
