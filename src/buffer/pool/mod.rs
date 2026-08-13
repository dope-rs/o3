use std::{error, fmt, io, marker, ops};

use crate::buffer::{self, bytes};

mod cursor;
pub mod state;

pub use cursor::Cursor;

#[repr(transparent)]
pub struct BorrowedCursor<'pool, C: Capacity = RuntimeCapacity> {
    cursor: Cursor<C, Borrowed<'pool>>,
}

impl<'pool, C: Capacity> BorrowedCursor<'pool, C> {
    pub(in crate::buffer) const fn new(
        lease: buffer::Lease<state::Uninitialized, C, Borrowed<'pool>>,
    ) -> Self {
        Self {
            cursor: Cursor { lease, head: 0 },
        }
    }

    #[must_use]
    pub fn freeze(self) -> bytes::Bytes<bytes::Pooled<'pool>> {
        use crate::buffer::PrefixConsumer;
        let Cursor { lease, head } = self.cursor;
        let mut bytes = bytes::Bytes::<bytes::Pooled<'pool>>::from(lease.freeze());
        let _ = PrefixConsumer::consume_prefix_up_to(&mut bytes, head as usize);
        bytes
    }
}

impl<'pool, C: Capacity> ops::Deref for BorrowedCursor<'pool, C> {
    type Target = Cursor<C, Borrowed<'pool>>;

    fn deref(&self) -> &Self::Target {
        &self.cursor
    }
}

impl<'pool, C: Capacity> ops::DerefMut for BorrowedCursor<'pool, C> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.cursor
    }
}

impl<C: Capacity> AsRef<[u8]> for BorrowedCursor<'_, C> {
    fn as_ref(&self) -> &[u8] {
        self.cursor.as_slice()
    }
}

impl<C: Capacity> buffer::PrefixLength for BorrowedCursor<'_, C> {
    fn prefix_len(&self) -> usize {
        self.cursor.len()
    }
}

impl<C: Capacity> buffer::PrefixConsumer for BorrowedCursor<'_, C> {
    fn consume_validated_prefix(&mut self, proof: buffer::PrefixProof) {
        self.cursor.consume_valid(proof.amount());
    }
}

#[doc(hidden)]
pub trait Ownership: buffer::Seal {
    const RETAINS_CORE: bool;
}

#[doc(hidden)]
pub struct Owned;

#[doc(hidden)]
pub struct Borrowed<'pool>(marker::PhantomData<&'pool ()>);

impl buffer::Seal for Owned {}
impl buffer::Seal for Borrowed<'_> {}

impl Ownership for Owned {
    const RETAINS_CORE: bool = true;
}

impl Ownership for Borrowed<'_> {
    const RETAINS_CORE: bool = false;
}

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

impl From<CreateError> for io::Error {
    fn from(error: CreateError) -> Self {
        match error {
            CreateError::Layout(error) => io::Error::new(io::ErrorKind::InvalidInput, error),
            CreateError::Allocation(_) => io::ErrorKind::OutOfMemory.into(),
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
