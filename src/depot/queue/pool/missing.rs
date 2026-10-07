use std::mem;

/// A requested lane outside this pool's allocated domain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Missing {
    pub index: usize,
    pub lanes: usize,
}

const _: () = assert!(mem::size_of::<Missing>() == 2 * mem::size_of::<usize>());
