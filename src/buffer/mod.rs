mod bytes;
mod inline;
mod owned;
mod pool;
mod prefix;
mod queue;
mod shared;
mod storage;

use std::error::Error;
use std::fmt;
use std::mem::MaybeUninit;
use std::ops::Range;
use std::ptr::{self, NonNull, copy_nonoverlapping};

use crate::marker::ThreadBound;
use refs::LocalRefCount;
use storage::refs;

pub use bytes::{Borrowed, ByteSink, ByteSpan, Bytes, Leased, RetainBytes, Retained, SliceWriter};
pub use inline::{INLINE_BYTES_CAPACITY, InlineBytes};
pub use owned::{BLOCK_CAPACITY, Owned};
pub use pool::shared::{
    Initialized, Pooled, SharedLease, SharedPool, SharedPoolLayout, SharedPoolPlan, Uninitialized,
};
pub use pool::{FixedPoolCapacity, Lease, Pool, PoolLayout, PoolLayoutError, RuntimePoolCapacity};
pub use prefix::{PrefixLength, ValidatedPrefix};
pub use queue::ring::ByteRing;
pub use queue::rolling::RollingBuffer;
pub use queue::{AdvanceSegment, RetainedSegmentQueue, SegmentQueue};
pub use shared::Shared;
pub use shared::snapshot::SnapshotBuf;
pub use shared::strings::SharedStr;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CapacityError {
    attempted: usize,
    capacity: usize,
}

impl CapacityError {
    pub(crate) const fn new(attempted: usize, capacity: usize) -> Self {
        Self {
            attempted,
            capacity,
        }
    }

    pub const fn attempted(self) -> usize {
        self.attempted
    }

    pub const fn capacity(self) -> usize {
        self.capacity
    }
}

impl fmt::Debug for CapacityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CapacityError")
            .field("attempted", &self.attempted)
            .field("capacity", &self.capacity)
            .finish()
    }
}

impl fmt::Display for CapacityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "capacity exceeded: attempted {}, capacity {}",
            self.attempted, self.capacity
        )
    }
}

impl Error for CapacityError {}

fn checked_append_len<const N: usize>(
    start: usize,
    capacity: usize,
    slices: &[&[u8]; N],
) -> Result<usize, CapacityError> {
    let mut end = start;
    for slice in slices {
        end = end
            .checked_add(slice.len())
            .ok_or_else(|| CapacityError::new(usize::MAX, capacity))?;
        if end > capacity {
            return Err(CapacityError::new(end, capacity));
        }
    }
    Ok(end)
}

fn write_vec_slices<const N: usize>(out: &mut Vec<u8>, slices: [&[u8]; N]) {
    let additional = slices
        .iter()
        .fold(0usize, |len, slice| len.saturating_add(slice.len()));
    let start = out.len();
    out.reserve(additional);
    let mut offset = start;
    for slice in slices {
        // SAFETY: the aggregate reserve covers every copy and safe borrowing
        // prevents the sources from aliasing this vector.
        unsafe { copy_nonoverlapping(slice.as_ptr(), out.as_mut_ptr().add(offset), slice.len()) };
        offset += slice.len();
    }
    // SAFETY: every byte in `start..offset` was initialized above.
    unsafe { out.set_len(offset) };
}

/// Failure to construct an exact-length [`Owned`] buffer.
///
/// [`Owned`]: crate::buffer::Owned
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExactBuildError<E> {
    /// The requested length exceeds the buffer representation.
    Capacity(CapacityError),
    /// The encoder returned an error.
    Build(E),
    /// The encoder completed without initializing the requested number of bytes.
    LengthMismatch { expected: usize, actual: usize },
}

impl<E: fmt::Display> fmt::Display for ExactBuildError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Capacity(error) => error.fmt(f),
            Self::Build(error) => error.fmt(f),
            Self::LengthMismatch { expected, actual } => {
                write!(
                    f,
                    "exact buffer length mismatch: expected {expected}, wrote {actual}"
                )
            }
        }
    }
}

impl<E> Error for ExactBuildError<E>
where
    E: Error + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Capacity(error) => Some(error),
            Self::Build(error) => Some(error),
            Self::LengthMismatch { .. } => None,
        }
    }
}

pub struct SpareWriter<'a> {
    ptr: NonNull<MaybeUninit<u8>>,
    capacity: usize,
    written: usize,
    target: &'a mut u32,
    _thread: ThreadBound,
}

pub enum SpareFillError<E> {
    Fill(E),
    Capacity,
}

