use std::num;

/// A preceding generation still owns resident credit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Reuse {
    pub retained: num::NonZero<usize>,
    pub previous: usize,
    pub requested: usize,
}

const _: () = assert!(size_of::<Reuse>() == size_of::<usize>() * 3);
