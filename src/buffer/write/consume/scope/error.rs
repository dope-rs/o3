use std::num::NonZero;

use crate::buffer::storage::range;

/// Rejected prefix before a plan or branded end position is produced.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Maximum { count: NonZero<usize>, maximum: u32 },
    Range(range::Error),
}
