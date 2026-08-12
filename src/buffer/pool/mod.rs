use std::{error, fmt};

use crate::buffer;

mod cursor;
pub mod state;

pub use cursor::Cursor;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AllocationError;

impl fmt::Display for AllocationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("buffer pool allocation failed")
    }
}

impl error::Error for AllocationError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreateError {
    Layout(LayoutError),
    Allocation(AllocationError),
}

impl fmt::Display for CreateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Layout(error) => error.fmt(formatter),
            Self::Allocation(error) => error.fmt(formatter),
        }
    }
}

impl error::Error for CreateError {}

impl From<CreateError> for std::io::Error {
    fn from(error: CreateError) -> Self {
        match error {
            CreateError::Layout(error) => {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, error)
            }
            CreateError::Allocation(_) => std::io::ErrorKind::OutOfMemory.into(),
        }
    }
}

impl From<LayoutError> for CreateError {
    fn from(error: LayoutError) -> Self {
        Self::Layout(error)
    }
}

impl From<AllocationError> for CreateError {
    fn from(error: AllocationError) -> Self {
        Self::Allocation(error)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutError {
    ZeroCapacity,
    SlotOverflow,
    CapacityOverflow,
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroCapacity => f.write_str("buffer pool capacity must be positive"),
            Self::SlotOverflow => f.write_str("buffer pool slot count overflow"),
            Self::CapacityOverflow => f.write_str("buffer pool allocation size overflow"),
        }
    }
}

impl error::Error for LayoutError {}

/// Selects whether a pool capacity is fixed in its type or stored in its layout.
///
/// This bound is sealed because the allocator's layout proof depends on it.
#[doc(hidden)]
pub trait Capacity: buffer::Seal {}

/// Selects a capacity supplied by [`Layout`] at construction time.
#[derive(Clone, Copy)]
pub struct RuntimeCapacity;

impl buffer::Seal for RuntimeCapacity {}

impl Capacity for RuntimeCapacity {}

/// Selects a capacity fixed in the pool type.
#[derive(Clone, Copy)]
pub struct FixedCapacity<const CAP: u32>;

impl<const CAP: u32> buffer::Seal for FixedCapacity<CAP> {}

impl<const CAP: u32> Capacity for FixedCapacity<CAP> {}