trait RangeExt {
    fn is_within(&self, len: usize) -> bool;
}

impl RangeExt for Range<usize> {
    fn is_within(&self, len: usize) -> bool {
        self.start <= self.end && self.end <= len
    }
}

impl<'a> SpareWriter<'a> {
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.written
    }

    pub fn is_empty(&self) -> bool {
        self.written == 0
    }

    pub fn remaining(&self) -> usize {
        self.capacity - self.written
    }

    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        unsafe { self.ptr.as_ptr().add(self.written).cast() }
    }

    pub fn spare_capacity_mut(&mut self) -> &mut [MaybeUninit<u8>] {
        unsafe {
            use std::slice::from_raw_parts_mut;
            from_raw_parts_mut(self.ptr.as_ptr().add(self.written), self.remaining())
        }
    }

    pub fn try_fill<E, F>(&mut self, fill: F) -> Result<(), SpareFillError<E>>
    where
        F: for<'b> FnOnce(&'b mut [MaybeUninit<u8>]) -> Result<&'b mut [u8], E>,
    {
        let expected = self.as_mut_ptr();
        let remaining = self.remaining();
        let (initialized, len) = {
            let initialized = fill(self.spare_capacity_mut()).map_err(SpareFillError::Fill)?;
            (initialized.as_ptr(), initialized.len())
        };
        if initialized != expected || len > remaining {
            return Err(SpareFillError::Capacity);
        }
        self.written += len;
        Ok(())
    }

    pub fn try_commit_initialized(&mut self, initialized: &[u8]) -> Result<(), CapacityError> {
        let attempted = self
            .written
            .checked_add(initialized.len())
            .ok_or_else(|| CapacityError::new(usize::MAX, self.capacity))?;
        if initialized.as_ptr() != self.as_mut_ptr() || attempted > self.capacity {
            return Err(CapacityError::new(attempted, self.capacity));
        }
        self.written = attempted;
        Ok(())
    }

    pub fn try_push(&mut self, byte: u8) -> Result<(), CapacityError> {
        if self.written == self.capacity {
            return Err(CapacityError::new(self.written + 1, self.capacity));
        }
        unsafe {
            self.ptr
                .as_ptr()
                .add(self.written)
                .write(MaybeUninit::new(byte))
        };
        self.written += 1;
        Ok(())
    }

    pub fn try_extend_from_slice(&mut self, src: &[u8]) -> Result<(), CapacityError> {
        self.try_extend_from_slices([src])
    }

    /// Appends every slice after validating their aggregate length.
    ///
    /// On failure, neither the writer length nor its target length changes.
    pub fn try_extend_from_slices<const N: usize>(
        &mut self,
        slices: [&[u8]; N],
    ) -> Result<(), CapacityError> {
        let end = checked_append_len(self.written, self.capacity, &slices)?;
        let mut offset = self.written;
        for src in slices {
            unsafe {
                copy_nonoverlapping(
                    src.as_ptr(),
                    self.ptr.as_ptr().add(offset).cast(),
                    src.len(),
                )
            };
            offset += src.len();
        }
        self.written = end;
        Ok(())
    }

    pub fn finish(self) -> usize {
        self.written
    }

    unsafe fn new(ptr: *mut MaybeUninit<u8>, capacity: usize, target: &'a mut u32) -> Self {
        debug_assert!(capacity <= (u32::MAX - *target) as usize);
        Self {
            ptr: unsafe { NonNull::new_unchecked(ptr) },
            capacity,
            written: 0,
            target,
            _thread: ThreadBound::NEW,
        }
    }

    fn commit(&mut self) {
        *self.target = self.target.wrapping_add(self.written as u32);
        self.written = 0;
    }
}

impl Drop for SpareWriter<'_> {
    fn drop(&mut self) {
        self.commit();
    }
}

/// # Safety
/// `buf` is valid through `*tail`, and `*head <= *tail`.
unsafe fn compact(buf: *mut MaybeUninit<u8>, head: &mut u32, tail: &mut u32) {
    if *head == 0 {
        return;
    }
    let len = (*tail - *head) as usize;
    if len != 0 {
        unsafe { ptr::copy(buf.add(*head as usize), buf, len) };
    }
    *head = 0;
    *tail = len as u32;
}

/// # Safety
/// `amount <= *tail - *head`.
unsafe fn consume(head: &mut u32, tail: &mut u32, amount: usize) {
    *head = head.wrapping_add(amount as u32);
    if *head == *tail {
        *head = 0;
        *tail = 0;
    }
}
