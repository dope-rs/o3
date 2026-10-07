use std::num;

/// The exact extent rejected before acquiring or framing a pool slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Capacity {
        length: usize,
        suffix: usize,
        capacity: num::NonZero<u32>,
    },
    Prefix {
        requested: usize,
        available: u32,
    },
    Suffix {
        requested: usize,
        available: u32,
    },
    Partition {
        suffix: usize,
        length: usize,
    },
}
