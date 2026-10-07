use std::mem;

use crate::mem::credit::fair;

/// Exact cause of a failed fair-credit acquisition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cause<W: crate::mem::raw::Word, const N: usize = 1> {
    Overflow {
        axis: fair::Axis<N>,
        held: W,
        requested: W,
    },
    Exhausted {
        axis: fair::Axis<N>,
        required: W,
        available: W,
    },
}

const _: () = assert!(mem::size_of::<Cause<usize, 3>>() == 4 * mem::size_of::<usize>());
