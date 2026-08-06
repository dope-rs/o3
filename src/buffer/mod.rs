mod bytes;
pub mod inline;
mod owned;
mod pool;
pub mod queue;
mod shared;
mod storage;
pub mod view;

use std::{
    error::Error,
    fmt,
    mem::MaybeUninit,
    ops::Range,
    ptr::{self, NonNull, copy_nonoverlapping},
    slice,
};

pub use bytes::{Borrowed, ByteSink, Bytes, RetainBytes, Retained, SliceWriter};
pub use owned::Owned;
pub use pool::{
    Cursor, FixedPoolCapacity, Frozen, Initialized, Layout, Lease, Plan, Pool, PoolCapacity,
    RuntimePoolCapacity, State, Uninitialized,
};
pub use shared::{Shared, strings::SharedStr};
use storage::refs::LocalRefCount;

use crate::ThreadBound;

/// Reports the logical byte prefix that an exclusive owner may consume.
pub trait PrefixLength {
    fn prefix_len(&self) -> usize;
}

/// Owns a logical byte prefix that can be consumed after it is proven to fit.
/// Only [`ValidatedPrefix`] can construct the proof passed to this method.
pub trait PrefixConsumer: PrefixLength {
    fn consume_validated_prefix(&mut self, proof: PrefixProof);

    fn try_consume_prefix(
        &mut self,
        amount: usize,
    ) -> Result<ValidatedPrefix<'_, Self>, CapacityError> {
        ValidatedPrefix::try_new(self, amount)
    }

    fn consume_prefix_up_to(&mut self, requested: usize) -> usize {
        let prefix = ValidatedPrefix::up_to(self, requested);
        let amount = prefix.len();
        prefix.commit();
        amount
    }
}

/// An unforgeable proof that one prefix fits its exclusively borrowed owner.
#[doc(hidden)]
pub struct PrefixProof {
    amount: usize,
}

impl PrefixProof {
    fn new(amount: usize) -> Self {
        Self { amount }
    }

    pub const fn amount(&self) -> usize {
        self.amount
    }
}

/// Proof that `amount` fits one exclusively borrowed target prefix.
#[must_use]
pub struct ValidatedPrefix<'a, T: PrefixConsumer + ?Sized> {
    target: &'a mut T,
    proof: PrefixProof,
}

impl<'a, T: PrefixConsumer + ?Sized> ValidatedPrefix<'a, T> {
    fn try_new(target: &'a mut T, amount: usize) -> Result<Self, CapacityError> {
        let available = target.prefix_len();
        if amount > available {
            return Err(CapacityError::new(amount, available));
        }
        Ok(Self {
            target,
            proof: PrefixProof::new(amount),
        })
    }

    /// Proves the largest prefix no longer than `requested`.
    fn up_to(target: &'a mut T, requested: usize) -> Self {
        let amount = requested.min(target.prefix_len());
        Self {
            target,
            proof: PrefixProof::new(amount),
        }
    }

    const fn len(&self) -> usize {
        self.proof.amount()
    }
}

impl<T: PrefixConsumer + ?Sized> ValidatedPrefix<'_, T> {
    /// Applies the validated mutation exactly once.
    pub fn commit(self) {
        self.target.consume_validated_prefix(self.proof);
    }
}

pub const BLOCK_CAPACITY: u32 = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoolLayoutError {
    ZeroCapacity,
    SlotOverflow,
    CapacityOverflow,
}

impl fmt::Display for PoolLayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroCapacity => f.write_str("buffer pool capacity must be positive"),
            Self::SlotOverflow => f.write_str("buffer pool slot count overflow"),
            Self::CapacityOverflow => f.write_str("buffer pool allocation size overflow"),
        }
    }
}

impl Error for PoolLayoutError {}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CapacityError {
    attempted: usize,
    capacity: usize,
}

impl CapacityError {
    const fn new(attempted: usize, capacity: usize) -> Self {
        Self {
            attempted,
            capacity,
        }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExactWriteError {
    expected: usize,
    actual: usize,
}

impl fmt::Display for ExactWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "exact write incomplete: expected {}, wrote {}",
            self.expected, self.actual
        )
    }
}

impl Error for ExactWriteError {}

/// An exact-length write reservation that rolls back unless committed.
/// Safe writes are confined to the reserved extent.
#[must_use = "dropping a write transaction rolls its bytes back"]
pub struct WriteTxn<'writer, 'target> {
    writer: &'writer mut SpareWriter<'target>,
    start: usize,
    end: usize,
    committed: bool,
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
    pub fn len(&self) -> usize {
        self.written
    }

    pub fn is_empty(&self) -> bool {
        self.written == 0
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.ptr.as_ptr().cast(), self.written) }
    }

    pub fn truncate(&mut self, len: usize) {
        self.written = self.written.min(len);
    }

    /// Reserves exactly `len` bytes for an all-or-nothing safe write.
    pub fn try_transaction(&mut self, len: usize) -> Result<WriteTxn<'_, 'a>, CapacityError> {
        let end = self
            .written
            .checked_add(len)
            .ok_or_else(|| CapacityError::new(usize::MAX, self.capacity))?;
        if end > self.capacity {
            return Err(CapacityError::new(end, self.capacity));
        }
        Ok(WriteTxn {
            start: self.written,
            end,
            writer: self,
            committed: false,
        })
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

    /// Appends one contiguous slice after validating its complete length.
    pub fn try_extend(&mut self, src: &[u8]) -> Result<(), CapacityError> {
        let end = self
            .written
            .checked_add(src.len())
            .ok_or_else(|| CapacityError::new(usize::MAX, self.capacity))?;
        if end > self.capacity {
            return Err(CapacityError::new(end, self.capacity));
        }
        unsafe {
            copy_nonoverlapping(
                src.as_ptr(),
                self.ptr.as_ptr().add(self.written).cast(),
                src.len(),
            )
        };
        self.written = end;
        Ok(())
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
        if self.is_empty() {
            return;
        }
        *self.target = self.target.wrapping_add(self.written as u32);
        self.written = 0;
    }
}

impl WriteTxn<'_, '_> {
    fn written(&self) -> usize {
        self.writer.written - self.start
    }

    pub fn remaining(&self) -> usize {
        self.end - self.writer.written
    }

    pub fn try_extend(&mut self, src: &[u8]) -> Result<(), CapacityError> {
        if src.len() > self.remaining() {
            return Err(CapacityError::new(
                self.written().saturating_add(src.len()),
                self.end - self.start,
            ));
        }
        self.writer.try_extend(src)
    }

    /// Returns the initialized portion of this transaction for in-place work.
    pub fn initialized_mut(&mut self) -> &mut [u8] {
        &mut self.writer.as_mut_slice()[self.start..]
    }

    /// Makes the exact initialized reservation visible in the parent writer.
    pub fn commit(mut self) -> Result<(), ExactWriteError> {
        let actual = self.written();
        let expected = self.end - self.start;
        if actual != expected {
            return Err(ExactWriteError { expected, actual });
        }
        self.committed = true;
        Ok(())
    }
}

impl Drop for WriteTxn<'_, '_> {
    fn drop(&mut self) {
        if !self.committed {
            self.writer.truncate(self.start);
        }
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
